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
        .with_state(state)
}

async fn create_volume(
    State(state): State<AppState>,
) -> Result<Json<VolumeResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.volume_handler.create_volume().await {
        Ok(volume) => Ok(Json(VolumeResponse {
            id: volume.id,
            path: volume.path.to_string_lossy().to_string(),
            created_at: volume.created_at,
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
