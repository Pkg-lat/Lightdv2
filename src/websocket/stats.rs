//! Stats collector for container resource monitoring
//! 
//! Collects real-time stats from Docker containers and broadcasts changes.

use bollard::container::StatsOptions;
use bollard::Docker;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
//use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::event_hub::{ContainerStats, EventHub, NetworkStats};
use crate::container::manager::ContainerManager;

/// Stats collector that monitors container resources
pub struct StatsCollector {
    docker: Arc<Docker>,
    manager: Arc<ContainerManager>,
    event_hub: Arc<EventHub>,
}

impl StatsCollector {
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
    
    /// Start collecting stats for a container
    pub async fn start_collecting(&self, internal_id: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get container state
        let state = self.manager.get_container(&internal_id).await?
            .ok_or("Container not found")?;
        
        let container_id = state.container_id.ok_or("Container not ready")?;
        let memory_limit = state.limits.memory.unwrap_or(0) as u64;
        
        let docker = self.docker.clone();
        let event_hub = self.event_hub.clone();
        let internal_id_clone = internal_id.clone();
        
        // Get or create the channel
        let (channel, _) = event_hub.get_or_create_channel(&internal_id);
        
        // Spawn the stats collection task
        tokio::spawn(async move {
            Self::collect_stats_loop(
                docker,
                container_id,
                internal_id_clone,
                event_hub,
                channel,
                memory_limit,
            ).await;
        });
        
        Ok(())
    }
    
    /// Main stats collection loop
    async fn collect_stats_loop(
        docker: Arc<Docker>,
        container_id: String,
        internal_id: String,
        event_hub: Arc<EventHub>,
        channel: Arc<super::event_hub::ContainerEventChannel>,
        memory_limit: u64,
    ) {
        tracing::info!("Starting stats collector for container {}", internal_id);
        
        let mut backoff = Duration::from_millis(500);
        #[allow(unused)]
        let mut last_cpu_total: Option<u64> = None;
        #[allow(unused)]
        let mut last_system_cpu: Option<u64> = None;
        
        loop {
            // Check if container exists
            let container_info = match docker.inspect_container(&container_id, None).await {
                Ok(info) => info,
                Err(e) => {
                    debug!("Container {} not found: {}", internal_id, e);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                    continue;
                }
            };
            
            let is_running = container_info.state
                .as_ref()
                .and_then(|s| s.running)
                .unwrap_or(false);
            
            if !is_running {
                debug!("Container {} not running", internal_id);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(5));
                continue;
            }
            
            backoff = Duration::from_millis(500);
            
            // Get stats stream
            let stats_opts = StatsOptions {
                stream: true,
                one_shot: false,
            };
            
            let mut stats_stream = docker.stats(&container_id, Some(stats_opts));
            
            while let Some(result) = stats_stream.next().await {
                match result {
                    Ok(stats) => {
                        // Calculate CPU percentage
                        let cpu_total = stats.cpu_stats.cpu_usage.total_usage;
                        let precpu_total = stats.precpu_stats.cpu_usage.total_usage;
                        
                        let cpu_absolute = {
                            let cpu_delta = cpu_total as i64 - precpu_total as i64;
                            let system_delta = stats.cpu_stats.system_cpu_usage.unwrap_or(0) as i64 
                                - stats.precpu_stats.system_cpu_usage.unwrap_or(0) as i64;
                            
                            if system_delta > 0 && cpu_delta > 0 {
                                let num_cpus = stats.cpu_stats.online_cpus.unwrap_or(1) as f64;
                                (cpu_delta as f64 / system_delta as f64) * num_cpus * 100.0
                            } else {
                                0.0
                            }
                        };
                        
                        // Get memory stats
                        let memory_bytes = stats.memory_stats.usage.unwrap_or(0);
                        let memory_limit_bytes = stats.memory_stats.limit.unwrap_or(memory_limit);
                        
                        // Get network stats
                        let (rx_bytes, tx_bytes) = if let Some(networks) = &stats.networks {
                            networks.values().fold((0u64, 0u64), |acc, net| {
                                (acc.0 + net.rx_bytes, acc.1 + net.tx_bytes)
                            })
                        } else {
                            (0, 0)
                        };
                        
                        // Calculate uptime
                        let uptime = {
                            let start = channel.uptime_start.read().await;
                            if let Some(start_time) = *start {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs();
                                now.saturating_sub(start_time)
                            } else {
                                0
                            }
                        };
                        
                        // Get state
                        let state_str = channel.get_state().await.to_string();
                        
                        // Build stats object
                        let container_stats = ContainerStats {
                            memory_bytes,
                            memory_limit_bytes,
                            cpu_absolute: (cpu_absolute * 100.0).round() / 100.0, // Round to 2 decimals
                            network: NetworkStats {
                                rx_bytes,
                                tx_bytes,
                            },
                            uptime,
                            state: state_str,
                            disk_bytes: 0, // TODO: Implement disk stats
                        };
                        
                        // Broadcast (with change detection)
                        event_hub.broadcast_stats(&internal_id, container_stats).await;
                    }
                    Err(e) => {
                        warn!("Stats error for {}: {}", internal_id, e);
                        break;
                    }
                }
                
                // Small sleep to avoid overwhelming
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            
            tracing::info!("Stats stream ended for {}", internal_id);
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}
