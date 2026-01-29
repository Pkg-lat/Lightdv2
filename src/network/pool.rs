use serde::{Deserialize, Serialize};
use sled::Db;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPort {
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String, // "tcp" or "udp"
    pub in_use: bool,
    pub created_at: u64,
}

pub struct NetworkPool {
    db: Arc<Db>,
}

impl NetworkPool {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = sled::open(db_path)?;
        Ok(Self { db: Arc::new(db) })
    }

    pub async fn add_port(&self, ip: String, port: u16, protocol: Option<String>) -> Result<NetworkPort, Box<dyn std::error::Error + Send + Sync>> {
        let id = Uuid::new_v4().to_string();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let protocol = protocol.unwrap_or_else(|| "tcp".to_string());

        let network_port = NetworkPort {
            id: id.clone(),
            ip: ip.clone(),
            port,
            protocol: protocol.clone(),
            in_use: false,
            created_at,
        };

        let serialized = serde_json::to_vec(&network_port)?;
        self.db.insert(id.as_bytes(), serialized)?;

        // Try to open port with iptables if available
        self.open_iptables_port(&ip, port, &protocol).await;

        tracing::info!("Added network port {}:{}/{} with ID {}", ip, port, protocol, id);
        Ok(network_port)
    }

    pub async fn get_port(&self, id: &str) -> Result<Option<NetworkPort>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = self.db.get(id.as_bytes())? {
            let port: NetworkPort = serde_json::from_slice(&data)?;
            Ok(Some(port))
        } else {
            Ok(None)
        }
    }

    pub async fn get_all_ports(&self) -> Result<Vec<NetworkPort>, Box<dyn std::error::Error + Send + Sync>> {
        let mut ports = Vec::new();
        
        for item in self.db.iter() {
            let (_, value) = item?;
            let port: NetworkPort = serde_json::from_slice(&value)?;
            ports.push(port);
        }

        Ok(ports)
    }

    pub async fn get_random_available(&self) -> Result<Option<NetworkPort>, Box<dyn std::error::Error + Send + Sync>> {
        let ports = self.get_all_ports().await?;
        let available: Vec<NetworkPort> = ports.into_iter().filter(|p| !p.in_use).collect();
        
        if available.is_empty() {
            Ok(None)
        } else {
            use rand::seq::SliceRandom;
            let mut rng = rand::thread_rng();
            Ok(available.choose(&mut rng).cloned())
        }
    }

    pub async fn mark_in_use(&self, id: &str, in_use: bool) -> Result<NetworkPort, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(mut port) = self.get_port(id).await? {
            port.in_use = in_use;
            let serialized = serde_json::to_vec(&port)?;
            self.db.insert(id.as_bytes(), serialized)?;
            tracing::info!("Marked port {} as in_use={}", id, in_use);
            Ok(port)
        } else {
            Err("Port not found".into())
        }
    }

    pub async fn delete_port(&self, id: &str) -> Result<NetworkPort, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(port) = self.get_port(id).await? {
            self.db.remove(id.as_bytes())?;
            
            // Try to close port with iptables if available
            self.close_iptables_port(&port.ip, port.port, &port.protocol).await;
            
            tracing::info!("Deleted network port {}:{}/{}", port.ip, port.port, port.protocol);
            Ok(port)
        } else {
            Err("Port not found".into())
        }
    }

    pub async fn bulk_delete(&self, ids: Vec<String>) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let mut deleted = Vec::new();
        
        for id in ids {
            if let Ok(port) = self.delete_port(&id).await {
                deleted.push(port.id);
            }
        }

        Ok(deleted)
    }

    pub async fn bulk_add(&self, ports: Vec<(String, u16, String)>) -> Result<Vec<NetworkPort>, Box<dyn std::error::Error + Send + Sync>> {
        let mut added = Vec::new();
        
        for (ip, port, protocol) in ports {
            match self.add_port(ip, port, Some(protocol)).await {
                Ok(network_port) => added.push(network_port),
                Err(e) => tracing::warn!("Failed to add port {}: {}", port, e),
            }
        }

        Ok(added)
    }

    pub async fn get_available_ports(&self) -> Result<Vec<NetworkPort>, Box<dyn std::error::Error + Send + Sync>> {
        let ports = self.get_all_ports().await?;
        Ok(ports.into_iter().filter(|p| !p.in_use).collect())
    }

    pub async fn return_port_to_pool(&self, id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.mark_in_use(id, false).await?;
        tracing::info!("Returned port {} to pool", id);
        Ok(())
    }

    async fn open_iptables_port(&self, ip: &str, port: u16, protocol: &str) {
        #[cfg(unix)]
        {
            let result = tokio::process::Command::new("which")
                .arg("iptables")
                .output()
                .await;

            if result.is_ok() && result.unwrap().status.success() {
                let cmd_result = tokio::process::Command::new("iptables")
                    .args(&[
                        "-A", "INPUT",
                        "-p", protocol,
                        "-d", ip,
                        "--dport", &port.to_string(),
                        "-j", "ACCEPT"
                    ])
                    .output()
                    .await;

                match cmd_result {
                    Ok(output) if output.status.success() => {
                        tracing::info!("Opened iptables port {}:{}/{}", ip, port, protocol);
                    }
                    _ => {
                        tracing::warn!("Failed to open iptables port {}:{}/{}", ip, port, protocol);
                    }
                }
            }
        }
    }

    async fn close_iptables_port(&self, ip: &str, port: u16, protocol: &str) {
        #[cfg(unix)]
        {
            let result = tokio::process::Command::new("which")
                .arg("iptables")
                .output()
                .await;

            if result.is_ok() && result.unwrap().status.success() {
                let cmd_result = tokio::process::Command::new("iptables")
                    .args(&[
                        "-D", "INPUT",
                        "-p", protocol,
                        "-d", ip,
                        "--dport", &port.to_string(),
                        "-j", "ACCEPT"
                    ])
                    .output()
                    .await;

                match cmd_result {
                    Ok(output) if output.status.success() => {
                        tracing::info!("Closed iptables port {}:{}/{}", ip, port, protocol);
                    }
                    _ => {
                        tracing::warn!("Failed to close iptables port {}:{}/{}", ip, port, protocol);
                    }
                }
            }
        }
    }
}
