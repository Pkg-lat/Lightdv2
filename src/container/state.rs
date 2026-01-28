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
    pub ip: String,
    pub port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerLimits {
    pub memory: Option<i64>,
    pub cpu: Option<f64>,
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
                cpu: None,
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

