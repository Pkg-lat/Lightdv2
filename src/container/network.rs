use super::manager::ContainerManager;
use super::state::PortBinding;
use crate::config::config::Config;
use bollard::Docker;
use bollard::container::{RemoveContainerOptions, Config as ContainerConfig, CreateContainerOptions};
use bollard::models::{HostConfig, Mount, MountTypeEnum, PortBinding as DockerPortBinding, PortMap};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

#[derive(Debug, Clone)]
#[allow(unused)]
pub enum NetworkEvent {
    RebindingStarted(String),
    RemovingOldContainer(String),
    CreatingNewContainer(String),
    UpdatingDatabase(String),
    RebindingComplete(String),
    Error(String, String),
}

pub struct NetworkRebinder {
    manager: Arc<ContainerManager>,
    docker: Docker,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
    config: Config,
}

impl NetworkRebinder {
    pub fn new(
        manager: Arc<ContainerManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<NetworkEvent>), Box<dyn std::error::Error>> {
        let docker = Docker::connect_with_local_defaults()?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let config = Config::load("config.json")?;

        Ok((
            Self {
                manager,
                docker,
                event_tx,
                config,
            },
            event_rx,
        ))
    }

    pub async fn rebind_ports(
        &self,
        internal_id: String,
        new_ports: Vec<PortBinding>,
        image: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Validate inputs
        if internal_id.trim().is_empty() {
            return Err("Internal ID cannot be empty".into());
        }
        
        if image.trim().is_empty() {
            return Err("Image cannot be empty".into());
        }

        // Validate ports
        for port in &new_ports {
            if port.container_port == 0 {
                return Err("Container port cannot be 0".into());
            }
            if port.host_port == 0 {
                return Err("Host port cannot be 0".into());
            }
            let protocol = port.protocol.to_lowercase();
            if protocol != "tcp" && protocol != "udp" {
                return Err(format!("Invalid protocol '{}', must be 'tcp' or 'udp'", port.protocol).into());
            }
        }

        let manager = self.manager.clone();
        let docker = self.docker.clone();
        let event_tx = self.event_tx.clone();
        let config = self.config.clone();

        // Spawn async non-blocking job
        tokio::spawn(async move {
            if let Err(e) = Self::rebind_ports_job(
                manager,
                docker,
                event_tx.clone(),
                internal_id.clone(),
                new_ports,
                image,
                config,
            )
            .await
            {
                let _ = event_tx.send(NetworkEvent::Error(internal_id.clone(), e.to_string()));
                tracing::error!("Network rebinding failed for {}: {}", internal_id, e);
            }
        });

        Ok(())
    }

    async fn rebind_ports_job(
        manager: Arc<ContainerManager>,
        docker: Docker,
        event_tx: mpsc::UnboundedSender<NetworkEvent>,
        internal_id: String,
        new_ports: Vec<PortBinding>,
        image: String,
        config: Config,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _ = event_tx.send(NetworkEvent::RebindingStarted(internal_id.clone()));

        // Get container state with timeout
        let state_result = timeout(
            Duration::from_secs(5),
            manager.get_container(&internal_id)
        ).await;

        let mut state = match state_result {
            Ok(Ok(Some(state))) => state,
            Ok(Ok(None)) => return Err(format!("Container '{}' not found", internal_id).into()),
            Ok(Err(e)) => return Err(format!("Failed to get container: {}", e).into()),
            Err(_) => return Err("Timeout while fetching container state".into()),
        };

        // Check if container is installing
        if state.is_installing {
            return Err("Cannot rebind network while container is installing".into());
        }

        // Remove old container if exists
        if let Some(old_container_id) = &state.container_id {
            let _ = event_tx.send(NetworkEvent::RemovingOldContainer(internal_id.clone()));
            
            // Try to remove with timeout
            let remove_result = timeout(
                Duration::from_secs(30),
                docker.remove_container(
                    old_container_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: true,
                        ..Default::default()
                    }),
                )
            ).await;

