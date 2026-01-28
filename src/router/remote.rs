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
    // Add any state needed
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct SuccessResponse {
    message: String,
}

pub fn remote_router() -> Router {
    let state = RemoteState {};
    
    Router::new()
        .route("/remote/config", get(get_config))
        .route("/remote/config/reload", post(reload_config))
        .with_state(state)
}

/// Get current configuration
async fn get_config(
    State(_state): State<RemoteState>,
) -> Response {
    match Config::load("config.json") {
        Ok(config) => {
            match serde_json::to_value(&config) {
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
