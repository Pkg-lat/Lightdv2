use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::network::pool::{NetworkPool, NetworkPort};

#[derive(Clone)]
pub struct NetworkState {
    pub pool: Arc<NetworkPool>,
}

#[derive(Deserialize)]
struct AddPortRequest {
    ip: String,
    port: u16,
    protocol: Option<String>, // Optional, defaults to "tcp"
}

#[derive(Deserialize)]
struct MarkInUseRequest {
    in_use: bool,
}

#[derive(Deserialize)]
struct BulkDeleteRequest {
    ids: Vec<String>,
}

#[derive(Deserialize)]
struct BulkAddRequest {
    ports: Vec<BulkAddPort>,
}

#[derive(Deserialize)]
struct BulkAddPort {
    ip: String,
    port: u16,
    #[serde(default = "default_protocol")]
    protocol: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct BulkDeleteResponse {
    deleted: Vec<String>,
}

#[derive(Serialize)]
struct BulkAddResponse {
    added: Vec<NetworkPort>,
    count: usize,
}

pub fn network_router(pool: Arc<NetworkPool>) -> Router {
    let state = NetworkState { pool };

    Router::new()
        .route("/network/ports", post(add_port))
        .route("/network/ports", get(get_all_ports))
        .route("/network/ports/bulk", post(bulk_add))
        .route("/network/ports/available", get(get_available_ports))
        .route("/network/ports/random", get(get_random_port))
        .route("/network/ports/:id", get(get_port))
        .route("/network/ports/:id", delete(delete_port))
        .route("/network/ports/:id/use", post(mark_in_use))
        .route("/network/ports/bulk-delete", post(bulk_delete))
        .with_state(state)
}

async fn add_port(
    State(state): State<NetworkState>,
    Json(payload): Json<AddPortRequest>,
) -> Result<Json<NetworkPort>, (StatusCode, Json<ErrorResponse>)> {
    match state.pool.add_port(payload.ip, payload.port, payload.protocol).await {
        Ok(port) => Ok(Json(port)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn get_all_ports(
    State(state): State<NetworkState>,
) -> Result<Json<Vec<NetworkPort>>, (StatusCode, Json<ErrorResponse>)> {
    match state.pool.get_all_ports().await {
        Ok(ports) => Ok(Json(ports)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn get_port(
    State(state): State<NetworkState>,
    Path(id): Path<String>,
) -> Result<Json<NetworkPort>, (StatusCode, Json<ErrorResponse>)> {
    match state.pool.get_port(&id).await {
        Ok(Some(port)) => Ok(Json(port)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Port not found".to_string(),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn get_random_port(
    State(state): State<NetworkState>,
) -> Result<Json<NetworkPort>, (StatusCode, Json<ErrorResponse>)> {
    match state.pool.get_random_available().await {
        Ok(Some(port)) => Ok(Json(port)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "No available ports".to_string(),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}



async fn mark_in_use(
    State(state): State<NetworkState>,
    Path(id): Path<String>,
    Json(payload): Json<MarkInUseRequest>,
) -> Result<Json<NetworkPort>, (StatusCode, Json<ErrorResponse>)> {
    match state.pool.mark_in_use(&id, payload.in_use).await {
        Ok(port) => Ok(Json(port)),
        Err(e) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

#[axum::debug_handler]
async fn delete_port(
    State(state): State<NetworkState>,
    Path(id): Path<String>,
) -> Result<Json<NetworkPort>, (StatusCode, Json<ErrorResponse>)> {
    state.pool.delete_port(&id).await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })
}

#[axum::debug_handler]
async fn bulk_delete(
    State(state): State<NetworkState>,
    Json(payload): Json<BulkDeleteRequest>,
) -> Result<Json<BulkDeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    state.pool.bulk_delete(payload.ids).await
        .map(|deleted| Json(BulkDeleteResponse { deleted }))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })
}

#[axum::debug_handler]
async fn bulk_add(
    State(state): State<NetworkState>,
    Json(payload): Json<BulkAddRequest>,
) -> Result<Json<BulkAddResponse>, (StatusCode, Json<ErrorResponse>)> {
    if payload.ports.len() > 50 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Maximum 50 ports allowed per bulk add".to_string(),
            }),
        ));
    }

    let ports_to_add: Vec<(String, u16, String)> = payload.ports
        .into_iter()
        .map(|p| (p.ip, p.port, p.protocol))
        .collect();

    match state.pool.bulk_add(ports_to_add).await {
        Ok(added) => {
            let count = added.len();
            Ok(Json(BulkAddResponse { added, count }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn get_available_ports(
    State(state): State<NetworkState>,
) -> Result<Json<Vec<NetworkPort>>, (StatusCode, Json<ErrorResponse>)> {
    match state.pool.get_available_ports().await {
        Ok(ports) => Ok(Json(ports)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}