//! Authentication middleware for API routes
//!
//! Validates the Bearer token and vendor header, then injects a `Principal`
//! describing who the caller is so handlers can authorize against it.

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::sync::Arc;

use crate::auth::principal::{secret_eq, Principal};
use crate::auth::tokens::TokenManager;
use crate::config::config::Config;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failed authentication attempts allowed per IP per window before rejecting.
const AUTH_FAILURES_PER_WINDOW: u32 = 20;

/// Length of the failure window.
const AUTH_WINDOW: Duration = Duration::from_secs(60);

/// Failed attempts per client IP within the current window.
///
/// Only failures are counted, so a well-behaved client is never throttled. The
/// intent is to make online guessing of a 32-hex master token impractical, which
/// it otherwise is not: bcrypt cannot help here because the token is compared
/// directly, so each request costs the attacker almost nothing.
static AUTH_FAILURES: std::sync::LazyLock<Mutex<std::collections::HashMap<IpAddr, (u32, Instant)>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Record a failed attempt; returns true when the caller is over the limit.
fn record_failure(ip: Option<IpAddr>) -> bool {
    let Some(ip) = ip else {
        // No peer address (e.g. a unix socket): cannot attribute, so do not throttle.
        return false;
    };

    let mut failures = match AUTH_FAILURES.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let now = Instant::now();
    let entry = failures.entry(ip).or_insert((0, now));

    if now.duration_since(entry.1) > AUTH_WINDOW {
        *entry = (0, now);
    }

    entry.0 = entry.0.saturating_add(1);

    // Opportunistic sweep so the map cannot grow without bound under a spray
    // across many source addresses.
    if failures.len() > 10_000 {
        failures.retain(|_, (_, started)| now.duration_since(*started) <= AUTH_WINDOW);
    }

    entry.0 > AUTH_FAILURES_PER_WINDOW
}

/// Clear the failure count after a successful authentication.
fn clear_failures(ip: Option<IpAddr>) {
    let Some(ip) = ip else { return };
    if let Ok(mut failures) = AUTH_FAILURES.lock() {
        failures.remove(&ip);
    }
}

#[derive(Clone)]
pub struct AuthConfig {
    pub api_token: String,
    pub allowed_origins: Vec<String>,
    pub token_manager: Arc<TokenManager>,
}

impl AuthConfig {
    pub fn from_config(config: &Config, token_manager: Arc<TokenManager>) -> Self {
        Self {
            api_token: config.authorization.token.clone(),
            allowed_origins: config.authorization.allowed_origins.clone(),
            token_manager,
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
            return accept_str.contains("Application/vnd.pkglatv1+json");
        }
    }
    false
}

/// Resolve the caller's identity from the Authorization header.
///
/// The master token from config (or the `LIGHTD_API_TOKEN` environment variable)
/// acts as admin. A token minted through `/auth/tokens` acts as the tenant it was
/// issued for. Returns `None` for anything unrecognized.
fn resolve_principal(headers: &HeaderMap, auth_config: &AuthConfig) -> Option<Principal> {
    let auth = headers.get("authorization")?;
    let auth_str = auth.to_str().ok()?;
    let token = auth_str.strip_prefix("Bearer ")?;

    // Master token => admin. Constant-time compare for the config token.
    //
    // The env var is checked too: with config.json no longer committed, deployments
    // need a way to supply the token without a file on disk.
    let config_token = std::env::var("LIGHTD_API_TOKEN").unwrap_or_else(|_| auth_config.api_token.clone());
    if !config_token.is_empty() && secret_eq(token, &config_token) {
        return Some(Principal::admin());
    }

    // Minted token => tenant (or admin, when minted with the system owner).
    match auth_config.token_manager.validate_token(token, false) {
        Ok(Some(data)) => {
            if data.is_admin() {
                Some(Principal::admin())
            } else {
                Some(Principal::tenant(data.owner))
            }
        }
        Ok(None) => None,
        Err(e) => {
            // Never echo the token here — a malformed input drives this path.
            tracing::error!("Token validation failed: {}", e);
            None
        }
    }
}

/// Authentication middleware
pub async fn auth_middleware(
    State(auth_config): State<Arc<AuthConfig>>,
    headers: HeaderMap,
    mut request: Request,
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

    // Peer address, when the server is serving with connection info available.
    let peer_ip = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip());

    // Resolve identity. The failure response deliberately does not repeat the
    // expected token format or value.
    let principal = match resolve_principal(&headers, &auth_config) {
        Some(principal) => {
            clear_failures(peer_ip);
            principal
        }
        None => {
            if record_failure(peer_ip) {
                tracing::warn!(?peer_ip, "Auth failure rate limit exceeded");
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(json!({
                        "error": "Too many failed authentication attempts"
                    }))
                ).into_response();
            }

            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({
                    "error": "Invalid or missing credentials"
                }))
            ).into_response();
        }
    };

    // Hand the identity to the handler.
    request.extensions_mut().insert(principal);

    next.run(request).await
}

/// Inject an admin `Principal` into every request, without authenticating.
///
/// This exists solely for routers that validate their own credentials and cannot
/// run `auth_middleware` — the WebSocket handler, which authenticates from a query
/// parameter during the upgrade.
///
/// The point is that *every* router ends up holding a `Principal` extension, so
/// the extractor fails at compile time when one is missing rather than silently
/// granting access. Authenticate separately; do not treat reaching a handler as
/// proof of identity.
pub async fn admin_principal_middleware(mut request: Request, next: Next) -> Response {
    request.extensions_mut().insert(Principal::admin());
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::principal::Role;
    use axum::http::header::AUTHORIZATION;

    fn test_auth_config() -> AuthConfig {
        let dir = std::env::temp_dir().join(format!("lightd_auth_test_{}", uuid::Uuid::new_v4()));
        let manager = Arc::new(TokenManager::new(&dir.join("tokens.db").to_string_lossy()).unwrap());
        AuthConfig {
            api_token: "lightd_master_token_value".to_string(),
            allowed_origins: Vec::new(),
            token_manager: manager,
        }
    }

    fn headers_with(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, format!("Bearer {}", token).parse().unwrap());
        headers
    }

    #[test]
    fn master_token_is_admin() {
        let config = test_auth_config();
        let principal = resolve_principal(&headers_with("lightd_master_token_value"), &config).unwrap();
        assert_eq!(principal.role, Role::Admin);
    }

    #[test]
    fn minted_token_is_scoped_to_its_owner() {
        let config = test_auth_config();
        let token = config
            .token_manager
            .generate_token_for("alice".to_string(), 3600, false)
            .unwrap();

        let principal = resolve_principal(&headers_with(&token), &config).unwrap();
        assert_eq!(principal.role, Role::Tenant("alice".to_string()));
        assert!(!principal.is_admin());
    }

    #[test]
    fn unknown_token_is_rejected() {
        let config = test_auth_config();
        assert!(resolve_principal(&headers_with("lightd_nope"), &config).is_none());
        assert!(resolve_principal(&headers_with("not-even-prefixed"), &config).is_none());
    }

    #[test]
    fn missing_authorization_header_is_rejected() {
        let config = test_auth_config();
        assert!(resolve_principal(&HeaderMap::new(), &config).is_none());
    }
}