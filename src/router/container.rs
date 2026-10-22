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
use crate::container::state::{ContainerState, InstallState, PortBinding};
use crate::container::update::{ContainerUpdater, ResourceLimits};
use crate::container::validation::{validate_identifier, validate_mounts};
use crate::auth::principal::Principal;
use axum::Extension;
use std::collections::HashMap;

#[derive(Clone)]
pub struct ContainerAppState {
    pub manager: Arc<ContainerManager>,
    pub lifecycle: Arc<LifecycleManager>,
    pub power: Arc<PowerManager>,
    pub network: Arc<NetworkRebinder>,
    pub pool: Arc<crate::network::pool::NetworkPool>,
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
    /// Port requests - user specifies container_port, we assign host_port from pool
    ports: Option<Vec<PortRequest>>,
}

#[derive(Deserialize)]
struct PortRequest {
    pub container_port: u16,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
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

/// Path parameter that is validated before any handler sees it.
///
/// `internal_id` is joined onto the host filesystem as
/// `containers_path.join(internal_id)` and used as a sled key, so an unvalidated
/// value is a path traversal. Using this extractor in place of `Path<String>`
/// means the check runs for every route automatically — and a route added later
/// cannot forget it, which a per-handler call could.
struct ValidatedId(String);

// axum 0.7 declares FromRequestParts with #[async_trait], so the impl must carry
// the same attribute to match the desugared method signature.
#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for ValidatedId
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, Json<ErrorResponse>);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let Path(id) = Path::<String>::from_request_parts(parts, state)
            .await
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Missing container id".to_string(),
                    }),
                )
            })?;

        validate_identifier("container id", &id).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse { error: e }),
            )
        })?;

        Ok(ValidatedId(id))
    }
}

#[derive(Serialize)]
struct SuccessResponse {
    message: String,
}

/// Reject a request unless the caller may act on `id`.
///
/// Returns 404 rather than 403: telling a tenant "this exists but is not yours"
/// still confirms the container id exists, which is information about other
/// tenants. Admins short-circuit, so the common path costs one comparison.
async fn ensure_owns(
    principal: &Principal,
    manager: &ContainerManager,
    id: &str,
) -> Result<(), Response> {
    if principal.is_admin() {
        return Ok(());
    }

    let not_found = || {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Container not found".to_string(),
            }),
        )
            .into_response()
    };

    match manager.get_container(id).await {
        Ok(Some(container)) if principal.can_access(&container) => Ok(()),
        Ok(_) => Err(not_found()),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response()),
    }
}

