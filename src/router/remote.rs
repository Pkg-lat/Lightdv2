//! Remote API routes for config management

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
#[allow(unused)]
use serde::{Deserialize, Serialize};
#[allow(unused)]
use std::sync::Arc;

use crate::config::config::Config;

#[derive(Clone)]
pub struct RemoteState {
    // Config lives on disk, not in state, so reloads are picked up per request.
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct SuccessResponse {
    message: String,
}

/// Redacted view of the config for API responses.
///
/// The daemon reads its own config from disk per request, so there is no in-memory
/// copy to serve instead. This type carries only non-secret fields: **never add a
/// token field here.** `authorization.token` authenticates every protected route
/// and `remote.token` is the outbound credential — neither belongs in a response.
#[derive(Serialize)]
struct RedactedConfig {
    version: String,
    server: crate::config::config::ServerConfig,
    authorization: RedactedAuthConfig,
    docker: crate::config::config::DockerConfig,
    storage: crate::config::config::StorageConfig,
    monitoring: crate::config::config::MonitoringConfig,
    remote: Option<RedactedRemoteConfig>,
    sftp: Option<crate::config::config::SftpConfig>,
}

#[derive(Serialize)]
struct RedactedAuthConfig {
    enabled: bool,
    allowed_origins: Vec<String>,
}

#[derive(Serialize)]
struct RedactedRemoteConfig {
    enabled: bool,
    url: String,
}

impl From<Config> for RedactedConfig {
    fn from(config: Config) -> Self {
        Self {
            version: config.version,
            server: config.server,
            authorization: RedactedAuthConfig {
                enabled: config.authorization.enabled,
                allowed_origins: config.authorization.allowed_origins,
            },
            docker: config.docker,
            storage: config.storage,
            monitoring: config.monitoring,
            remote: config.remote.map(|r| RedactedRemoteConfig {
                enabled: r.enabled,
                url: r.url,
            }),
            sftp: config.sftp,
        }
    }
}

pub fn remote_router() -> Router {
    let state = RemoteState {};

    Router::new()
        .route("/remote/config", get(get_config))
        .route("/remote/config/reload", post(reload_config))
        .with_state(state)
}

/// Get current configuration with secrets redacted.
async fn get_config(
    State(_state): State<RemoteState>,
) -> Response {
    match Config::load("config.json") {
        Ok(config) => {
            let redacted = RedactedConfig::from(config);
            match serde_json::to_value(&redacted) {
                Ok(json) => (StatusCode::OK, Json(json)).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Failed to serialize config: {}", e),
                    }),
                ).into_response(),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to load config: {}", e),
            }),
        ).into_response(),
    }
}

/// Reload configuration from file
async fn reload_config(
    State(_state): State<RemoteState>,
) -> Response {
    match Config::load("config.json") {
        Ok(_config) => {
            // TODO: Actually reload the config in the running application
            // This would require passing config through Arc<RwLock<Config>>
            (StatusCode::OK, Json(SuccessResponse {
                message: "Configuration reloaded successfully".to_string(),
            })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to reload config: {}", e),
            }),
        ).into_response(),
    }
}
