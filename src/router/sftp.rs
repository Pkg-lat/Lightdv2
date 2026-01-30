//! SFTP API routes

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::container::manager::ContainerManager;
use crate::sftp::credentials::CredentialsManager;

#[derive(Clone)]
pub struct SftpState {
    pub credentials_manager: Arc<CredentialsManager>,
    pub container_manager: Arc<ContainerManager>,
    pub sftp_host: String,
    pub sftp_port: u16,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct CredentialsResponse {
    username: String,
    password: String,
    host: String,
    port: u16,
    volume_path: String,
}

#[derive(Serialize)]
struct SftpInfoResponse {
    username: String,
    host: String,
    port: u16,
    volume_path: String,
    created_at: u64,
    updated_at: u64,
}

#[derive(Deserialize)]
struct GenerateCredentialsRequest {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

pub fn sftp_router(
    credentials_manager: Arc<CredentialsManager>,
    container_manager: Arc<ContainerManager>,
    sftp_host: String,
    sftp_port: u16,
) -> Router {
    let state = SftpState {
        credentials_manager,
        container_manager,
        sftp_host,
        sftp_port,
    };
    
    Router::new()
        .route("/containers/:id/sftp/credentials", post(generate_credentials))
        .route("/containers/:id/sftp/info", get(get_sftp_info))
        .with_state(state)
}

/// Generate or reset SFTP credentials for a container
async fn generate_credentials(
    State(state): State<SftpState>,
    Path(container_id): Path<String>,
    Json(payload): Json<GenerateCredentialsRequest>,
) -> Response {
    // Get container to verify it exists and get volume_id
    let container = match state.container_manager.get_container(&container_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Container not found".to_string(),
                }),
            ).into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to get container: {}", e),
                }),
            ).into_response();
        }
    };
    
    // Generate credentials
    match state.credentials_manager.generate_credentials(
        &container_id,
        &container.volume_id,
        payload.username,
        payload.password,
    ) {
        Ok((username, password)) => {
            tracing::info!("Generated SFTP credentials for container: {}", container_id);
            
            (StatusCode::OK, Json(CredentialsResponse {
                username,
                password,
                host: state.sftp_host.clone(),
                port: state.sftp_port,
                volume_path: format!("/home/container"),
            })).into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to generate credentials: {}", e),
                }),
            ).into_response()
        }
    }
}

/// Get SFTP connection info for a container
async fn get_sftp_info(
    State(state): State<SftpState>,
    Path(container_id): Path<String>,
) -> Response {
    // Get credentials
    match state.credentials_manager.get_credentials(&container_id) {
        Ok(Some(creds)) => {
            (StatusCode::OK, Json(SftpInfoResponse {
                username: creds.username,
                host: state.sftp_host.clone(),
                port: state.sftp_port,
                volume_path: format!("/home/container"),
                created_at: creds.created_at,
                updated_at: creds.updated_at,
            })).into_response()
        }
        Ok(None) => {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "SFTP credentials not found for this container".to_string(),
                }),
            ).into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to get credentials: {}", e),
                }),
            ).into_response()
        }
    }
}