pub fn container_router(
    manager: Arc<ContainerManager>,
    lifecycle: Arc<LifecycleManager>,
    power: Arc<PowerManager>,
    network: Arc<NetworkRebinder>,
    pool: Arc<crate::network::pool::NetworkPool>,
) -> Router {
    let state = ContainerAppState { manager, lifecycle, power, network, pool };

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
        .route("/containers/:id/resources", post(update_resources))
        .route("/containers/:id/resources", get(get_resources))
        .route("/containers/:id/volumes", post(update_volumes))
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
    Extension(principal): Extension<Principal>,
    Json(payload): Json<CreateContainerRequest>,
) -> Response {
    // Both identifiers are joined onto host paths, so validate before anything is
    // created or written. A 400 here beats a container that escapes the storage root.
    for (kind, value) in [
        ("internal_id", &payload.internal_id),
        ("volume_id", &payload.volume_id),
    ] {
        if let Err(e) = validate_identifier(kind, value) {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response();
        }
    }

    // Create container state
    // The owner is taken from the authenticated principal, never from the request
    // body — otherwise a caller could name a container into another tenant's scope.
    let owner = principal
        .owner_id()
        .map(str::to_string)
        .unwrap_or_else(|| crate::auth::tokens::ADMIN_OWNER.to_string());

    match state
        .manager
        .create_container(
            payload.internal_id.clone(),
            payload.volume_id,
            payload.startup_command,
            owner,
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
            
            // Assign ports from pool if requested
            if let Some(port_requests) = payload.ports {
                let mut assigned_ports = Vec::new();
                
                for request in port_requests {
                    // Claim a port atomically. Reserving and marking separately let
                    // two concurrent creations be handed the same host port.
                    match state.pool.reserve_port_for(&payload.internal_id).await {
                        Ok(Some(network_port)) => {
                            // Create port binding
                            let binding = PortBinding {
                                container_port: request.container_port,
                                host_port: network_port.port,
                                protocol: request.protocol,
                            };

                            assigned_ports.push(binding);
                            tracing::info!("Assigned port {} -> {} for container {}", 
                                request.container_port, network_port.port, payload.internal_id);
                        }
                        Ok(None) => {
                            tracing::error!("No available ports in pool for container {}", payload.internal_id);
                            return (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(ErrorResponse {
                                    error: "No available ports in pool".to_string(),
                                }),
                            ).into_response();
                        }
                        Err(e) => {
                            tracing::error!("Failed to get port from pool: {}", e);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ErrorResponse {
                                    error: format!("Failed to assign ports: {}", e),
                                }),
                            ).into_response();
                        }
                    }
                }
                
                // Update container with assigned ports
                if let Ok(Some(mut container)) = state.manager.get_container(&payload.internal_id).await {
                    container.ports = assigned_ports;
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
    Extension(principal): Extension<Principal>,
) -> Response {
    match state.manager.list_containers().await {
        Ok(mut containers) => {
            // Scope the listing to the caller. This route previously returned every
            // container on the host, including other tenants'.
            if !principal.is_admin() {
                containers.retain(|c| principal.can_access(c));
            }
            hydrate_live_limits(&state, &mut containers).await;
            (StatusCode::OK, Json(containers)).into_response()
        }
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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

    match state.manager.get_container(&id).await {
        Ok(Some(mut container)) => {
            hydrate_single_live_limits(&state, &mut container).await;
            (StatusCode::OK, Json(container)).into_response()
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

fn apply_live_limits(container: &mut ContainerState, limits: &ResourceLimits) {
    container.limits.memory = limits.memory;
    container.limits.memory_swap = limits.memory_swap;
    container.limits.memory_reservation = limits.memory_reservation;
    container.limits.cpu_shares = limits.cpu_shares;
    container.limits.cpu_period = limits.cpu_period;
    container.limits.cpu_quota = limits.cpu_quota;
    container.limits.cpuset_cpus = limits.cpuset_cpus.clone();
    container.limits.blkio_weight = limits.blkio_weight;
    container.limits.cpu = match (limits.cpu_quota, limits.cpu_period) {
        (Some(quota), Some(period)) if quota > 0 && period > 0 => Some(quota as f64 / period as f64),
        _ => limits.cpu_shares.map(|shares| shares as f64 / 1024.0),
    };
}

async fn hydrate_single_live_limits(state: &ContainerAppState, container: &mut ContainerState) {
    if container.container_id.is_none() {
        return;
    }

    let updater = match ContainerUpdater::new(state.manager.clone()) {
        Ok((updater, _rx)) => updater,
        Err(e) => {
            tracing::warn!(
                container_id = %container.internal_id,
                error = %e,
                "Could not create updater for live limits hydration"
            );
            return;
        }
    };

    match updater.get_current_resources(&container.internal_id).await {
        Ok(limits) => {
            apply_live_limits(container, &limits);
            tracing::debug!(
                container_id = %container.internal_id,
                cpu_quota = ?limits.cpu_quota,
                cpu_period = ?limits.cpu_period,
                memory = ?limits.memory,
                "Hydrated live limits from Docker"
            );
        }
        Err(e) => {
            tracing::warn!(
                container_id = %container.internal_id,
                error = %e,
                "Failed to hydrate live limits from Docker; returning stored state"
            );
        }
    }
}

async fn hydrate_live_limits(state: &ContainerAppState, containers: &mut [ContainerState]) {
    let updater = match ContainerUpdater::new(state.manager.clone()) {
        Ok((updater, _rx)) => updater,
        Err(e) => {
            tracing::warn!(error = %e, "Could not create updater for live limits hydration");
            return;
        }
    };

    for container in containers.iter_mut() {
        if container.container_id.is_none() {
            continue;
        }

        match updater.get_current_resources(&container.internal_id).await {
            Ok(limits) => apply_live_limits(container, &limits),
            Err(e) => tracing::warn!(
                container_id = %container.internal_id,
                error = %e,
                "Failed to hydrate live limits from Docker for container"
            ),
        }
    }
}

async fn delete_container(
    State(state): State<ContainerAppState>,
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

    // Return this container's ports to the pool.
    //
    // Previously this scanned every pool entry and released the first whose port
    // number matched a binding, so when two containers shared a port it released
    // whichever it found first — possibly the other container's, or its own while
    // leaving the other claimed. Ports are now released by the container that
    // holds them.
    match state.pool.return_ports_for(&id).await {
        Ok(released) => {
            for port in released {
                tracing::info!("Returned port {} to pool on container delete", port.port);
            }
        }
        Err(e) => tracing::error!("Failed to return ports for {}: {}", id, e),
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
    Json(payload): Json<ReinstallContainerRequest>,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
    Json(payload): Json<RepairContainerRequest>,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
    Json(payload): Json<UpdateStartupRequest>,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

    let startup_command = payload.startup_command;
    tracing::info!(
        container_id = %id,
        command_length = startup_command.len(),
        "Startup command update requested"
    );

    match state.manager.update_startup_command(&id, startup_command).await {
        Ok(_) => (
            {
                tracing::info!(container_id = %id, "Startup command updated");
                (
                    StatusCode::OK,
                    Json(SuccessResponse {
                        message: "Startup command updated".to_string(),
                    }),
                )
            }
        ).into_response(),
        Err(e) => {
            tracing::error!(container_id = %id, error = %e, "Startup command update failed");
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        }.into_response(),
    }
}

/// Update container start pattern
async fn update_start_pattern(
    State(state): State<ContainerAppState>,
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
    Json(payload): Json<UpdateStartPatternRequest>,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

    let start_pattern = payload.start_pattern;
    tracing::info!(
        container_id = %id,
        has_pattern = start_pattern.is_some(),
        "Start pattern update requested"
    );

    match state.manager.update_start_pattern(&id, start_pattern).await {
        Ok(_) => (
            {
                tracing::info!(container_id = %id, "Start pattern updated");
                (
                    StatusCode::OK,
                    Json(SuccessResponse {
                        message: "Start pattern updated".to_string(),
                    }),
                )
            }
        ).into_response(),
        Err(e) => {
            tracing::error!(container_id = %id, error = %e, "Start pattern update failed");
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        }.into_response(),
    }
}

// === Power Action Handlers ===

#[axum::debug_handler]
async fn start_container(
    State(state): State<ContainerAppState>,
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
    Json(payload): Json<RebindNetworkRequest>,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

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

// === Container Update Handlers ===

#[derive(Deserialize)]
struct UpdateResourcesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<i64>, // Memory in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_swap: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_reservation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_shares: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_period: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_quota: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cpuset_cpus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blkio_weight: Option<u16>,
}

#[derive(Deserialize)]
struct UpdateVolumesRequest {
    volumes: HashMap<String, String>,
}

#[derive(Serialize)]
struct ResourcesResponse {
    memory: Option<i64>,
    memory_swap: Option<i64>,
    memory_reservation: Option<i64>,
    cpu_shares: Option<i64>,
    cpu_period: Option<i64>,
    cpu_quota: Option<i64>,
    cpuset_cpus: Option<String>,
    blkio_weight: Option<u16>,
}

/// Update container resource limits (live, no restart)
async fn update_resources(
    State(state): State<ContainerAppState>,
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
    Json(payload): Json<UpdateResourcesRequest>,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

    // Create updater
    let updater = match ContainerUpdater::new(state.manager.clone()) {
        Ok((updater, _rx)) => updater,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to create updater: {}", e),
                }),
            )
                .into_response();
        }
    };

    let limits = ResourceLimits {
        memory: payload.memory,
        memory_swap: payload.memory_swap,
        memory_reservation: payload.memory_reservation,
        cpu_shares: payload.cpu_shares,
        cpu_period: payload.cpu_period,
        cpu_quota: payload.cpu_quota,
        cpuset_cpus: payload.cpuset_cpus,
        blkio_weight: payload.blkio_weight,
    };

    tracing::info!(
        container_id = %id,
        memory = ?limits.memory,
        memory_swap = ?limits.memory_swap,
        memory_reservation = ?limits.memory_reservation,
        cpu_shares = ?limits.cpu_shares,
        cpu_period = ?limits.cpu_period,
        cpu_quota = ?limits.cpu_quota,
        cpuset_cpus = ?limits.cpuset_cpus,
        blkio_weight = ?limits.blkio_weight,
        "Resource limits update requested"
    );

    match updater.update_resources(id.clone(), limits).await {
        Ok(()) => {
            tracing::info!(container_id = %id, "Resource limits update accepted");
            (
                StatusCode::ACCEPTED,
                Json(SuccessResponse {
                    message: "Resource update started".to_string(),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(container_id = %id, error = %e, "Resource limits update failed");
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}

/// Get current resource limits
async fn get_resources(
    State(state): State<ContainerAppState>,
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

    tracing::debug!(container_id = %id, "Resource limits lookup requested");

    let updater = match ContainerUpdater::new(state.manager.clone()) {
        Ok((updater, _rx)) => updater,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to create updater: {}", e),
                }),
            )
                .into_response();
        }
    };

    match updater.get_current_resources(&id).await {
        Ok(limits) => {
            tracing::debug!(
                container_id = %id,
                memory = ?limits.memory,
                memory_swap = ?limits.memory_swap,
                memory_reservation = ?limits.memory_reservation,
                cpu_shares = ?limits.cpu_shares,
                cpu_period = ?limits.cpu_period,
                cpu_quota = ?limits.cpu_quota,
                cpuset_cpus = ?limits.cpuset_cpus,
                blkio_weight = ?limits.blkio_weight,
                "Resource limits fetched"
            );

            (
                StatusCode::OK,
                Json(ResourcesResponse {
                    memory: limits.memory,
                    memory_swap: limits.memory_swap,
                    memory_reservation: limits.memory_reservation,
                    cpu_shares: limits.cpu_shares,
                    cpu_period: limits.cpu_period,
                    cpu_quota: limits.cpu_quota,
                    cpuset_cpus: limits.cpuset_cpus,
                    blkio_weight: limits.blkio_weight,
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(container_id = %id, error = %e, "Resource limits lookup failed");
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}

/// Update container volumes (requires restart)
async fn update_volumes(
    State(state): State<ContainerAppState>,
    Extension(principal): Extension<Principal>,
    ValidatedId(id): ValidatedId,
    Json(payload): Json<UpdateVolumesRequest>,
) -> Response {
    // Ownership gate: 404 rather than 403 so a tenant cannot probe
    // which container ids exist on the host.
    if let Err(response) = ensure_owns(&principal, &state.manager, &id).await {
        return response;
    }

    let volumes = payload.volumes;
    let volume_targets: Vec<&str> = volumes.keys().map(String::as_str).collect();
    tracing::info!(
        container_id = %id,
        volume_count = volumes.len(),
        targets = ?volume_targets,
        "Volume update requested"
    );

    let updater = match ContainerUpdater::new(state.manager.clone()) {
        Ok((updater, _rx)) => updater,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to create updater: {}", e),
                }),
            )
                .into_response();
        }
    };

    match updater.update_volumes(id.clone(), volumes).await {
        Ok(()) => {
            tracing::info!(container_id = %id, "Volume update accepted");
            (
                StatusCode::ACCEPTED,
                Json(SuccessResponse {
                    message: "Volumes updated (restart container to apply changes)".to_string(),
                }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(container_id = %id, error = %e, "Volume update failed");
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}
