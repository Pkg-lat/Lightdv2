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
pub enum UpdateEvent {
    UpdateStarted { container_id: String },
    ResourcesUpdated { container_id: String },
    VolumesUpdated { container_id: String },
    DatabaseUpdated { container_id: String },
    UpdateComplete { container_id: String },
    Error { container_id: String, message: String },
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_quota: Option<i64>, // CPU CFS quota in microseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpuset_cpus: Option<String>, // CPUs to use (e.g., "0-3")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blkio_weight: Option<u16>, // Block IO weight (10-1000)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct VolumeMount {
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

pub struct ContainerUpdater {
    manager: Arc<ContainerManager>,
    docker: Docker,
    event_tx: mpsc::UnboundedSender<UpdateEvent>,
}

impl ContainerUpdater {
    pub fn new(
        manager: Arc<ContainerManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<UpdateEvent>), Box<dyn std::error::Error + Send + Sync>> {
        let docker = Docker::connect_with_local_defaults()?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Ok((
            Self {
                manager,
                docker,
                event_tx,
            },
            event_rx,
        ))
    }

    /// Update container resource limits (live, no restart required)
    pub async fn update_resources(
        &self,
        internal_id: String,
        limits: ResourceLimits,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = self.manager.clone();
        let docker = self.docker.clone();
        let event_tx = self.event_tx.clone();

        // Spawn async job
        tokio::spawn(async move {
            if let Err(e) = Self::update_resources_job(
                manager,
                docker,
                event_tx.clone(),
                internal_id.clone(),
                limits,
            )
            .await
            {
                let _ = event_tx.send(UpdateEvent::Error {
                    container_id: internal_id.clone(),
                    message: e.to_string(),
                });
                tracing::error!("Failed to update resources for {}: {}", internal_id, e);
            }
        });

        Ok(())
    }

    async fn update_resources_job(
        manager: Arc<ContainerManager>,
        docker: Docker,
        event_tx: mpsc::UnboundedSender<UpdateEvent>,
        internal_id: String,
        limits: ResourceLimits,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = event_tx.send(UpdateEvent::UpdateStarted { 
            container_id: internal_id.clone() 
        });

        // Get container state
        let state = manager
            .get_container(&internal_id)
            .await?
            .ok_or_else(|| format!("Container not found: {}", internal_id))?;

        let container_id = state
            .container_id
            .ok_or("Container not yet created")?;

        // Validate limits
        Self::validate_resource_limits(&limits)?;

        // Build update options
        let mut update_opts = UpdateContainerOptions::<String>::default();

        if let Some(memory) = limits.memory {
            update_opts.memory = Some(memory);
        }

        if let Some(memory_swap) = limits.memory_swap {
            update_opts.memory_swap = Some(memory_swap);
        }

        if let Some(memory_reservation) = limits.memory_reservation {
            update_opts.memory_reservation = Some(memory_reservation);
        }

        if let Some(cpu_shares) = limits.cpu_shares {
            update_opts.cpu_shares = Some(cpu_shares.try_into().unwrap_or(1024));
        }

        if let Some(cpu_period) = limits.cpu_period {
            update_opts.cpu_period = Some(cpu_period);
        }

        if let Some(cpu_quota) = limits.cpu_quota {
            update_opts.cpu_quota = Some(cpu_quota);
        }

        if let Some(ref cpuset_cpus) = limits.cpuset_cpus {
            update_opts.cpuset_cpus = Some(cpuset_cpus.clone());
        }

        if let Some(_blkio_weight) = limits.blkio_weight {
            // Block I/O weight is only supported on Linux
            #[cfg(target_os = "linux")]
            {
                update_opts.blkio_weight = Some(_blkio_weight);
            }
            
            #[cfg(not(target_os = "linux"))]
            {
                tracing::warn!("Block I/O weight not supported on this platform, skipping");
            }
        }

        // Apply update to Docker container
        docker.update_container(&container_id, update_opts).await?;

        let _ = event_tx.send(UpdateEvent::ResourcesUpdated { 
            container_id: internal_id.clone() 
        });

        tracing::info!("Updated resources for container {}", internal_id);

        // Update database with new limits
        manager.update_limits(&internal_id, limits.clone()).await?;

        let _ = event_tx.send(UpdateEvent::DatabaseUpdated { 
            container_id: internal_id.clone() 
        });
        let _ = event_tx.send(UpdateEvent::UpdateComplete { 
            container_id: internal_id.clone() 
        });

        tracing::info!("Container {} update complete", internal_id);
        Ok(())
    }

