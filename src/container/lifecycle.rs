use super::manager::ContainerManager;
use crate::config::config::Config as AppConfig;

use bollard::Docker;
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions, RemoveContainerOptions};
use bollard::models::{HostConfig, Mount, MountTypeEnum};

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

#[allow(unused)]
#[derive(Debug, Clone)]
pub enum LifecycleEvent {
    Started(String),
    DockerConnected,
    CreatingContainer(String),
    ContainerCreated(String, String),
    RunningInstallScript(String),
    InstallScriptComplete(String, i32),
    SettingUpEntrypoint(String),
    Ready(String),
    Error(String, String),
    // Reinstall events
    ReinstallStarted(String),
    RemovingOldContainer(String),
    // Repair events
    RepairStarted(String),
    CorruptionDetected(String, String),
}

pub struct LifecycleManager {
    manager: Arc<ContainerManager>,
    docker: Docker,
    event_tx: mpsc::UnboundedSender<LifecycleEvent>,
    base_path: PathBuf,
}

impl LifecycleManager {
    pub fn new(
        manager: Arc<ContainerManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<LifecycleEvent>), Box<dyn std::error::Error + Send + Sync>> {
        let docker = Docker::connect_with_local_defaults()?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        
        // Load config to get the storage path
        let config = AppConfig::load("config.json")
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { 
                format!("Failed to load config: {}", e).into() 
            })?;
        let base_path = PathBuf::from(&config.storage.base_path);

