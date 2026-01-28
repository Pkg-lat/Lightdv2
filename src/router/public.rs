//! Public routes that don't require authentication

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

#[derive(Serialize)]
struct PingResponse {
    status: String,
    version: String,
}

pub fn public_router() -> Router {
    Router::new()
        .route("/api/v1/public/ping", get(ping))
}

async fn ping() -> Response {
    (StatusCode::OK, Json(PingResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })).into_response()
}
