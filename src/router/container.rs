use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::container::lifecycle::LifecycleManager;
use crate::container::manager::ContainerManager;
use crate::container::power::{PowerManager, PowerAction};
use crate::container::network::NetworkRebinder;
use crate::container::state::{InstallState, PortBinding};

#[derive(Clone)]
pub struct ContainerAppState {
    pub manager: Arc<ContainerManager>,
    pub lifecycle: Arc<LifecycleManager>,
    pub power: Arc<PowerManager>,
    pub network: Arc<NetworkRebinder>,
}

// === Request DTOs ===

#[derive(Deserialize)]
struct CreateContainerRequest {
    internal_id: String,
    volume_id: String,
    startup_command: String,
    image: String,
    install_script: Option<String>,
    /// Pattern to detect when server is fully started (string or regex)
    start_pattern: Option<String>,
}

#[derive(Deserialize)]
struct ReinstallContainerRequest {
    image: String,
    install_script: Option<String>,
}

#[derive(Deserialize)]
struct RepairContainerRequest {
    image: String,
}

#[derive(Deserialize)]
struct UpdateStartupRequest {
    startup_command: String,
}

#[derive(Deserialize)]
struct UpdateStartPatternRequest {
    /// Pattern to detect when server is fully started (string or regex)
    /// Set to null to disable pattern matching
    start_pattern: Option<String>,
}

// === Response DTOs ===

#[derive(Serialize)]
struct CreateContainerResponse {
    internal_id: String,
    message: String,
}

#[derive(Serialize)]
struct ContainerStatusResponse {
    internal_id: String,
    install_state: String,
    is_installing: bool,
    container_id: Option<String>,
    is_healthy: bool,
    corruption_issue: Option<String>,
}

#[derive(Serialize)]
struct ReinstallResponse {
    internal_id: String,
    message: String,
}

#[derive(Serialize)]
struct RepairResponse {
    internal_id: String,
    repaired: bool,
    message: String,
}

