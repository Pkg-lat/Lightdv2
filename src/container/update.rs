//! Container update system for live configuration changes
//! 
//! Uses Bollard's update_container to modify running containers without downtime

use super::manager::ContainerManager;
use bollard::Docker;
use bollard::container::UpdateContainerOptions;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event")]
pub enum UpdateEvent {
    UpdateStarted(String),
    ResourcesUpdated(String),
    VolumesUpdated(String),
    DatabaseUpdated(String),
    UpdateComplete(String),
    Error(String, String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResourceLimits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<i64>, // Memory limit in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_swap: Option<i64>, // Memory + swap limit in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_reservation: Option<i64>, // Soft memory limit in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_shares: Option<i64>, // CPU shares (relative weight)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_period: Option<i64>, // CPU CFS period in microseconds
    #[serde(skip_seriali