//! Authentication middleware for API routes
//! 
//! Validates Bearer tokens and vendor headers.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::sync::Arc;

use crate::config::config::Config;

#[derive(Clone)]
pub struct AuthConfig {
    pub api_token: String,
    pub allowed_origins: Vec<String>,
}

impl AuthConfig {
    pub fn from_config(config: &Config) -> Self {
        Self {
            api_token: config.authorization.token.clone(),
            allowed_origins: vec!["*".to_string()], // TODO: Load from config
        }
    }
}

/// Check if origin is allowed
fn is_origin_allowed(origin: Option<&str>, allowed_origins: &[String]) -> bool {
    match origin {
        Some(o) => allowed_origins.iter().any(|allowed| allowed == "*" || allowed == o),
        None => true, // Allow requests without origin (like from curl)
    }
}

/// Validate vendor header
fn validate_vendor(headers: &HeaderMap) -> bool {
    if let Some(accept) = headers.get("accept") {
        if let Ok(accept_str) = accept.to_str() {
            return accept_str.contains("Application/vnd.pkglat");
        }
    }
    false
}

/// Extract and validate Bearer token
fn validate_bearer_token(headers: &HeaderMap, api_token: &str) -> bool {
    if let Some(auth) = headers.get("authorization") {
        if let Ok(auth_str) = auth.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                // if the token doesn't start with that aswell, we just fuck off.
                return token.starts_with("lightd_") && token == api_token;
            }
        }
    }
    false
}

/// Authentication middleware
pub async fn auth_middleware(
    State(auth_config): State<Arc<AuthConfig>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    // Check origin
    let origin = headers.get("origin").and_then(|h| h.to_str().ok());
    if !is_origin_allowed(origin, &auth_config.allowed_origins) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Origin not allowed"
            }))
        ).into_response();
    }
    
    // Check vendor
    if !validate_vendor(&headers) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({
                "error": "Invalid vendor. Expected: Application/vnd.pkglatv1+json"
            }))
        ).into_response();
    }
    
    // Check Bearer token
    if !validate_bearer_token(&headers, &auth_config.api_token) {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({
                "error": "Invalid or missing Bearer token. Format: Authorization: Bearer lightd_<token>"
            }))
        ).into_response();
    }
    
    next.run(request).await
}
