//! Billing API routes

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::billing::tracker::BillingTracker;
use crate::billing::estimator::{CostEstimator, ResourceConfig};

#[derive(Clone)]
pub struct BillingState {
    tracker: Arc<BillingTracker>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
struct EstimateRequest {
    memory_gb: f64,
    cpu_vcpus: f64,
    storage_gb: f64,
    egress_gb_per_month: f64,
}

#[derive(Deserialize)]
struct VolumeEstimateRequest {
    size_gb: f64,
}

#[derive(Serialize)]
struct UsageResponse {
    container_id: String,
    memory_gb: f64,
    cpu_vcpus: f64,
    storage_gb: f64,
    egress_gb: f64,
    duration_hours: f64,
    estimated_cost: f64,
}

#[derive(Serialize)]
struct RatesResponse {
    memory_per_gb_hour: f64,
    cpu_per_vcpu_hour: f64,
    storage_per_gb_hour: f64,
    egress_per_gb: f64,
}

pub fn billing_router(tracker: Arc<BillingTracker>) -> Router {
    let state = BillingState { tracker };

    Router::new()
        .route("/billing/rates", get(get_rates))
        .route("/billing/estimate", post(estimate_cost))
        .route("/billing/estimate/volume", post(estimate_volume_cost))
        .route("/billing/usage/:container_id/hourly", get(get_hourly_usage))
        .route("/billing/usage/:container_id/daily", get(get_daily_usage))
        .route("/billing/usage/:container_id/monthly", get(get_monthly_usage))
        .route("/billing/containers", get(list_tracked_containers))
        .with_state(state)
}

/// Get current billing rates
async fn get_rates(State(state): State<BillingState>) -> Response {
    let rates = state.tracker.get_rates().await;
    
    (
        StatusCode::OK,
        Json(RatesResponse {
            memory_per_gb_hour: rates.memory_per_gb_hour,
            cpu_per_vcpu_hour: rates.cpu_per_vcpu_hour,
            storage_per_gb_hour: rates.storage_per_gb_hour,
            egress_per_gb: rates.egress_per_gb,
        }),
    )
        .into_response()
}

/// Estimate costs for a configuration
async fn estimate_cost(
    State(state): State<BillingState>,
    Json(req): Json<EstimateRequest>,
) -> Response {
    let rates = state.tracker.get_rates().await;
    let estimator = CostEstimator::new(rates);
    
    let config = ResourceConfig {
        memory_gb: req.memory_gb,
        cpu_vcpus: req.cpu_vcpus,
        storage_gb: req.storage_gb,
        egress_gb_per_month: req.egress_gb_per_month,
    };
    
    let estimate = estimator.estimate(&config);
    
    (StatusCode::OK, Json(estimate)).into_response()
}

/// Estimate costs for a volume
async fn estimate_volume_cost(
    State(state): State<BillingState>,
    Json(req): Json<VolumeEstimateRequest>,
) -> Response {
    let rates = state.tracker.get_rates().await;
    let estimator = CostEstimator::new(rates);
    
    let estimate = estimator.estimate_volume(req.size_gb);
    
    (StatusCode::OK, Json(estimate)).into_response()
}

/// Get hourly usage and cost
async fn get_hourly_usage(
    State(state): State<BillingState>,
    Path(container_id): Path<String>,
) -> Response {
    match state.tracker.get_usage_snapshot(&container_id, 1.0).await {
        Ok(snapshot) => {
            let cost = state.tracker.calculate_cost(&snapshot).await;
            
            (
                StatusCode::OK,
                Json(UsageResponse {
                    container_id,
                    memory_gb: snapshot.memory_gb,
                    cpu_vcpus: snapshot.cpu_vcpus,
                    storage_gb: snapshot.storage_gb,
                    egress_gb: snapshot.egress_gb,
                    duration_hours: snapshot.duration_hours,
                    estimated_cost: cost,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Get daily usage and cost
async fn get_daily_usage(
    State(state): State<BillingState>,
    Path(container_id): Path<String>,
) -> Response {
    match state.tracker.get_usage_snapshot(&container_id, 24.0).await {
        Ok(snapshot) => {
            let cost = state.tracker.calculate_cost(&snapshot).await;
            
            (
                StatusCode::OK,
                Json(UsageResponse {
                    container_id,
                    memory_gb: snapshot.memory_gb,
                    cpu_vcpus: snapshot.cpu_vcpus,
                    storage_gb: snapshot.storage_gb,
                    egress_gb: snapshot.egress_gb,
                    duration_hours: snapshot.duration_hours,
                    estimated_cost: cost,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Get monthly usage and cost
async fn get_monthly_usage(
    State(state): State<BillingState>,
    Path(container_id): Path<String>,
) -> Response {
    match state.tracker.get_usage_snapshot(&container_id, 720.0).await {
        Ok(snapshot) => {
            let cost = state.tracker.calculate_cost(&snapshot).await;
            
            (
                StatusCode::OK,
                Json(UsageResponse {
                    container_id,
                    memory_gb: snapshot.memory_gb,
                    cpu_vcpus: snapshot.cpu_vcpus,
                    storage_gb: snapshot.storage_gb,
                    egress_gb: snapshot.egress_gb,
                    duration_hours: snapshot.duration_hours,
                    estimated_cost: cost,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// List all tracked containers
async fn list_tracked_containers(State(state): State<BillingState>) -> Response {
    let containers = state.tracker.get_tracked_containers().await;
    
    (StatusCode::OK, Json(containers)).into_response()
}
