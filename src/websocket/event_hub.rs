//! WebSocket Event Hub for Container Management
//! 
//! This module provides a centralized event broadcasting system for container
//! WebSocket connections, similar to Pterodactyl Wings also the name is very funny.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Events that can be sent TO the WebSocket clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "args")]
pub enum OutboundEvent {
    /// Container stats update
    #[serde(rename = "stats")]
    Stats(Vec<String>),
    
    /// Console output from container
    #[serde(rename = "console output")]
    ConsoleOutput(Vec<String>),
    
    /// Duplicate console output (line repeated N times)
    #[serde(rename = "console duplicate")]
    ConsoleDuplicate(Vec<String>),
    
    /// Container lifecycle events (installing, installed, exit, etc.)
    #[serde(rename = "event")]
    Event(Vec<String>),
    
    /// Daemon messages (Container stopped, etc.)
    #[serde(rename = "daemon_message")]
    DaemonMessage(Vec<String>),
    
    /// Response to logs request
    #[serde(rename = "logs")]
    Logs(Vec<String>),
}

/// Events that can be received FROM WebSocket clients
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum InboundEvent {
    /// Power action: start, kill, restart
    Power { power: Vec<String> },
    
    /// Send command to container stdin
    SendCommand { send_command: Vec<String> },
    
    /// Request last N lines of logs
    RequestLogs { logs: Vec<String> },
}

/// Container stats data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub memory_bytes: u64,
    pub memory_limit_bytes: u64,
    pub cpu_absolute: f64,
    pub network: NetworkStats,
    pub uptime: u64,
    pub state: String,
    pub disk_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// Container runtime state for tracking running state
#[derive(Debug, Clone, PartialEq)]
pub enum ContainerRuntimeState {
    Offline,
    Starting,
    Running,
    Stopping,
}

impl ToString for ContainerRuntimeState {
    fn to_string(&self) -> String {
        match self {
            ContainerRuntimeState::Offline => "offline".to_string(),
            ContainerRuntimeState::Starting => "starting".to_string(),
            ContainerRuntimeState::Running => "running".to_string(),
            ContainerRuntimeState::Stopping => "stopping".to_string(),
        }
    }
}

/// Per-container event hub that multiple WebSocket clients can subscribe to
pub struct ContainerEventChannel {
    /// Broadcast channel for events going to clients
    pub event_tx: broadcast::Sender<OutboundEvent>,
    /// Channel for commands coming from clients
    pub command_tx: mpsc::UnboundedSender<String>,
    /// Current runtime state
    pub state: RwLock<ContainerRuntimeState>,
    /// Last known stats (for change detection)
    pub last_stats: RwLock<Option<ContainerStats>>,
    /// Log buffer (circular, stores last 1000 lines)
    pub log_buffer: RwLock<Vec<String>>,
    /// Start pattern (regex or plain text to detect server started)
    pub start_pattern: RwLock<Option<String>>,
    /// Container uptime start timestamp
    pub uptime_start: RwLock<Option<u64>>,
}

impl ContainerEventChannel {
    pub fn new(command_tx: mpsc::UnboundedSender<String>) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            event_tx,
            command_tx,
            state: RwLock::new(ContainerRuntimeState::Offline),
            last_stats: RwLock::new(None),
            log_buffer: RwLock::new(Vec::with_capacity(1000)),
            start_pattern: RwLock::new(None),
            uptime_start: RwLock::new(None),
        }
    }
    
    /// Subscribe to events for this container
    pub fn subscribe(&self) -> broadcast::Receiver<OutboundEvent> {
        self.event_tx.subscribe()
    }
    
    /// Send a command to the container
    pub fn send_command(&self, command: String) -> Result<(), mpsc::error::SendError<String>> {
        self.command_tx.send(command)
    }
    
    /// Add a log line to the buffer
    pub async fn add_log(&self, line: String) {
        let mut buffer = self.log_buffer.write().await;
        if buffer.len() >= 1000 {
            buffer.remove(0);
        }
        buffer.push(line);
    }
    
    /// Get last N log lines
    pub async fn get_logs(&self, count: usize) -> Vec<String> {
        let buffer = self.log_buffer.read().await;
        let start = if buffer.len() > count { buffer.len() - count } else { 0 };
        buffer[start..].to_vec()
    }
    
    /// Set the start pattern for detecting when server is ready
    #[allow(unused)]
    pub async fn set_start_pattern(&self, pattern: Option<String>) {
        let mut pat = self.start_pattern.write().await;
        *pat = pattern;
    }
    
    /// Get current state
    pub async fn get_state(&self) -> ContainerRuntimeState {
        self.state.read().await.clone()
    }
    
    /// Set runtime state
    pub async fn set_state(&self, new_state: ContainerRuntimeState) {
        let mut state = self.state.write().await;
        *state = new_state;
    }
}

/// Global event hub managing all container channels
pub struct EventHub {
    /// Map of internal_id -> ContainerEventChannel
    channels: DashMap<String, Arc<ContainerEventChannel>>,
}

