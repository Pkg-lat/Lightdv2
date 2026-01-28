use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::filesystem::handler::VolumeHandler;
//use crate::filesystem::volume::Volume;

#[derive(Clone)]
pub struct AppState {
    pub volume_handler: Arc<VolumeHandler>,
}

#[derive(Serialize)]
struct VolumeResponse {
    id: String,
    path: String,
    created_at: u64,
    quota_mb: Option<u64>,
}

#[derive(Serialize)]
struct FilesResponse {
    files: Vec<String>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
struct WriteFileRequest {
    filename: String,
    content: Option<String>,
}

#[derive(Serialize)]
struct WriteFileResponse {
    success: bool,
    path: String,
}

#[derive(Deserialize)]
struct CreateFolderRequest {
    root: String,
    name: String,
}

#[derive(Serialize)]
struct CreateFolderResponse {
    success: bool,
    path: String,
}

#[derive(Deserialize)]
struct CopyRequest {
    source: String,
    destination: String,
    is_folder: bool,
}

#[derive(Serialize)]
struct CopyResponse {
    success: bool,
    path: String,
}

#[derive(Deserialize)]
struct DecompressRequest {
    root: String,
    file: String,
}

#[derive(Serialize)]
struct DecompressResponse {
    success: bool,
    path: String,
}

#[derive(Deserialize)]
struct CompressRequest {
    sources: Vec<String>,
    output: String,
    format: String,
}

#[derive(Serialize)]
struct CompressResponse {
    success: bool,
    path: String,
}

#[derive(Deserialize)]
struct CreateVolumeRequest {
    size: Option<u64>, // Size in MB
}

#[derive(Serialize)]
struct QuotaResponse {
    size_mb: u64,
    used_mb: u64,
    available_mb: u64,
    percentage_used: f64,
}

#[derive(Deserialize)]
struct ResizeVolumeRequest {
    size: u64, // New size in MB
}

pub fn volume_router(volume_handler: Arc<VolumeHandler>) -> Router {
    let state = AppState { volume_handler };

    Router::new()
        .route("/volumes", post(create_volume))
        .route("/volumes", get(list_volumes))
        .route("/volumes/:id", delete(delete_volume))
        .route("/volumes/:id/files", get(list_files))
        .route("/volumes/:id/write", post(write_file))
        .route("/volumes/:id/create-folder", post(create_folder))
        .route("/volumes/:id/copy", post(copy_file_or_folder))
        .route("/volumes/:id/decompress", post(decompress_archive))
        .route("/volumes/:id/compress", post(compress_files))
        .route("/volumes/:id/quota", get(get_volume_quota))
        .route("/volumes/:id/resize", post(resize_volume))
        .with_state(state)
}

async fn create_volume(
    State(state): State<AppState>,
    Json(payload): Json<Option<CreateVolumeRequest>>,
) -> Result<Json<VolumeResponse>, (StatusCode, Json<ErrorResponse>)> {
    let size_mb = payload.and_then(|p| p.size);
    
    let result = if size_mb.is_some() {
        state.volume_handler.create_volume_with_quota(size_mb).await
    } else {
        state.volume_handler.create_volume().await
    };
    
    match result {
        Ok(volume) => Ok(Json(VolumeResponse {
            id: volume.id,
            path: volume.path.to_string_lossy().to_string(),
            created_at: volume.created_at,
            quota_mb: volume.quota_mb,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn list_volumes(
    State(state): State<AppState>,
) -> Json<Vec<VolumeResponse>> {
    let volumes = state.volume_handler.list_volumes().await;
    let response: Vec<VolumeResponse> = volumes
        .into_iter()
        .map(|v| VolumeResponse {
            id: v.id,
            path: v.path.to_string_lossy().to_string(),
            created_at: v.created_at,
            quota_mb: v.quota_mb,
        })
        .collect();

    Json(response)
}

async fn delete_volume(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.delete_volume(&id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn list_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<FilesResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.list_volume_files(&id).await {
        Ok(files) => Ok(Json(FilesResponse { files })),
        Err(e) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn write_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<WriteFileRequest>,
) -> Result<Json<WriteFileResponse>, (StatusCode, Json<ErrorResponse>)> {
    let content = payload.content.unwrap_or_default();
    
    match state.volume_handler.write_file(&id, &payload.filename, &content).await {
        Ok(path) => Ok(Json(WriteFileResponse {
            success: true,
            path: path.to_string_lossy().to_string(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn create_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<CreateFolderRequest>,
) -> Result<Json<CreateFolderResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.create_folder(&id, &payload.root, &payload.name).await {
        Ok(path) => Ok(Json(CreateFolderResponse {
            success: true,
            path: path.to_string_lossy().to_string(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn copy_file_or_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<CopyRequest>,
) -> Result<Json<CopyResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.copy(&id, &payload.source, &payload.destination, payload.is_folder).await {
        Ok(path) => Ok(Json(CopyResponse {
            success: true,
            path: path.to_string_lossy().to_string(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn decompress_archive(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<DecompressRequest>,
) -> Result<Json<DecompressResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.decompress(&id, &payload.root, &payload.file).await {
        Ok(path) => Ok(Json(DecompressResponse {
            success: true,
            path: path.to_string_lossy().to_string(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn compress_files(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<CompressRequest>,
) -> Result<Json<CompressResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.compress(&id, payload.sources, &payload.output, &payload.format).await {
        Ok(path) => Ok(Json(CompressResponse {
            success: true,
            path: path.to_string_lossy().to_string(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn get_volume_quota(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<QuotaResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.get_volume_quota(&id).await {
        Ok(quota) => {
            let percentage_used = if quota.size_mb > 0 {
                (quota.used_mb as f64 / quota.size_mb as f64) * 100.0
            } else {
                0.0
            };
            
            Ok(Json(QuotaResponse {
                size_mb: quota.size_mb,
                used_mb: quota.used_mb,
                available_mb: quota.available_mb,
                percentage_used,
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn resize_volume(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ResizeVolumeRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.resize_volume(&id, payload.size).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}
