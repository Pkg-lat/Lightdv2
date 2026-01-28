//! Console streamer for container log streaming
//! 
//! Attaches to Docker container to stream stdout/stderr and send stdin commands.

use bollard::container::{AttachContainerOptions, LogsOptions, LogOutput};
use bollard::Docker;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::{debug};

use super::event_hub::{EventHub, ContainerRuntimeState};
use crate::container::manager::ContainerManager;

/// Check if a container is running
async fn is_container_running(docker: &Docker, container_id: &str) -> bool {
    match docker.inspect_container(container_id, None).await {
        Ok(info) => {
            info.state
                .and_then(|s| s.running)
                .unwrap_or(false)
        }
        Err(_) => false,
    }
}

/// Get container start timestamp
async fn get_container_started_at(docker: &Docker, container_id: &str) -> Option<i64> {
    match docker.inspect_container(container_id, None).await {
        Ok(info) => {
            info.state
                .and_then(|s| s.started_at)
                .and_then(|ts| {
                    // Parse ISO 8601 timestamp
                    chrono::DateTime::parse_from_rfc3339(&ts)
                        .ok()
                        .map(|dt| dt.timestamp())
                })
        }
        Err(_) => None,
    }
}

/// Console streamer that manages stdin/stdout for a container
pub struct ConsoleStreamer {
    docker: Arc<Docker>,
    manager: Arc<ContainerManager>,
    event_hub: Arc<EventHub>,
}

#[allow(unused_mut)]
impl ConsoleStreamer {
    pub fn new(
        manager: Arc<ContainerManager>,
        event_hub: Arc<EventHub>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let docker = Arc::new(Docker::connect_with_local_defaults()?);
        
        Ok(Self {
            docker,
            manager,
            event_hub,
        })
    }
    
    /// Start streaming for a container (called when WebSocket connects)
    pub async fn start_streaming(&self, internal_id: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get container state
        let state = self.manager.get_container(&internal_id).await?
            .ok_or("Container not found")?;
        
        let container_id = state.container_id.ok_or("Container not ready")?;
        let start_pattern = state.start_pattern.clone();
        
        let docker = self.docker.clone();
        let event_hub = self.event_hub.clone();
        let internal_id_clone = internal_id.clone();
        
        // Get or create the channel
        let (_channel, mut command_rx) = event_hub.get_or_create_channel(&internal_id);
        
        // Spawn the streaming task
        tokio::spawn(async move {
            Self::stream_logs_attached(
                docker,
                container_id,
                internal_id_clone,
                event_hub,
                command_rx,
                start_pattern,
            ).await;
        });
        
        Ok(())
    }
    