impl EventHub {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }
    
    /// Get or create a channel for a container
    pub fn get_or_create_channel(
        &self,
        internal_id: &str,
    ) -> (Arc<ContainerEventChannel>, mpsc::UnboundedReceiver<String>) {
        if let Some(channel) = self.channels.get(internal_id) {
            // Return existing channel with a new dummy receiver
            // (commands will go through existing channel)
            let (_, rx) = mpsc::unbounded_channel();
            (channel.clone(), rx)
        } else {
            let (command_tx, command_rx) = mpsc::unbounded_channel();
            let channel = Arc::new(ContainerEventChannel::new(command_tx));
            self.channels.insert(internal_id.to_string(), channel.clone());
            (channel, command_rx)
        }
    }
    
    /// Get an existing channel (if any)
    pub fn get_channel(&self, internal_id: &str) -> Option<Arc<ContainerEventChannel>> {
        self.channels.get(internal_id).map(|c| c.clone())
    }
    
    /// Remove a channel
    #[allow(unused)]
    pub fn remove_channel(&self, internal_id: &str) {
        self.channels.remove(internal_id);
    }
    
    /// Broadcast console output to a container's channel
    pub async fn broadcast_console(&self, internal_id: &str, line: &str) {
        if let Some(channel) = self.channels.get(internal_id) {
            // Add to log buffer
            channel.add_log(line.to_string()).await;
            
            // Check for start pattern
            let state = channel.get_state().await;
            if state == ContainerRuntimeState::Starting {
                let pattern = channel.start_pattern.read().await;
                if let Some(ref pat) = *pattern {
                    if line.contains(pat) || Self::match_pattern(pat, line) {
                        channel.set_state(ContainerRuntimeState::Running).await;
                        let _ = channel.event_tx.send(OutboundEvent::Event(vec!["running".to_string()]));
                    }
                }
            }
            
            // Broadcast
            let _ = channel.event_tx.send(OutboundEvent::ConsoleOutput(vec![line.to_string()]));
        }
    }
    
    /// Broadcast duplicate console output
    pub async fn broadcast_console_duplicate(&self, internal_id: &str, count: u32) {
        if let Some(channel) = self.channels.get(internal_id) {
            let _ = channel.event_tx.send(OutboundEvent::ConsoleDuplicate(vec![count.to_string()]));
        }
    }
    
    /// Broadcast stats update (only if changed significantly)
    pub async fn broadcast_stats(&self, internal_id: &str, stats: ContainerStats) {
        if let Some(channel) = self.channels.get(internal_id) {
            let should_send = {
                let last = channel.last_stats.read().await;
                match &*last {
                    Some(prev) => Self::stats_changed(prev, &stats),
                    None => true,
                }
            };
            
            if should_send {
                let mut last = channel.last_stats.write().await;
                *last = Some(stats.clone());
                
                let stats_json = serde_json::to_string(&stats).unwrap_or_default();
                let _ = channel.event_tx.send(OutboundEvent::Stats(vec![stats_json]));
            }
        }
    }
    
    /// Broadcast lifecycle event
    pub async fn broadcast_event(&self, internal_id: &str, event: &str) {
        if let Some(channel) = self.channels.get(internal_id) {
            let _ = channel.event_tx.send(OutboundEvent::Event(vec![event.to_string()]));
        }
    }
    
    /// Broadcast daemon message
    pub async fn broadcast_daemon_message(&self, internal_id: &str, message: &str) {
        if let Some(channel) = self.channels.get(internal_id) {
            let _ = channel.event_tx.send(OutboundEvent::DaemonMessage(vec![message.to_string()]));
        }
    }
    
    /// Send logs response
    pub async fn send_logs(&self, internal_id: &str, count: usize) {
        if let Some(channel) = self.channels.get(internal_id) {
            let logs = channel.get_logs(count).await;
            let _ = channel.event_tx.send(OutboundEvent::Logs(logs));
        }
    }
    
    /// Check if stats changed enough to warrant sending
    fn stats_changed(prev: &ContainerStats, new: &ContainerStats) -> bool {
        // Always send if state changed
        if prev.state != new.state {
            return true;
        }
        
        // Send if CPU changed by more than 0.5%
        if (prev.cpu_absolute - new.cpu_absolute).abs() > 0.5 {
            return true;
        }
        
        // Send if memory changed by more than 1MB
        let mem_diff = if prev.memory_bytes > new.memory_bytes {
            prev.memory_bytes - new.memory_bytes
        } else {
            new.memory_bytes - prev.memory_bytes
        };
        if mem_diff > 1_048_576 {
            return true;
        }
        
        // Send if network changed by more than 10KB
        let net_rx_diff = if prev.network.rx_bytes > new.network.rx_bytes {
            prev.network.rx_bytes - new.network.rx_bytes
        } else {
            new.network.rx_bytes - prev.network.rx_bytes
        };
        let net_tx_diff = if prev.network.tx_bytes > new.network.tx_bytes {
            prev.network.tx_bytes - new.network.tx_bytes
        } else {
            new.network.tx_bytes - prev.network.tx_bytes
        };
        if net_rx_diff > 10_240 || net_tx_diff > 10_240 {
            return true;
        }
        
        false
    }
    
    /// Match pattern against line (supports simple regex)
    fn match_pattern(pattern: &str, line: &str) -> bool {
        if let Ok(re) = regex::Regex::new(pattern) {
            re.is_match(line)
        } else {
            line.contains(pattern)
        }
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}
