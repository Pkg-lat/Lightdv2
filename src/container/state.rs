use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InstallState {
    Ready,
    Installing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerState {
    pub internal_id: String,
    pub volume_id: String,
    pub mount: HashMap<String, String>,
    pub limits: ContainerLimits,
    pub container_id: Option<String>,
    pub ports: Vec<PortBinding>, // Changed to Vec of PortBinding
    pub is_installing: bool,
    pub install_state: InstallState,
    pub startup_command: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// Pattern to detect when server is fully started (string or regex)
    #[serde(default)]
    pub start_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortBinding {
    pub container_port: u16,
    pub host_port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerLimits {
    #[serde(default)]
    pub memory: Option<i64>,
    #[serde(default)]
    pub memory_swap: Option<i64>,
    #[serde(default)]
    pub memory_reservation: Option<i64>,
    #[serde(default)]
    pub cpu: Option<f64>,
    #[serde(default)]
    pub cpu_shares: Option<i64>,
    #[serde(default)]
    pub cpu_period: Option<i64>,
    #[serde(default)]
    pub cpu_quota: Option<i64>,
    #[serde(default)]
    pub cpuset_cpus: Option<String>,
    #[serde(default)]
    pub blkio_weight: Option<u16>,
    #[serde(default)]
    pub disk: Option<i64>,
}

impl ContainerState {
    pub fn new(
        internal_id: String,
        volume_id: String,
        startup_command: String,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            internal_id,
            volume_id,
            mount: HashMap::new(),
            limits: ContainerLimits {
                memory: None,
                memory_swap: None,
                memory_reservation: None,
                cpu: None,
                cpu_shares: None,
                cpu_period: None,
                cpu_quota: None,
                cpuset_cpus: None,
                blkio_weight: None,
                disk: None,
            },
            container_id: None,
            ports: Vec::new(),
            is_installing: false,
            install_state: InstallState::Ready,
            startup_command,
            created_at: now,
            updated_at: now,
            start_pattern: None,
        }
    }

    pub fn update_timestamp(&mut self) {
        self.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
}

