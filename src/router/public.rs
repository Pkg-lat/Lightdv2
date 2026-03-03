//! Public routes that don't require authentication

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use sysinfo::System;

#[derive(Serialize)]
struct PingResponse {
    status: String,
    version: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    uptime_seconds: u64,
    system: SystemInfo,
    docker: DockerInfo,
}

#[derive(Serialize)]
struct SystemInfo {
    hostname: String,
    os: String,
    kernel_version: String,
    cpu_count: usize,
    cpu_usage_percent: f32,
    memory_total_bytes: u64,
    memory_used_bytes: u64,
    memory_available_bytes: u64,
    memory_usage_percent: f32,
    disk_total_bytes: u64,
    disk_used_bytes: u64,
    disk_available_bytes: u64,
    disk_usage_percent: f32,
}

#[derive(Serialize)]
struct DockerInfo {
    available: bool,
    version: Option<String>,
}

pub fn public_router() -> Router {
    Router::new()
        .route("/api/v1/public/ping", get(ping))
        .route("/api/v1/public/health", get(health))
}

async fn ping() -> Response {
    (StatusCode::OK, Json(PingResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })).into_response()
}

async fn health() -> Response {
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Get system information
    let mut sys = System::new_all();
    sys.refresh_all();
    
    // Calculate CPU usage
    let cpu_usage: f32 = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / sys.cpus().len() as f32;
    
    // Get memory info
    let memory_total = sys.total_memory();
    let memory_used = sys.used_memory();
    let memory_available = sys.available_memory();
    let memory_usage_percent = (memory_used as f32 / memory_total as f32) * 100.0;
    
    // Get disk info (root disk)
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let (disk_total, disk_used, disk_available) = disks.iter()
        .find(|disk| {
            disk.mount_point().to_str() == Some("/")
        })
        .map(|disk| {
            let total = disk.total_space();
            let available = disk.available_space();
            let used = total - available;
            (total, used, available)
        })
        .unwrap_or((0, 0, 0));
    
    let disk_usage_percent = if disk_total > 0 {
        (disk_used as f32 / disk_total as f32) * 100.0
    } else {
        0.0
    };
    
    // Check Docker availability
    let docker_info = match bollard::Docker::connect_with_local_defaults() {
        Ok(docker) => {
            match docker.version().await {
                Ok(version) => DockerInfo {
                    available: true,
                    version: Some(version.version.unwrap_or_else(|| "unknown".to_string())),
                },
                Err(_) => DockerInfo {
                    available: false,
                    version: None,
                },
            }
        }
        Err(_) => DockerInfo {
            available: false,
            version: None,
        },
    };
    
    let system_info = SystemInfo {
        hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
        os: System::name().unwrap_or_else(|| "unknown".to_string()),
        kernel_version: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
        cpu_count: sys.cpus().len(),
        cpu_usage_percent: cpu_usage,
        memory_total_bytes: memory_total,
        memory_used_bytes: memory_used,
        memory_available_bytes: memory_available,
        memory_usage_percent: memory_usage_percent,
        disk_total_bytes: disk_total,
        disk_used_bytes: disk_used,
        disk_available_bytes: disk_available,
        disk_usage_percent: disk_usage_percent,
    };
    
    let uptime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() - start_time;
    
    (StatusCode::OK, Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_seconds: uptime,
        system: system_info,
        docker: docker_info,
    })).into_response()
}