        Ok((
            Self {
                manager,
                docker,
                event_tx,
                base_path,
            },
            event_rx,
        ))
    }

    /// Verify Docker daemon is running and accessible
    pub async fn check_docker(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match self.docker.ping().await {
            Ok(_) => {
                let _ = self.event_tx.send(LifecycleEvent::DockerConnected);
                tracing::info!("Docker marked as accessible and ready.");
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("Docker daemon is not accessible: {}. Please ensure Docker is running u muppet!", e);
                tracing::error!("{}", error_msg);
                Err(error_msg.into())
            }
        }
    }

    pub async fn install_container(
        &self,
        internal_id: String,
        image: String,
        install_script: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // First verify Docker is available
        self.check_docker().await?;

        let manager = self.manager.clone();
        let docker = self.docker.clone();
        let event_tx = self.event_tx.clone();
        let base_path = self.base_path.clone();

        // Spawn async non-blocking job
        tokio::spawn(async move {
            if let Err(e) = Self::install_container_job(
                manager.clone(),
                docker,
                event_tx.clone(),
                internal_id.clone(),
                image,
                install_script,
                base_path,
            )
            .await
            {
                let error_msg = e.to_string();
                let _ = event_tx.send(LifecycleEvent::Error(
                    internal_id.clone(),
                    error_msg.clone(),
                ));
                
                // Mark the container as failed in the database
                if let Err(mark_err) = manager.mark_failed(&internal_id, &error_msg).await {
                    tracing::error!("Failed to mark container {} as failed: {}", internal_id, mark_err);
                }
                
                tracing::error!("Container installation failed for {}: {}", internal_id, error_msg);
            }
        });

        Ok(())
    }

    async fn install_container_job(
        manager: Arc<ContainerManager>,
        docker: Docker,
        event_tx: mpsc::UnboundedSender<LifecycleEvent>,
        internal_id: String,
        image: String,
        install_script: Option<String>,
        base_path: PathBuf,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = event_tx.send(LifecycleEvent::Started(internal_id.clone()));

        // Get container state
        let state = manager
            .get_container(&internal_id)
            .await?
            .ok_or_else(|| format!("Container state not found for internal_id: {}", internal_id))?;

        // Create absolute paths for volumes
        let volume_path = base_path.join("volumes").join(&state.volume_id);
        let container_data_path = base_path.join("containers").join(&internal_id);

        // Ensure paths exist
        tokio::fs::create_dir_all(&volume_path).await?;
        tokio::fs::create_dir_all(&container_data_path).await?;

        // Convert to absolute path strings
        let volume_path_str = volume_path.canonicalize()?.to_string_lossy().to_string();
        let container_data_path_str = container_data_path.canonicalize()?.to_string_lossy().to_string();

        let mut mounts = vec![
            Mount {
                target: Some("/home/container".to_string()),
                source: Some(volume_path_str.clone()),
                typ: Some(MountTypeEnum::BIND),
                ..Default::default()
            },
            Mount {
                target: Some("/app/data".to_string()),
                source: Some(container_data_path_str.clone()),
                typ: Some(MountTypeEnum::BIND),
                ..Default::default()
            },
        ];

        // Add custom mounts
        for (target, source) in &state.mount {
            mounts.push(Mount {
                target: Some(target.clone()),
                source: Some(source.clone()),
                typ: Some(MountTypeEnum::BIND),
                ..Default::default()
            });
        }

        let _ = event_tx.send(LifecycleEvent::CreatingContainer(internal_id.clone()));

        // Create container config
        let mut host_config = HostConfig {
            mounts: Some(mounts),
            ..Default::default()
        };

        // Apply limits
        if let Some(memory) = state.limits.memory {
            host_config.memory = Some(memory);
        }
        if let Some(cpu) = state.limits.cpu {
            host_config.nano_cpus = Some((cpu * 1_000_000_000.0) as i64);
        }

        let container_name = format!("lightd-{}", internal_id);

        // Check if container already exists and remove it
        if let Ok(Some(_)) = docker.inspect_container(&container_name, None).await.map(Some).or_else(|e| {
            if e.to_string().contains("404") || e.to_string().contains("No such container") {
                Ok(None)
            } else {
                Err(e)
            }
        }) {
            tracing::info!("Removing existing container: {}", container_name);
            docker.remove_container(&container_name, Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            })).await?;
        }

        // Write initial entrypoint.sh (will be updated later)
        let entrypoint_path = container_data_path.join("entrypoint.sh");
        tokio::fs::write(&entrypoint_path, "#!/bin/sh\necho 'Container initializing...'\nsleep infinity\n").await?;

        let config = Config {
            image: Some(image.clone()),
            working_dir: Some("/home/container".to_string()),
            host_config: Some(host_config),
            entrypoint: Some(vec!["/bin/sh".to_string(), "/app/data/entrypoint.sh".to_string()]),
            tty: Some(true),
            open_stdin: Some(true),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: container_name.clone(),
            ..Default::default()
        };

        let container = docker.create_container(Some(options), config).await?;
        let container_id = container.id;

        let _ = event_tx.send(LifecycleEvent::ContainerCreated(
            internal_id.clone(),
            container_id.clone(),
        ));

        tracing::info!("Container {} created with Docker ID: {}", internal_id, container_id);

        // Run install script if provided
        if let Some(script) = install_script {
            let _ = event_tx.send(LifecycleEvent::RunningInstallScript(internal_id.clone()));

            // Write install script
            let install_path = container_data_path.join("install.sh");
            tokio::fs::write(&install_path, &script).await?;

            // Update entrypoint to run install script
            let install_entrypoint = 
                "#!/bin/sh\ncd /home/container\n/bin/sh /app/data/install.sh\nexit_code=$?\necho \"Install script exited with code: $exit_code\"\nexit 0\n";
            tokio::fs::write(&entrypoint_path, install_entrypoint).await?;

            // Start container for installation
            docker
                .start_container(&container_id, None::<StartContainerOptions<String>>)
                .await?;

            tracing::info!("Started container {} for installation", internal_id);

            // Wait for container to stop (install complete) with timeout
            let timeout = tokio::time::Duration::from_secs(300); // 5 minute timeout
            let start_time = std::time::Instant::now();
            
            loop {
                if start_time.elapsed() > timeout {
                    tracing::warn!("Installation timeout for container {}", internal_id);
                    break;
                }

                match docker.inspect_container(&container_id, None).await {
                    Ok(info) => {
                        if let Some(state) = info.state {
                            if state.running == Some(false) {
                                let exit_code = state.exit_code.unwrap_or(-1);
                                let _ = event_tx.send(LifecycleEvent::InstallScriptComplete(
                                    internal_id.clone(),
                                    exit_code as i32,
                                ));
                                tracing::info!("Install script for {} completed with exit code: {}", internal_id, exit_code);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to inspect container during install: {}", e);
                    }
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }

        // Setup final entrypoint with startup command
        let _ = event_tx.send(LifecycleEvent::SettingUpEntrypoint(internal_id.clone()));

        let final_entrypoint = format!(
            "#!/bin/sh\ncd /home/container\nexec sh -c '{}'\n",
            state.startup_command.replace("'", "'\\''")
        );
        tokio::fs::write(&entrypoint_path, final_entrypoint).await?;

        tracing::info!("Set up final entrypoint for container {}", internal_id);

        // Mark as ready in database
        manager.mark_ready(&internal_id, container_id.clone()).await?;

        let _ = event_tx.send(LifecycleEvent::Ready(internal_id.clone()));

        tracing::info!("Container {} installation complete and ready", internal_id);
        Ok(())
    }

    /// Reinstall a container with a new install script
    /// This will remove the existing Docker container and create a new one
    pub async fn reinstall_container(
        &self,
        internal_id: String,
        image: String,
        install_script: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // First verify Docker is available
        self.check_docker().await?;

        // Mark as installing in DB first
        self.manager.mark_installing(&internal_id).await?;

        let manager = self.manager.clone();
        let docker = self.docker.clone();
        let event_tx = self.event_tx.clone();
        let base_path = self.base_path.clone();

        let _ = event_tx.send(LifecycleEvent::ReinstallStarted(internal_id.clone()));

        // Spawn async non-blocking job
        tokio::spawn(async move {
            // First try to remove the old container
            let container_name = format!("lightd-{}", internal_id);
            let _ = event_tx.send(LifecycleEvent::RemovingOldContainer(internal_id.clone()));

            // Try to remove old container (ignore errors if it doesn't exist)
            let _ = docker.remove_container(&container_name, Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            })).await;

            tracing::info!("Removed old container {} for reinstall", container_name);

            // Now run the install job
            if let Err(e) = Self::install_container_job(
                manager.clone(),
                docker,
                event_tx.clone(),
                internal_id.clone(),
                image,
                install_script,
                base_path,
            )
            .await
            {
                let error_msg = e.to_string();
                let _ = event_tx.send(LifecycleEvent::Error(
                    internal_id.clone(),
                    error_msg.clone(),
                ));
                
                // Mark the container as failed in the database
                if let Err(mark_err) = manager.mark_failed(&internal_id, &error_msg).await {
                    tracing::error!("Failed to mark container {} as failed: {}", internal_id, mark_err);
                }
                
                tracing::error!("Container reinstall failed for {}: {}", internal_id, error_msg);
            }
        });

        Ok(())
    }

    /// Check for corruption and automatically repair if needed
    /// Returns true if container was repaired, false if no repair needed
    pub async fn repair_if_corrupted(
        &self,
        internal_id: String,
        image: String,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let (is_valid, issue) = self.manager.validate_container(&internal_id).await?;

        if is_valid {
            tracing::info!("Container {} is healthy, no repair needed", internal_id);
            return Ok(false);
        }

        let issue_msg = issue.unwrap_or_else(|| "Unknown issue".to_string());
        let _ = self.event_tx.send(LifecycleEvent::CorruptionDetected(
            internal_id.clone(),
            issue_msg.clone(),
        ));
        let _ = self.event_tx.send(LifecycleEvent::RepairStarted(internal_id.clone()));

        tracing::warn!("Container {} is corrupted ({}), starting repair", internal_id, issue_msg);

        // Trigger a reinstall to repair
        self.reinstall_container(internal_id, image, None).await?;

        Ok(true)
    }

    /// Verify Docker container exists and matches DB state
    pub async fn verify_container_sync(
        &self,
        internal_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let state = self.manager.get_container(internal_id).await?;
        
        match state {
            Some(container_state) => {
                if let Some(container_id) = &container_state.container_id {
                    // Check if Docker container exists
                    match self.docker.inspect_container(container_id, None).await {
                        Ok(_) => Ok(true),
                        Err(e) => {
                            if e.to_string().contains("404") || e.to_string().contains("No such container") {
                                tracing::warn!("Container {} has container_id {} but Docker container doesn't exist", internal_id, container_id);
                                Ok(false)
                            } else {
                                Err(e.into())
                            }
                        }
                    }
                } else if container_state.install_state == super::state::InstallState::Ready {
                    // Ready but no container ID - corruption
                    Ok(false)
                } else {
                    // Installing state - that's OK
                    Ok(true)
                }
            }
            None => Err("Container not found".into())
        }
    }

    /* Dead code
    pub async fn get_container_id(
        &self,
        internal_id: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        match self.manager.get_container(internal_id).await? {
            Some(state) => {
                match state.container_id {
                    Some(id) => Ok(id),
                    None => Err("Pending".into()),
                }
            }
            None => Err("Container not found".into()),
        }
    }

    /// Start a container by internal_id
    pub async fn start_container(
        &self,
        internal_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let container_id = self.get_container_id(internal_id).await?;
        
        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await?;
        
        tracing::info!("Started container {} ({})", internal_id, container_id);
        Ok(())
    }

    /// Kill (stop) a container by internal_id
    pub async fn kill_container(
        &self,
        internal_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let container_id = self.get_container_id(internal_id).await?;
        
        self.docker
            .stop_container(&container_id, None)
            .await?;
        
        tracing::info!("Killed container {} ({})", internal_id, container_id);
        Ok(())
    }

    /// Restart a container by internal_id
    pub async fn restart_container(
        &self,
        internal_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let container_id = self.get_container_id(internal_id).await?;
        
        self.docker
            .restart_container(&container_id, None)
            .await?;
        
        tracing::info!("Restarted container {} ({})", internal_id, container_id);
        Ok(())
    }*/
}
