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
use crate::auth::principal::{secret_eq, Principal};
use crate::auth::tokens::TokenManager;
use crate::container::manager::ContainerManager;
use crate::container::power::{PowerManager, PowerAction};
use crate::container::validation::validate_identifier;

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
    /// Master token from config, accepted for the upgrade. Without this the panel's
    /// own admin credential could not open a console.
    pub api_token: String,
}

/// Handle WebSocket upgrade request
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(internal_id): Path<String>,
    Query(query): Query<WebSocketQuery>,
    State(state): State<WebSocketState>,
) -> Response {
    tracing::info!("WebSocket upgrade request for container: {}", internal_id);

    // Validate the path parameter before it is used as a sled key.
    if let Err(e) = validate_identifier("container id", &internal_id) {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }

    // Resolve the token to a principal. The master token is accepted directly (in
    // constant time); otherwise the token must be a minted, unexpired token, whose
    // owner scopes which containers are reachable.
    let principal = {
        let master = std::env::var("LIGHTD_API_TOKEN").unwrap_or_else(|_| state.api_token.clone());
        if !master.is_empty() && secret_eq(&query.token, &master) {
            Some(Principal::admin())
        } else {
            match state.token_manager.validate_token(&query.token, false) {
                Ok(Some(data)) if data.is_admin() => Some(Principal::admin()),
                Ok(Some(data)) => Some(Principal::tenant(data.owner)),
                Ok(None) => None,
                Err(e) => {
                    tracing::error!("WebSocket token validation error: {}", e);
                    None
                }
            }
        }
    };

    let principal = match principal {
        Some(principal) => principal,
        None => {
            tracing::warn!("WebSocket upgrade rejected: invalid or expired token");
            return (
                StatusCode::UNAUTHORIZED,
                "Invalid or expired token"
            ).into_response();
        }
    };

    // Authorize against the container *before* upgrading. Previously the handler
    // checked only that the container existed, so any valid token could read the
    // console of, write stdin to, and power-cycle every container on the host.
    match state.manager.get_container(&internal_id).await {
        Ok(Some(container)) if principal.can_access(&container) => {}
        Ok(_) => {
            // 404 rather than 403: do not confirm that another tenant's id exists.
            tracing::warn!("WebSocket upgrade rejected: container not accessible");
            return (StatusCode::NOT_FOUND, "Container not found").into_response();
        }
        Err(e) => {
            tracing::error!("Error loading container for upgrade: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Lookup failed").into_response();
        }
    }

    ws.on_upgrade(move |socket| handle_socket(socket, internal_id, state, principal))
}

/// Handle the actual WebSocket connection
async fn handle_socket(socket: WebSocket, internal_id: String, state: WebSocketState, principal: Principal) {
    tracing::info!("WebSocket connected for container: {}", internal_id);

    // Verify the container still exists and the principal still owns it. The
    // upgrade already checked this, but the connection lives for a long time and a
    // container can be reassigned in between.
    let container = match state.manager.get_container(&internal_id).await {
        Ok(Some(c)) if principal.can_access(&c) => c,
        Ok(_) => {
            tracing::warn!("WebSocket closed: container not accessible");
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
    
    // Spawn task to handle outgoing messages.
    //
    // The per-event re-validation was removed: it re-checked the *same* immutable
    // token on every event, so it could never observe an expiry it did not already
    // know about, and for a single-use token it always failed and tore the
    // connection down on the first event. Authorization is decided once, at upgrade,
    // and re-checked per connection rather than per event.
    let manager_send = state.manager.clone();
    let internal_id_send = internal_id.clone();
    let principal_send = principal.clone();
    let send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            // Cheap per-event ownership re-check: a container can be deleted or
            // reassigned while the socket is open.
            if !principal_send.is_admin() {
                match manager_send.get_container(&internal_id_send).await {
                    Ok(Some(c)) if principal_send.can_access(&c) => {}
                    _ => {
                        tracing::warn!("Container no longer accessible, closing WebSocket");
                        break;
                    }
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
    
    // Clean up streaming tasks when WebSocket disconnects
    tracing::info!("WebSocket disconnected for container: {}, cleaning up...", internal_id);
    
    // Stop console streaming
    state.console_streamer.stop_streaming(&internal_id).await;
    
    // Stop stats collection
    state.stats_collector.stop_collecting(&internal_id).await;
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