            match remove_result {
                Ok(Ok(_)) => {
                    tracing::info!("Removed old container: {}", old_container_id);
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to remove old container {}: {}", old_container_id, e);
                    // Continue anyway - container might not exist
                }
                Err(_) => {
                    return Err("Timeout while removing old container".into());
                }
            }
        }

        let _ = event_tx.send(NetworkEvent::CreatingNewContainer(internal_id.clone()));

        // Use config paths
        let volume_path = format!("{}/{}", config.storage.volumes_path, state.volume_id);
        let container_data_path = format!("{}/{}", config.storage.containers_path, internal_id);
        
        // Ensure container data directory exists
        if let Err(e) = tokio::fs::create_dir_all(&container_data_path).await {
            return Err(format!("Failed to create container data directory: {}", e).into());
        }

        let mut mounts = vec![
            Mount {
                target: Some("/home/container".to_string()),
                source: Some(volume_path),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(false),
                ..Default::default()
            },
            Mount {
                target: Some("/app/data".to_string()),
                source: Some(container_data_path),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(false),
                ..Default::default()
            },
        ];

        // Add custom mounts
        for (target, source) in &state.mount {
            if target.trim().is_empty() || source.trim().is_empty() {
                tracing::warn!("Skipping invalid mount: {} -> {}", target, source);
                continue;
            }
            
            mounts.push(Mount {
                target: Some(target.clone()),
                source: Some(source.clone()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(false),
                ..Default::default()
            });
        }

        // Build port bindings with validation
        let mut port_bindings = PortMap::new();
        for port in &new_ports {
            let protocol = port.protocol.to_lowercase();
            let key = format!("{}/{}", port.container_port, protocol);
            
            port_bindings.insert(
                key,
                Some(vec![DockerPortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(port.host_port.to_string()),
                }]),
            );
        }

        let mut host_config = HostConfig {
            mounts: Some(mounts),
            port_bindings: Some(port_bindings),
            auto_remove: Some(false),
            ..Default::default()
        };

        // Apply resource limits with validation
        if let Some(memory) = state.limits.memory {
            if memory > 0 {
                host_config.memory = Some(memory);
            }
        }
        
        if let Some(cpu) = state.limits.cpu {
            if cpu > 0.0 && cpu <= 1024.0 {
                host_config.nano_cpus = Some((cpu * 1_000_000_000.0) as i64);
            }
        }

        let container_config = ContainerConfig {
            image: Some(image.clone()),
            working_dir: Some("/home/container".to_string()),
            host_config: Some(host_config),
            entrypoint: Some(vec!["/bin/sh".to_string(), "/app/data/entrypoint.sh".to_string()]),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: format!("lightd-{}", internal_id),
            ..Default::default()
        };

        // Create container with timeout
        let create_result = timeout(
            Duration::from_secs(60),
            docker.create_container(Some(options), container_config)
        ).await;

        let container = match create_result {
            Ok(Ok(container)) => container,
            Ok(Err(e)) => return Err(format!("Failed to create container: {}", e).into()),
            Err(_) => return Err("Timeout while creating container".into()),
        };

        let container_id = container.id;

        // Update state with new ports and container ID
        let _ = event_tx.send(NetworkEvent::UpdatingDatabase(internal_id.clone()));
        
        state.ports = new_ports;
        state.container_id = Some(container_id.clone());
        state.update_timestamp();

        // Update database with timeout
        let update_result = timeout(
            Duration::from_secs(5),
            manager.update_container(state)
        ).await;

        match update_result {
            Ok(Ok(_)) => {
                tracing::info!("Updated container state in database for {}", internal_id);
            }
            Ok(Err(e)) => {
                return Err(format!("Failed to update database: {}", e).into());
            }
            Err(_) => {
                return Err("Timeout while updating database".into());
            }
        }

        let _ = event_tx.send(NetworkEvent::RebindingComplete(internal_id.clone()));

        tracing::info!("Network rebinding complete for {} with container ID {}", internal_id, container_id);
        Ok(())
    }
}
