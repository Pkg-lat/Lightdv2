//! Authentication routes for token generation

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::principal::Principal;
use crate::auth::tokens::{TokenManager, ADMIN_OWNER};

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
    /// Owner the token acts as. Omit to mint an admin (system) token.
    ///
    /// Only an admin may mint, and the owner is recorded on the container at
    /// creation, so a token can never be widened to reach another tenant's
    /// containers after the fact.
    #[serde(default)]
    owner: Option<String>,
}

#[derive(Serialize)]
struct GenerateTokenResponse {
    token: String,
    owner: String,
    expires_in_seconds: u64,
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

    if ttl.is_empty() {
        return Err("Invalid TTL format".to_string());
    }

    // `checked_mul` rather than a bare `*`: "99999999999999999h" overflows u64 and
    // in a release build wraps to a tiny value, silently minting a token that
    // expires almost immediately (or, on some inputs, far in the future).
    let (value, multiplier) = if ttl.ends_with('m') {
        (&ttl[..ttl.len() - 1], 60u64)
    } else if ttl.ends_with('h') {
        (&ttl[..ttl.len() - 1], 3600u64)
    } else if ttl.ends_with('s') {
        (&ttl[..ttl.len() - 1], 1u64)
    } else {
        (ttl, 1u64)
    };

    let value: u64 = value.trim().parse().map_err(|_| "Invalid TTL format")?;
    value
        .checked_mul(multiplier)
        .ok_or_else(|| "TTL is too large".to_string())
}

async fn generate_token(
    State(state): State<AuthState>,
    Extension(principal): Extension<Principal>,
    Json(payload): Json<GenerateTokenRequest>,
) -> Response {
    // Minting is an admin operation. A tenant token must not be able to mint
    // another token with a different owner, which would be a privilege escalation.
    if !principal.is_admin() {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Only an admin token may mint tokens".to_string(),
            }),
        ).into_response();
    }

    // Parse TTL
    let ttl_seconds = match parse_ttl(&payload.ttl) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse { error: e.to_string() }),
            ).into_response();
        }
    };

    if ttl_seconds == 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "TTL must be greater than zero".to_string(),
            }),
        ).into_response();
    }

    // Validate the owner for the same reason identifiers are validated elsewhere:
    // it is compared against container owners and appears in logs.
    let owner = match payload.owner {
        Some(ref o) if !o.is_empty() => {
            if let Err(e) = crate::container::validation::validate_identifier("owner", o) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse { error: e }),
                ).into_response();
            }
            o.clone()
        }
        _ => ADMIN_OWNER.to_string(),
    };

    // Generate token
    match state.token_manager.generate_token_for(owner.clone(), ttl_seconds, payload.remove_on_use) {
        Ok(token) => {
            (StatusCode::OK, Json(GenerateTokenResponse {
                token,
                owner,
                expires_in_seconds: ttl_seconds.min(crate::auth::tokens::MAX_TTL_SECONDS),
            })).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ttl_suffixes() {
        assert_eq!(parse_ttl("30s").unwrap(), 30);
        assert_eq!(parse_ttl("15m").unwrap(), 900);
        assert_eq!(parse_ttl("1h").unwrap(), 3600);
        assert_eq!(parse_ttl("45").unwrap(), 45);
        assert_eq!(parse_ttl(" 10m ").unwrap(), 600);
    }

    #[test]
    fn rejects_bad_ttl() {
        assert!(parse_ttl("").is_err());
        assert!(parse_ttl("abc").is_err());
        assert!(parse_ttl("10x").is_err());
        assert!(parse_ttl("-5").is_err());
    }

    #[test]
    fn rejects_overflowing_ttl() {
        // Must not wrap around to a small or negative expiry.
        assert!(parse_ttl("99999999999999999h").is_err());
        assert!(parse_ttl("18446744073709551615m").is_err());
    }
}