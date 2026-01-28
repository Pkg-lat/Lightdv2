//! Firewall API routes

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::network::firewall::{
    DDoSProtection, FirewallAction, FirewallManager, FirewallRule, Protocol, RateLimit,
};

#[derive(Clone)]
pub struct FirewallState {
    manager: Arc<FirewallManager>,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct SuccessResponse {
    message: String,
}

#[derive(Deserialize)]
struct CreateRuleRequest {
    container_id: String,
    source_ip: Option<String>,
    source_port: Option<u16>,
    dest_port: Option<u16>,
    protocol: Protocol,
    action: FirewallAction,
    rate_limit: Option<RateLimit>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct ToggleRuleRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct DDoSProtectionRequest {
    enabled: bool,
    syn_flood_protection: bool,
    connection_limit: Option<u32>,
    rate_limit: Option<RateLimit>,
}

#[derive(Serialize)]
struct RuleResponse {
    rule: FirewallRule,
}

#[derive(Serialize)]
struct RulesListResponse {
    rules: Vec<FirewallRule>,
}

#[derive(Serialize)]
struct NetworkResponse {
    network_name: String,
}

pub fn firewall_router(manager: Arc<FirewallManager>) -> Router {
    let state = FirewallState { manager };

    Router::new()
        .route("/firewall/networks/:container_id", post(create_network))
        .route("/firewall/networks/:container_id", delete(delete_network))
        .route("/firewall/rules", post(create_rule))
        .route("/firewall/rules/:rule_id", delete(delete_rule))
        .route("/firewall/rules/:rule_id/toggle", put(toggle_rule))
        .route("/firewall/rules/container/:container_id", get(get_container_rules))
        .route("/firewall/ddos/:container_id", post(enable_ddos_protection))
        .route("/firewall/cleanup/:container_id", delete(cleanup_container))
        .with_state(state)
}

/// Create isolated network for container
async fn create_network(
    State(state): State<FirewallState>,
    Path(container_id): Path<String>,
) -> Response {
    match state.manager.create_container_network(&container_id).await {
        Ok(network_name) => (
            StatusCode::CREATED,
            Json(NetworkResponse { network_name }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Delete container network
async fn delete_network(
    State(state): State<FirewallState>,
    Path(container_id): Path<String>,
) -> Response {
    match state.manager.remove_container_network(&container_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: "Network removed successfully".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Create a firewall rule
async fn create_rule(
    State(state): State<FirewallState>,
    Json(req): Json<CreateRuleRequest>,
) -> Response {
    let rule = FirewallRule {
        id: Uuid::new_v4().to_string(),
        container_id: req.container_id,
        source_ip: req.source_ip,
        source_port: req.source_port,
        dest_port: req.dest_port,
        protocol: req.protocol,
        action: req.action,
        rate_limit: req.rate_limit,
        description: req.description,
        enabled: true,
    };

    match state.manager.add_rule(rule.clone()).await {
        Ok(()) => (StatusCode::CREATED, Json(RuleResponse { rule })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Delete a firewall rule
async fn delete_rule(
    State(state): State<FirewallState>,
    Path(rule_id): Path<String>,
) -> Response {
    match state.manager.remove_rule(&rule_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: "Rule deleted successfully".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Toggle a firewall rule on/off
async fn toggle_rule(
    State(state): State<FirewallState>,
    Path(rule_id): Path<String>,
    Json(req): Json<ToggleRuleRequest>,
) -> Response {
    match state.manager.toggle_rule(&rule_id, req.enabled).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: format!("Rule {} successfully", if req.enabled { "enabled" } else { "disabled" }),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Get all rules for a container
async fn get_container_rules(
    State(state): State<FirewallState>,
    Path(container_id): Path<String>,
) -> Response {
    let rules = state.manager.get_container_rules(&container_id).await;
    (StatusCode::OK, Json(RulesListResponse { rules })).into_response()
}

/// Enable DDoS protection for a container
async fn enable_ddos_protection(
    State(state): State<FirewallState>,
    Path(container_id): Path<String>,
    Json(req): Json<DDoSProtectionRequest>,
) -> Response {
    let protection = DDoSProtection {
        enabled: req.enabled,
        syn_flood_protection: req.syn_flood_protection,
        connection_limit: req.connection_limit,
        rate_limit: req.rate_limit,
    };

    match state
        .manager
        .enable_ddos_protection(&container_id, protection)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: "DDoS protection configured successfully".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}

/// Clean up all firewall rules for a container
async fn cleanup_container(
    State(state): State<FirewallState>,
    Path(container_id): Path<String>,
) -> Response {
    match state.manager.cleanup_container_rules(&container_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SuccessResponse {
                message: "Container firewall rules cleaned up successfully".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}