#[derive(Serialize)]
struct ValidateResponse {
    internal_id: String,
    is_valid: bool,
    issue: Option<String>,
    docker_synced: bool,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct SuccessResponse {
    message: String,
}

pub fn container_router(
    manager: Arc<ContainerManager>,
    lifecycle: Arc<LifecycleManager>,
    power: Arc<PowerManager>,
    network: Arc<NetworkRebinder>,
) -> Router {
    let state = ContainerAppState { manager, lifecycle, power, network };

    Router::new()
        // Container CRUD
        .route("/containers", post(create_container))
        .route("/containers", get(list_containers))
        .route("/containers/:id", get(get_container))
        .route("/containers/:id", delete(delete_container))
        // Container lifecycle
        .route("/containers/:id/reinstall", post(reinstall_container))
        .route("/containers/:id/repair", post(repair_container))
        .route("/containers/:id/validate", get(validate_container))
        .route("/containers/:id/status", get(get_container_status))
        // Update operations
        .route("/containers/:id/startup", post(update_startup_command))
        .route("/containers/:id/start-pattern", post(update_start_pattern))
        // Power actions
        .route("/containers/:id/start", post(start_container))
        .route("/containers/:id/kill", post(kill_container))
        .route("/containers/:id/restart", post(restart_container))
        // Network operations
        .route("/containers/:id/rebind-network", post(rebind_network))
        .with_state(state)
}

// Container Crud handlers

#[axum::debug_handler]
async fn create_container(
    State(state): State<ContainerAppState>,
    Json(payload): Json<CreateContainerRequest>,
) -> Response {
    // Create container state
    match state
        .manager
        .create_container(
            payload.internal_id.clone(),
            payload.volume_id,
            payload.startup_command,
        )
        .await
    {
        Ok(_) => {
            // Update start_pattern if provided
            if let Some(pattern) = payload.start_pattern {
                if let Ok(Some(mut container)) = state.manager.get_container(&payload.internal_id).await {
                    container.start_pattern = Some(pattern);
                    let _ = state.manager.update_container(container).await;
                }
            }
            
            // Start async installation
            if let Err(e) = state
                .lifecycle
                .install_container(
                    payload.internal_id.clone(),
                    payload.image,
                    payload.install_script,
                )
                .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: e.to_string(),
                    }),
                ).into_response();
            }

            (StatusCode::OK, Json(CreateContainerResponse {
                internal_id: payload.internal_id,
                message: "Container installation started".to_string(),
            })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

async fn list_containers(
    State(state): State<ContainerAppState>,
) -> Response {
    match state.manager.list_containers().await {
        Ok(containers) => (StatusCode::OK, Json(containers)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

async fn get_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
) -> Response {
    match state.manager.get_container(&id).await {
        Ok(Some(container)) => (StatusCode::OK, Json(container)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Container not found".to_string(),
            }),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

async fn delete_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
) -> Response {
    match state.manager.delete_container(&id).await {
        Ok(container) => (StatusCode::OK, Json(container)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}



/// Reinstall a container - removes old Docker container and creates new one
#[axum::debug_handler]
async fn reinstall_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
    Json(payload): Json<ReinstallContainerRequest>,
) -> Response {
    // Check if container exists
    match state.manager.get_container(&id).await {
        Ok(Some(container)) => {
            // Check if already installing
            if container.is_installing {
                return (
                    StatusCode::CONFLICT,
                    Json(ErrorResponse {
                        error: "Container is already being installed/reinstalled".to_string(),
                    }),
                ).into_response();
            }

            // Start reinstall
            match state.lifecycle.reinstall_container(
                id.clone(),
                payload.image,
                payload.install_script,
            ).await {
                Ok(_) => (
                    StatusCode::OK,
                    Json(ReinstallResponse {
                        internal_id: id,
                        message: "Container reinstall started".to_string(),
                    }),
                ).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: e.to_string(),
                    }),
                ).into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Container not found".to_string(),
            }),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

/// Repair a corrupted container - detects issues and reinstalls if needed
#[axum::debug_handler]
async fn repair_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
    Json(payload): Json<RepairContainerRequest>,
) -> Response {
    match state.lifecycle.repair_if_corrupted(id.clone(), payload.image).await {
        Ok(repaired) => {
            let message = if repaired {
                "Container corruption detected and repair started".to_string()
            } else {
                "Container is healthy, no repair needed".to_string()
            };

            (StatusCode::OK, Json(RepairResponse {
                internal_id: id,
                repaired,
                message,
            })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

/// Validate container state and check for corruption
async fn validate_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
) -> Response {
    // First validate DB state
    let (is_valid, issue) = match state.manager.validate_container(&id).await {
        Ok(result) => result,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            ).into_response();
        }
    };

    // Then check Docker sync
    let docker_synced = match state.lifecycle.verify_container_sync(&id).await {
        Ok(synced) => synced,
        Err(_) => false,
    };

    (StatusCode::OK, Json(ValidateResponse {
        internal_id: id,
        is_valid: is_valid && docker_synced,
        issue,
        docker_synced,
    })).into_response()
}

/// Get detailed container status
async fn get_container_status(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
) -> Response {
    match state.manager.get_container(&id).await {
        Ok(Some(container)) => {
            // Validate for corruption
            let (is_healthy, corruption_issue) = match state.manager.validate_container(&id).await {
                Ok((valid, issue)) => (valid, issue),
                Err(_) => (false, Some("Validation error".to_string())),
            };

            let install_state_str = match container.install_state {
                InstallState::Ready => "ready",
                InstallState::Installing => "installing",
                InstallState::Failed => "failed",
            };

            (StatusCode::OK, Json(ContainerStatusResponse {
                internal_id: container.internal_id,
                install_state: install_state_str.to_string(),
                is_installing: container.is_installing,
                container_id: container.container_id,
                is_healthy,
                corruption_issue,
            })).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Container not found".to_string(),
            }),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

// === Update Handlers ===

/// Update container startup command
async fn update_startup_command(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateStartupRequest>,
) -> Response {
    match state.manager.update_startup_command(&id, payload.startup_command).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: "Startup command updated".to_string(),
            }),
        ).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

/// Update container start pattern
async fn update_start_pattern(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateStartPatternRequest>,
) -> Response {
    match state.manager.update_start_pattern(&id, payload.start_pattern).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: "Start pattern updated".to_string(),
            }),
        ).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        ).into_response(),
    }
}

// === Power Action Handlers ===

#[axum::debug_handler]
async fn start_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
) -> Response {
    match state.power.execute_action(id.clone(), PowerAction::Start).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: format!("Container {} start initiated", id),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

#[axum::debug_handler]
async fn kill_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
) -> Response {
    match state.power.execute_action(id.clone(), PowerAction::Kill).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: format!("Container {} kill initiated", id),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

#[axum::debug_handler]
async fn restart_container(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
) -> Response {
    match state.power.execute_action(id.clone(), PowerAction::Restart).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: format!("Container {} restart initiated", id),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}


// === Network Rebinding Handler ===

#[derive(Deserialize)]
struct RebindNetworkRequest {
    ports: Vec<PortBinding>,
    image: String,
}

#[axum::debug_handler]
async fn rebind_network(
    State(state): State<ContainerAppState>,
    Path(id): Path<String>,
    Json(payload): Json<RebindNetworkRequest>,
) -> Response {
    match state.network.rebind_ports(id.clone(), payload.ports, payload.image).await {
        Ok(_) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: format!("Container {} network rebinding initiated", id),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}