    /// Stream logs in attached mode - uses docker attach for stdin + docker logs for output
    async fn stream_logs_attached(
        docker: Arc<Docker>,
        container_id: String,
        internal_id: String,
        event_hub: Arc<EventHub>,
        mut input_rx: mpsc::UnboundedReceiver<String>,
        start_pattern: Option<String>,
    ) {
        let mut last_line: Option<String> = None;
        let mut duplicate_count: u32 = 0;
        let mut pattern_matched = false;

        tracing::info!("Starting log streamer for container {}", internal_id);
        
        // Compile regex if pattern provided
        let pattern_regex = start_pattern.as_ref().and_then(|p| {
            regex::Regex::new(p).ok()
        });

        // Spawn a task for stdin handling (attach for input only)
        let docker_input = docker.clone();
        let cid_input = container_id.clone();
        let internal_id_input = internal_id.clone();
        
        let _stdin_task = tokio::spawn(async move {
            loop {
                // Wait for container to be running before attaching for stdin
                if !is_container_running(&docker_input, &cid_input).await {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }

                let attach_opts = AttachContainerOptions::<String> {
                    // Ma4z caught this, the stderr and stdout were on false
                    stdin: Some(true),
                    stdout: Some(true),
                    stderr: Some(true),
                    stream: Some(true),
                    logs: Some(false),
                    ..Default::default()
                };

                match docker_input.attach_container(&cid_input, Some(attach_opts)).await {
                    Ok(attached) => {
                       // tracing::info!("Attached stdin to container {}", internal_id_input);
                       // Only for logs 
                        let mut input = attached.input;
                        
                        while let Some(command) = input_rx.recv().await {
                            tracing::info!("Sending command to container {}: {}", internal_id_input, command);
                            let payload = format!("{}\n", command);
                            if let Err(e) = input.write_all(payload.as_bytes()).await {
                                tracing::error!("Failed to write to stdin for {}: {}", internal_id_input, e);
                                break;
                            }
                            let _ = input.flush().await;
                        }
                    }
                    Err(e) => {
                        debug!("Failed to attach stdin to {}: {}", internal_id_input, e);
                    }
                }
                
                // If we get here, either attach failed or container stopped
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });

        // Track if we've seen the container running
        let mut was_running = false;

        // Main log streaming loop using docker logs API with follow=true
        let mut backoff = Duration::from_millis(100);
        let mut log_count: u64 = 0;

        loop {
            // Check if container exists and is running
            let running = is_container_running(&docker, &container_id).await;
            
            if !running {
                if was_running {
                    // Container just stopped
                    tracing::info!("Container {} stopped", internal_id);
                    event_hub.broadcast_event(&internal_id, "exit").await;
                    event_hub.broadcast_daemon_message(&internal_id, "Container stopped").await;
                    
                    // Update state
                    if let Some(channel) = event_hub.get_channel(&internal_id) {
                        channel.set_state(ContainerRuntimeState::Offline).await;
                    }
                    was_running = false;
                }
                
                debug!("Container {} not running, waiting...", internal_id);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(2));
                continue;
            }

            if !was_running {
                // Container just started
                tracing::info!("Container {} is now running", internal_id);
                was_running = true;
                
                // Update state to starting (will become running when pattern matches)
                if let Some(channel) = event_hub.get_channel(&internal_id) {
                    channel.set_state(ContainerRuntimeState::Starting).await;
                    
                    // Set uptime start
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    *channel.uptime_start.write().await = Some(now);
                }
            }

            tracing::info!("container {} is running, starting log stream (follow=true)", internal_id);
            backoff = Duration::from_millis(100);

            // Get container start time for filtering
            let since = get_container_started_at(&docker, &container_id).await.unwrap_or(0);

            let log_opts = LogsOptions::<String> {
                follow: true,
                stdout: true,
                stderr: true,
                since,
                timestamps: false,
                tail: "0".to_string(), // Don't replay old logs, just stream new ones
                ..Default::default()
            };

            let mut log_stream = docker.logs(&container_id, Some(log_opts));

            while let Some(result) = log_stream.next().await {
                match result {
                    Ok(log_output) => {
                        let message_bytes = match log_output {
                            LogOutput::StdOut { message } |
                            LogOutput::StdErr { message } |
                            LogOutput::Console { message } |
                            LogOutput::StdIn { message } => message,
                        };

                        let message = String::from_utf8_lossy(&message_bytes);
                        for line in message.lines() {
                            let line = line.trim_end();
                            if !line.is_empty() {
                                log_count += 1;
                                debug!("Container {} log #{}: {}", internal_id, log_count, line);
                                
                                // Check for start pattern match
                                if !pattern_matched {
                                    if let Some(ref regex) = pattern_regex {
                                        if regex.is_match(line) {
                                            pattern_matched = true;
                                            tracing::info!("Server marked as running, start up pattern matched. for {}: {}", internal_id, line);
                                            
                                            // Transition to running state
                                            if let Some(channel) = event_hub.get_channel(&internal_id) {
                                                channel.set_state(ContainerRuntimeState::Running).await;
                                            }
                                            event_hub.broadcast_event(&internal_id, "running").await;
                                            event_hub.broadcast_daemon_message(&internal_id, "Server started").await;
                                        }
                                    }
                                }
                                
                                // Check for duplicates
                                if let Some(ref last) = last_line {
                                    if last == line {
                                        duplicate_count += 1;
                                        event_hub.broadcast_console_duplicate(&internal_id, duplicate_count).await;
                                        continue;
                                    }
                                }

                                last_line = Some(line.to_string());
                                duplicate_count = 1;
                                event_hub.broadcast_console(&internal_id, line).await;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Log stream error for {}: {}", internal_id, e);
                        break;
                    }
                }
            }

            tracing::info!("Log stream ended for {} (total {} logs)", internal_id, log_count);
            
            // Small delay before retry
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
