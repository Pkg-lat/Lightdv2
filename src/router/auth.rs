//! Authentication routes for token generation

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::tokens::TokenManager;

#[derive(Clone)]
pub struct AuthState {
    pub token_manager: Arc<TokenManager>,
}

#[derive(Deserialize)]
struct GenerateTokenRequest {
    /// Time to live in seconds (e.g., "15m" = 900 seconds)
    ttl: String,
    /// Remove token after first use
    #[serde(default)]
    remove_on_use: bool,
}

#[derive(Serialize)]
struct GenerateTokenResponse {
    token: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

pub fn auth_router(token_manager: Arc<TokenManager>) -> Router {
    let state = AuthState { token_manager };
    
    Router::new()
        .route("/auth/tokens", post(generate_token))
        .with_state(state)
}

/// Parse TTL string (e.g., "15m", "1h", "30s")
fn parse_ttl(ttl: &str) -> Result<u64, String> {
    let ttl = ttl.trim();
    
    if ttl.ends_with('m') {
        let minutes: u64 = ttl[..ttl.len()-1].parse()
            .map_err(|_| "Invalid TTL format")?;
        Ok(minutes * 60)
    } else if ttl.ends_with('h') {
        let hours: u64 = ttl[..ttl.len()-1].parse()
            .map_err(|_| "Invalid TTL format")?;
        Ok(hours * 3600)
    } else if ttl.ends_with('s') {
        ttl[..ttl.len()-1].parse()
            .map_err(|_| "Invalid TTL format".to_string())
    } else {
        // Assume seconds if no suffix
        ttl.parse().map_err(|_| "Invalid TTL format".to_string())
    }
}

async fn generate_token(
    State(state): State<AuthState>,
    Json(payload): Json<GenerateTokenRequest>,
) -> Response {
    // Parse TTL
    let ttl_seconds = match parse_ttl(&payload.ttl) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse { error: e }),
            ).into_response();
        }
    };
    
    // Generate token
    match state.token_manager.generate_token(ttl_seconds, payload.remove_on_use) {
        Ok(token) => {
            (StatusCode::OK, Json(GenerateTokenResponse { token })).into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            ).into_response()
        }
    }
}
