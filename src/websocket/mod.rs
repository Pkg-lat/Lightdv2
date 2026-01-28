//! WebSocket module for container management
//! 
//! Provides real-time communication with containers similar to Pterodactyl Wings.
//! 
//! ## Events
//! 
//! ### Outbound (server -> client)
//! - `stats` - Container resource stats (CPU, memory, network, uptime)
//! - `console output` - Console output from container
//! - `console duplicate` - Duplicate line count
//! - `event` - Lifecycle events (installing, installed, exit, starting, running, stopping)
//! - `daemon_message` - Daemon messages (Container stopped, etc.)
//! - `logs` - Response to logs request
//! 
//! ### Inbound (client -> server)
//! - `power` - Power actions (start, kill, restart)
//! - `send command` - Send command to container stdin
//! - `logs` - Request last N lines of logs
//!  Need this to write the docs

pub mod event_hub;
pub mod console;
pub mod stats;
pub mod handler;
#[allow(unused)]
pub use event_hub::{EventHub, OutboundEvent, InboundEvent, ContainerStats, ContainerRuntimeState};
pub use console::ConsoleStreamer;
pub use stats::StatsCollector;
pub use handler::{ws_handler, WebSocketState, notify_installing, notify_installed};