    /// Update container volumes (requires restart)
    pub async fn update_volumes(
        &self,
        internal_id: String,
        volumes: HashMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = self.manager.clone();
        let event_tx = self.event_tx.clone();

        // Spawn async job
        tokio::spawn(async move {
            if let Err(e) = Self::update_volumes_job(
                manager,
                event_tx.clone(),
                internal_id.clone(),
                volumes,
            )
            .await
            {
                let _ = event_tx.send(UpdateEvent::Error {
                    container_id: internal_id.clone(),
                    message: e.to_string(),
                });
                tracing::error!("Failed to update volumes for {}: {}", internal_id, e);
            }
        });

        Ok(())
    }

    async fn update_volumes_job(
        manager: Arc<ContainerManager>,
        event_tx: mpsc::UnboundedSender<UpdateEvent>,
        internal_id: String,
        volumes: HashMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = event_tx.send(UpdateEvent::UpdateStarted { 
            container_id: internal_id.clone() 
        });

        // Validate volumes
        Self::validate_volumes(&volumes)?;

        // Update database
        manager.update_volumes(&internal_id, volumes).await?;

        let _ = event_tx.send(UpdateEvent::VolumesUpdated { 
            container_id: internal_id.clone() 
        });
        let _ = event_tx.send(UpdateEvent::DatabaseUpdated { 
            container_id: internal_id.clone() 
        });
        let _ = event_tx.send(UpdateEvent::UpdateComplete { 
            container_id: internal_id.clone() 
        });

        tracing::info!("Updated volumes for container {} (restart required)", internal_id);
        Ok(())
    }

    /// Get current resource usage
    pub async fn get_current_resources(
        &self,
        internal_id: &str,
    ) -> Result<ResourceLimits, Box<dyn std::error::Error + Send + Sync>> {
        let state = self.manager
            .get_container(internal_id)
            .await?
            .ok_or_else(|| format!("Container not found: {}", internal_id))?;

        let container_id = state
            .container_id
            .ok_or("Container not yet created")?;

        // Inspect container to get current limits
        let inspect = self.docker.inspect_container(&container_id, None).await?;

        let host_config = inspect.host_config.ok_or("No host config found")?;

        Ok(ResourceLimits {
            memory: host_config.memory,
            memory_swap: host_config.memory_swap,
            memory_reservation: host_config.memory_reservation,
            cpu_shares: host_config.cpu_shares,
            cpu_period: host_config.cpu_period,
            cpu_quota: host_config.cpu_quota,
            cpuset_cpus: host_config.cpuset_cpus,
            blkio_weight: host_config.blkio_weight,
        })
    }

    /// Validate resource limits
    fn validate_resource_limits(
        limits: &ResourceLimits,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Memory validation
        if let Some(memory) = limits.memory {
            if memory < 4 * 1024 * 1024 {
                // Minimum 4MB
                return Err("Memory limit must be at least 4MB".into());
            }
            if memory > 1024 * 1024 * 1024 * 1024 {
                // Maximum 1TB
                return Err("Memory limit cannot exceed 1TB".into());
            }
        }

        // Memory swap validation
        if let (Some(memory), Some(memory_swap)) = (limits.memory, limits.memory_swap) {
            if memory_swap != -1 && memory_swap < memory {
                return Err("Memory swap must be greater than or equal to memory limit".into());
            }
        }

        // CPU shares validation
        if let Some(cpu_shares) = limits.cpu_shares {
            if cpu_shares < 2 || cpu_shares > 262144 {
                return Err("CPU shares must be between 2 and 262144".into());
            }
        }

        // CPU period validation
        if let Some(cpu_period) = limits.cpu_period {
            if cpu_period < 1000 || cpu_period > 1000000 {
                return Err("CPU period must be between 1000 and 1000000 microseconds".into());
            }
        }

        // CPU quota validation
        if let Some(cpu_quota) = limits.cpu_quota {
            if cpu_quota < 1000 && cpu_quota != -1 {
                return Err("CPU quota must be at least 1000 microseconds or -1 for unlimited".into());
            }
        }

        // Block IO weight validation
        if let Some(blkio_weight) = limits.blkio_weight {
            #[cfg(not(target_os = "linux"))]
            {
                tracing::warn!("Block I/O weight is only supported on Linux and will be ignored");
            }
            
            if blkio_weight < 10 || blkio_weight > 1000 {
                return Err("Block IO weight must be between 10 and 1000".into());
            }
        }

        Ok(())
    }

    /// Validate volumes
    fn validate_volumes(
        volumes: &HashMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for (target, source) in volumes {
            // Validate target path
            if target.is_empty() || !target.starts_with('/') {
                return Err(format!("Invalid target path: {}", target).into());
            }

            // Validate source path
            if source.is_empty() {
                return Err(format!("Invalid source path for target: {}", target).into());
            }

            // Check for dangerous paths
            // Lightd is secure by default mate.
            let dangerous_paths = ["/", "/bin", "/boot", "/dev", "/etc", "/lib", "/proc", "/sys"];
            if dangerous_paths.contains(&target.as_str()) {
                return Err(format!("Cannot mount to system path: {}", target).into());
            }
        }

        Ok(())
    }
}
