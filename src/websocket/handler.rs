//! WebSocket handler for container connections
//! 
//! Handles WebSocket connections and routes messages to appropriate handlers.
 #[allow(unused)]
use axum::{

    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade, CloseFrame},
        Path, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
 #[allow(unused)]
use tracing::{debug, error, info, warn};
 #[allow(unused)]
use super::event_hub::{EventHub, InboundEvent, OutboundEvent, ContainerRuntimeState};
use super::console::ConsoleStreamer;
use super::stats::StatsCollector;
use crate::auth::tokens::TokenManager;
use crate::container::manager::ContainerManager;
use crate::container::power::{PowerManager, PowerAction};

#[derive(Deserialize)]
pub struct WebSocketQuery {
    token: String,
}

/// WebSocket handler state
#[derive(Clone)]
pub struct WebSocketState {
    pub manager: Arc<ContainerManager>,
    pub power: Arc<PowerManager>,
    pub event_hub: Arc<EventHub>,
    pub console_streamer: Arc<ConsoleStreamer>,
    pub stats_collector: Arc<StatsCollector>,
    pub token_manager: Arc<TokenManager>,
}

/// Handle WebSocket upgrade request
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(internal_id): Path<String>,
    Query(query): Query<WebSocketQuery>,
    State(state): State<WebSocketState>,
) -> Response {
    tracing::info!("WebSocket upgrade request for container: {}", internal_id);
    
    // Validate token
    match state.token_manager.validate_token(&query.token, true) {
        Ok(true) => {
            tracing::info!("Token validated for WebSocket connection: {}", internal_id);
            ws.on_upgrade(move |socket| handle_socket(socket, internal_id, state, query.token))
        }
        Ok(false) => {
            tracing::warn!("Invalid or expired token for WebSocket: {}", internal_id);
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or expired token"
            ).into_response()
        }
        Err(e) => {
            tracing::error!("Token validation error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Token validation failed"
            ).into_response()
        }
    }
}

/// Handle the actual WebSocket connection
async fn handle_socket(socket: WebSocket, internal_id: String, state: WebSocketState, token: String) {
    tracing::info!("WebSocket connected for container: {}", internal_id);
    
    // Verify container exists
    let container = match state.manager.get_container(&internal_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            tracing::error!("Container not found: {}", internal_id);
            return;
        }
        Err(e) => {
            tracing::error!("Error getting container: {}", e);
            return;
        }
    };
    
    // Get or create event channel
    let (channel, _) = state.event_hub.get_or_create_channel(&internal_id);
    
    // Subscribe to events
    let mut event_rx = channel.subscribe();
    
    // Start console streaming if container is ready
    if container.container_id.is_some() {
        if let Err(e) = state.console_streamer.start_streaming(internal_id.clone()).await {
            tracing::warn!("Failed to start console streaming: {}", e);
        }
        
        if let Err(e) = state.stats_collector.start_collecting(internal_id.clone()).await {
            tracing::warn!("Failed to start stats collection: {}", e);
        }
    }
    
    // Split the socket
    let (mut sender, mut receiver) = socket.split();
    
    // Clone state for the receiver task
    let state_recv = state.clone();
    let internal_id_recv = internal_id.clone();
    let channel_recv = channel.clone();
    
    // Spawn task to handle incoming messages
    let recv_task = tokio::spawn(async move {
        while let Some(result) = receiver.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    debug!("Received: {}", text);
                    
                    // Parse the message
                    match serde_json::from_str::<InboundEvent>(&text) {
                        Ok(event) => {
                            handle_inbound_event(
                                event,
                                &internal_id_recv,
                                &state_recv,
                                &channel_recv,
                            ).await;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse message: {} - {}", e, text);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    tracing::info!("Client closed connection for {}", internal_id_recv);
                    break;
                }
                Ok(Message::Ping(_data)) => {
                    // Handled automatically by axum
                    debug!("Ping received");
                }
                Ok(_) => {
                    // Other message types (binary, pong)
                }
                Err(e) => {
                    tracing::error!("Error receiving message: {}", e);
                    break;
                }
            }
        }
    });
    
    // Spawn task to handle outgoing messages
    let token_manager_send = state.token_manager.clone();
    let token_clone = token.clone();
    let send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            // Check if token is still valid
            match token_manager_send.validate_token(&token_clone, false) {
                Ok(false) | Err(_) => {
                    tracing::warn!("Token expired during WebSocket connection, closing");
                    break;
                }
                Ok(true) => {
                    // Token still valid, continue
                }
            }
            
            let json = match serde_json::to_string(&event) {
                Ok(j) => j,
                Err(e) => {
                    tracing::error!("Failed to serialize event: {}", e);
                    continue;
                }
            };
            
            if sender.send(Message::Text(json.into())).await.is_err() {
                // Client disconnected
                break;
            }
        }
    });
    
    // Wait for either task to complete
    tokio::select! {
        _ = recv_task => {
            debug!("Receiver task ended for {}", internal_id);
        }
        _ = send_task => {
            debug!("Sender task ended for {}", internal_id);
        }
    }
    
    tracing::info!("WebSocket disconnected for container: {}", internal_id);
}

