use serde::{Deserialize, Serialize};
use sled::Db;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Protocols accepted by the pool.
///
/// The value reaches `iptables -p` (as its own argv element, so this is not shell
/// injection) and is persisted, so an unchecked string is a validation gap: bad
/// values fail at the OS or get replayed into rules that never match.
const VALID_PROTOCOLS: [&str; 3] = ["tcp", "udp", "sctp"];

pub fn validate_protocol(protocol: &str) -> Result<(), String> {
    if VALID_PROTOCOLS.contains(&protocol) {
        Ok(())
    } else {
        Err(format!(
            "Unsupported protocol {:?}: expected one of {}",
            protocol,
            VALID_PROTOCOLS.join(", ")
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPort {
    pub id: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String, // "tcp" or "udp"
    pub in_use: bool,
    pub created_at: u64,
    /// Container that currently holds this port, when `in_use`.
    ///
    /// Needed so a port can be returned to the pool by identity instead of by
    /// guessing from a port number, which released the wrong entry when two
    /// containers shared a port.
    #[serde(default)]
    pub assigned_to: Option<String>,
}

pub struct NetworkPool {
    db: Arc<Db>,
    /// Serializes the read-then-write of port reservation.
    ///
    /// `get_random_available` followed by `mark_in_use` is a check-then-act: two
    /// concurrent container creations both observe the same free port and both
    /// claim it. Holding this across both steps makes the pair atomic.
    reserve_lock: Arc<Mutex<()>>,
}

impl NetworkPool {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = sled::open(db_path)?;
        Ok(Self {
            db: Arc::new(db),
            reserve_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn add_port(&self, ip: String, port: u16, protocol: Option<String>) -> Result<NetworkPort, Box<dyn std::error::Error + Send + Sync>> {
        let protocol = protocol.unwrap_or_else(|| "tcp".to_string());
        validate_protocol(&protocol)?;

        // Port 0 means "any port" to the kernel, which is neither a usable pool
        // entry nor a meaningful iptables match.
        if port == 0 {
            return Err("Port must be between 1 and 65535".into());
        }

        let id = Uuid::new_v4().to_string();
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let network_port = NetworkPort {
            id: id.clone(),
            ip: ip.clone(),
            port,
            protocol: protocol.clone(),
            in_use: false,
            created_at,
            assigned_to: None,
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
            if !in_use {
                port.assigned_to = None;
            }
            let serialized = serde_json::to_vec(&port)?;
            self.db.insert(id.as_bytes(), serialized)?;
            tracing::info!("Marked port {} as in_use={}", id, in_use);
            Ok(port)
        } else {
            Err("Port not found".into())
        }
    }

    /// Atomically claim an available port for `owner`.
    ///
    /// Reserving and marking are done under one lock so two concurrent callers
    /// cannot be handed the same host port. The previous two-step API
    /// (`get_random_available` then `mark_in_use`) was a check-then-act race.
    pub async fn reserve_port_for(
        &self,
        owner: &str,
    ) -> Result<Option<NetworkPort>, Box<dyn std::error::Error + Send + Sync>> {
        let _guard = self.reserve_lock.lock().await;

        let ports = self.get_all_ports().await?;
        let available: Vec<NetworkPort> = ports
            .into_iter()
            .filter(|p| !p.in_use && p.assigned_to.is_none())
            .collect();

        if available.is_empty() {
            return Ok(None);
        }

        let mut chosen = {
            use rand::seq::SliceRandom;
            let mut rng = rand::thread_rng();
            available
                .choose(&mut rng)
                .cloned()
                .expect("available is non-empty")
        };

        chosen.in_use = true;
        chosen.assigned_to = Some(owner.to_string());

        let serialized = serde_json::to_vec(&chosen)?;
        self.db.insert(chosen.id.as_bytes(), serialized)?;

        tracing::info!(
            "Reserved port {}:{} for {}",
            chosen.ip,
            chosen.port,
            owner
        );

        Ok(Some(chosen))
    }

    /// Return a port to the pool, identifying the holder by container id.
    ///
    /// Matching on `assigned_to` rather than a port number matters because the
    /// caller used to locate the entry by `port == host_port`, which released
    /// whichever entry happened to match first when duplicates existed.
    pub async fn return_ports_for(
        &self,
        owner: &str,
    ) -> Result<Vec<NetworkPort>, Box<dyn std::error::Error + Send + Sync>> {
        let mut released = Vec::new();

        for item in self.db.iter() {
            let (key, value) = item?;
            let mut port: NetworkPort = match serde_json::from_slice(&value) {
                Ok(port) => port,
                Err(_) => continue,
            };

            if port.assigned_to.as_deref() != Some(owner) {
                continue;
            }

            port.in_use = false;
            port.assigned_to = None;

            let serialized = serde_json::to_vec(&port)?;
            self.db.insert(&key, serialized)?;
            released.push(port);
        }

        Ok(released)
    }

    pub async fn delete_port(&self, id: &str) -> Result<NetworkPort, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(port) = self.get_port(id).await? {
            // Refuse to delete a claimed port. Removing it freed the slot in the
            // pool while the container kept its host binding, so the pool would hand
            // the same port to the next container.
            if port.in_use || port.assigned_to.is_some() {
                return Err(format!(
                    "Port {} is currently in use by {}; release it first",
                    port.port,
                    port.assigned_to.as_deref().unwrap_or("a container")
                )
                .into());
            }

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
            match self.delete_port(&id).await {
                Ok(port) => deleted.push(port.id),
                // Surface in-use ports instead of silently skipping them; a bulk
                // delete that quietly does nothing is how the port leak went unnoticed.
                Err(e) => tracing::warn!("Skipped port {}: {}", id, e),
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
