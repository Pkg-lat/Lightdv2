use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub version: String,
    pub server: ServerConfig,
    pub authorization: AuthConfig,
    pub docker: DockerConfig,
    pub storage: StorageConfig,
    pub monitoring: MonitoringConfig,
    #[serde(default)]
    pub remote: Option<RemoteConfig>,
    #[serde(default)]
    pub sftp: Option<SftpConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SftpConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DockerConfig {
    pub socket_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    pub base_path: String,
    pub containers_path: String,
    pub volumes_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MonitoringConfig {
    pub enabled: bool,
    pub interval_ms: u64,
    pub billing: BillingConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BillingConfig {
    pub memory_per_gb_hour: f64,
    pub cpu_per_vcpu_hour: f64,
    pub storage_per_gb_hour: f64,
    pub egress_per_gb: f64,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn get_version(&self) -> &str {
        &self.version
    }
}