/// Handle an inbound event from the client
async fn handle_inbound_event(
    event: InboundEvent,
    internal_id: &str,
    state: &WebSocketState,
    channel: &Arc<super::event_hub::ContainerEventChannel>,
) {
    match event {
        InboundEvent::Power { power: args } => {
            if args.is_empty() {
                tracing::warn!("Power event with no action");
                return;
            }
            
            let action = args[0].to_lowercase();
            tracing::info!("Power action for {}: {}", internal_id, action);
            
            let power_action = match action.as_str() {
                "start" => {
                    // Set state to starting
                    channel.set_state(ContainerRuntimeState::Starting).await;
                    state.event_hub.broadcast_event(internal_id, "starting").await;
                    Some(PowerAction::Start)
                }
                "kill" => {
                    channel.set_state(ContainerRuntimeState::Stopping).await;
                    state.event_hub.broadcast_event(internal_id, "stopping").await;
                    Some(PowerAction::Kill)
                }
                "restart" => {
                    channel.set_state(ContainerRuntimeState::Stopping).await;
                    state.event_hub.broadcast_event(internal_id, "stopping").await;
                    Some(PowerAction::Restart)
                }
                _ => {
                    tracing::warn!("Unknown power action: {}", action);
                    None
                }
            };
            
            if let Some(pa) = power_action {
                match state.power.execute_action(internal_id.to_string(), pa).await {
                    Ok(_) => {},
                    Err(e) => {
                        let error_msg = format!("Power action failed: {}", e);
                        tracing::error!("{}", error_msg);
                        state.event_hub.broadcast_daemon_message(internal_id, &error_msg).await;
                    }
                }
            }
        }
        
        InboundEvent::SendCommand { send_command: args } => {
            if args.is_empty() {
                tracing::warn!("SendCommand with no command");
                return;
            }
            
            let command = &args[0];
            tracing::info!("Sending command to {}: {}", internal_id, command);
            
            if let Err(e) = channel.send_command(command.clone()) {
                tracing::error!("Failed to send command: {}", e);
            }
        }
        
        InboundEvent::RequestLogs { logs: args } => {
            let count: usize = args.get(0)
                .and_then(|s| s.parse().ok())
                .unwrap_or(50);
            
            tracing::info!("Requesting {} logs for {}", count, internal_id);
            state.event_hub.send_logs(internal_id, count).await;
        }
    }
}

/// Handle container lifecycle events (called from lifecycle manager)
pub async fn notify_installing(event_hub: &EventHub, internal_id: &str) {
    event_hub.broadcast_event(internal_id, "installing").await;
}

pub async fn notify_installed(event_hub: &EventHub, internal_id: &str) {
    event_hub.broadcast_event(internal_id, "installed").await;
}
