use super::manager::ContainerManager;
use crate::config::config::Config as AppConfig;

use bollard::Docker;
use bollard::container::{Config, CreateContainerOptions, StartContainerOptions, RemoveContainerOptions, LogsOptions};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use futures::StreamExt;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

#[allow(unused)]
#[derive(Debug, Clone)]
pub enum LifecycleEvent {
    Started(String),
    DockerConnected,
    PullingImage(String, String),
    ImagePulled(String, String),
    CreatingContainer(String),
    ContainerCreated(String, String),
    RunningInstallScript(String),
    InstallScriptComplete(String, i32),
    SettingUpEntrypoint(String),
    Ready(String),
    Error(String, String),
    ReinstallStarted(String),
    RemovingOldContainer(String),
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
        
        let config = AppConfig::load("config.json")
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { 
                format!("Failed to load config: {}", e).into() 
            })?;
        let base_path = PathBuf::from(&config.storage.base_path);
        
        tracing::info!("Lifecycle manager initialized");

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

    /// Ensure Lightd network exists
    pub async fn ensure_network(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Self::ensure_network_static(&self.docker).await
    }

    /// Ensure Lightd network exists (static version for use in spawned tasks)
    async fn ensure_network_static(docker: &Docker) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        use bollard::network::{CreateNetworkOptions, InspectNetworkOptions};
        use std::collections::HashMap;
        
        const NETWORK_NAME: &str = "lightd_network";
        
        // Check if network exists
        match docker.inspect_network(NETWORK_NAME, None::<InspectNetworkOptions<String>>).await {
            Ok(network) => {
                if let Some(id) = network.id {
                    tracing::debug!("Lightd network exists: {}", id);
                    return Ok(id);
                }
            }
            Err(e) => {
                if !e.to_string().contains("404") && !e.to_string().contains("not found") {
                    return Err(e.into());
                }
            }
        }
        
        // Create network
        tracing::info!("Creating Lightd network");
        
        let mut labels = HashMap::new();
        labels.insert("managed-by", "lightd");
        
        let config = CreateNetworkOptions {
            name: NETWORK_NAME,
            check_duplicate: true,
            driver: "bridge",
            internal: false,
            attachable: true,
            ingress: false,
            enable_ipv6: false,
            labels,
            ..Default::default()
        };
        
        let response = docker.create_network(config).await?;
        
        let network_id = response.id.ok_or("Network created but no ID returned")?;
        tracing::info!("Created Lightd network: {}", network_id);
        
        Ok(network_id)
    }

    /// Verify Docker daemon is running and accessible
    pub async fn check_docker(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ping_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            self.docker.ping()
        ).await;
        
        match ping_result {
            Ok(Ok(_)) => {
                let _ = self.event_tx.send(LifecycleEvent::DockerConnected);
                tracing::info!("Docker daemon accessible");
                Ok(())
            }
            Ok(Err(e)) => {
                let error_msg = format!("Docker daemon not accessible: {}", e);
                tracing::error!("{}", error_msg);
                Err(error_msg.into())
            }
            Err(_) => {
                let error_msg = "Docker ping timeout after 5 seconds";
                tracing::error!("{}", error_msg);
                Err(error_msg.into())
            }
        }
    }

    /// Ensure Docker image is available, pull if necessary
    async fn ensure_image_available(
        docker: &Docker,
        image: &str,
        internal_id: &str,
        event_tx: &mpsc::UnboundedSender<LifecycleEvent>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use bollard::image::CreateImageOptions;
        
        // Check if image exists
        match docker.inspect_image(image).await {
            Ok(_) => {
                tracing::debug!("Image {} already available", image);
                return Ok(());
            }
            Err(e) => {
                if !e.to_string().contains("404") && !e.to_string().contains("No such image") {
                    return Err(e.into());
                }
            }
        }
        
        // Image not found, pull it
        let _ = event_tx.send(LifecycleEvent::PullingImage(
            internal_id.to_string(),
            image.to_string(),
        ));
        
        tracing::info!("Pulling image: {}", image);
        
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });
        
        let mut stream = docker.create_image(options, None, None);
        
        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(status) = info.status {
                        tracing::info!("[{}] Image pull: {}", internal_id, status);
                    }
                    if let Some(progress) = info.progress {
                        tracing::debug!("[{}] {}", internal_id, progress);
                    }
                }
                Err(e) => {
                    let error_msg = format!("Image pull failed: {}", e);
                    tracing::error!("{}", error_msg);
                    return Err(error_msg.into());
                }
            }
        }
        
        let _ = event_tx.send(LifecycleEvent::ImagePulled(
            internal_id.to_string(),
            image.to_string(),
        ));
        
        tracing::info!("Image {} pulled successfully", image);
        Ok(())
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

        // Use absolute paths directly (don't canonicalize - causes issues with disk images)
        let volume_path_str = volume_path.to_string_lossy().to_string();
        let container_data_path_str = container_data_path.to_string_lossy().to_string();

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

        // Ensure image is available
        if let Err(e) = Self::ensure_image_available(
            &docker,
            &image,
            &internal_id,
            &event_tx,
        ).await {
            return Err(format!("Failed to pull image: {}", e).into());
        }

        // Ensure Lightd network exists
        let network_id = Self::ensure_network_static(&docker).await?;

        // Create container config
        let mut host_config = HostConfig {
            mounts: Some(mounts.clone()),
            network_mode: Some("lightd_network".to_string()),
            ..Default::default()
        };

        // Apply limits
        if let Some(memory) = state.limits.memory {
            host_config.memory = Some(memory);
        }
        if let Some(cpu) = state.limits.cpu {
            host_config.nano_cpus = Some((cpu * 1_000_000_000.0) as i64);
        }

        // Apply port bindings
        let mut port_bindings = std::collections::HashMap::new();
        let mut exposed_ports = std::collections::HashMap::new();
        
        for port_binding in &state.ports {
            let container_port_key = format!("{}/{}", port_binding.container_port, port_binding.protocol);
            let host_binding = bollard::models::PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(port_binding.host_port.to_string()),
            };
            
            port_bindings.insert(container_port_key.clone(), Some(vec![host_binding]));
            exposed_ports.insert(container_port_key, std::collections::HashMap::new());
            
            tracing::info!("Binding container port {} to host port {} ({})", 
                port_binding.container_port, port_binding.host_port, port_binding.protocol);
        }
        
        if !port_bindings.is_empty() {
            host_config.port_bindings = Some(port_bindings);
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

        // For install phase, run as root. Will be recreated with lightd+ user after install
        let container_user_config = None;  // Always run as root

        let config = Config {
            image: Some(image.clone()),
            working_dir: Some("/home/container".to_string()),
            host_config: Some(host_config),
            entrypoint: Some(vec!["/bin/sh".to_string(), "/app/data/entrypoint.sh".to_string()]),
            user: container_user_config,
            tty: Some(true),
            open_stdin: Some(true),
            exposed_ports: if exposed_ports.is_empty() { None } else { Some(exposed_ports) },
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
        // This is prob universal, later we can 
        // check if this is proper
        if let Some(script) = install_script {
            let _ = event_tx.send(LifecycleEvent::RunningInstallScript(internal_id.clone()));

            // Write install script
            let install_path = container_data_path.join("install.sh");
            tokio::fs::write(&install_path, &script).await?;

            // Simple entrypoint that runs install
            // This is if install content is provided
            // :D
            let install_entrypoint = 
                "#!/bin/sh\ncd /home/container\n/bin/sh /app/data/install.sh\n";
            tokio::fs::write(&entrypoint_path, install_entrypoint).await?;

            // Start container for installation
            docker.start_container(&container_id, None::<StartContainerOptions<String>>).await?;

            // Allow logs to be streamed
            // Very effective
            let log_docker = docker.clone();
            let log_container_id = container_id.clone();
            let log_internal_id = internal_id.clone();
            
            tokio::spawn(async move {
                let mut logs = log_docker.logs(&log_container_id, Some(LogsOptions::<String> {
                    follow: true,
                    stdout: true,
                    stderr: true,
                    ..Default::default()
                }));
                
                while let Some(Ok(log)) = logs.next().await {
                    let line = format!("{}", log);
                    tracing::info!("[{}] {}", log_internal_id, line.trim());
                }
            });

            // Wait for container to stop (install complete)
            // Kinda weird we have a timeout time period, maybe we can change this to get from the config
            // TODO: MAKE it so that the timeout is taken from the config.json
            let timeout = tokio::time::Duration::from_secs(600);
            let start_time = std::time::Instant::now();
            let mut install_completed = false;
            
            loop {
                if start_time.elapsed() > timeout {
                    tracing::error!("Install timeout for {}", internal_id);
                    break;
                }

                match docker.inspect_container(&container_id, None).await {
                    Ok(info) => {
                        if let Some(state_info) = info.state {
                            if state_info.running == Some(false) {
                                let exit_code = state_info.exit_code.unwrap_or(-1);
                                install_completed = true;
                                tracing::info!("Install complete for {} (exit code: {})", internal_id, exit_code);
                                
                                let _ = event_tx.send(LifecycleEvent::InstallScriptComplete(
                                    internal_id.clone(),
                                    exit_code as i32,
                                ));
                                
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to inspect container: {}", e);
                        break;
                    }
                }

                // Let's wait, it's useful here
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }

            if !install_completed {
                tracing::error!("Install did not complete for {}", internal_id);
                let _ = docker.stop_container(&container_id, None).await;
            }

            // Don't remove the container - we'll reuse it for runtime
            // Just stop it so we can update the entrypoint
            docker.stop_container(&container_id, None).await?;
        }

        // Setup final entrypoint with startup command
        let _ = event_tx.send(LifecycleEvent::SettingUpEntrypoint(internal_id.clone()));

        let final_entrypoint = format!(
            "#!/bin/sh\ncd /home/container\nexec sh -c '{}'\n",
            state.startup_command.replace("'", "'\\''")
        );
        tokio::fs::write(&entrypoint_path, final_entrypoint).await?;

        // Mark as ready in database
        manager.mark_ready(&internal_id, container_id.clone()).await?;
        let _ = event_tx.send(LifecycleEvent::Ready(internal_id.clone()));

        // Start the container
        docker.start_container(&container_id, None::<StartContainerOptions<String>>).await?;
        tracing::info!("Started container {}", internal_id);

        // Verify container is running
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        match docker.inspect_container(&container_id, None).await {
            Ok(info) => {
                if let Some(state_info) = info.state {
                    if state_info.running == Some(true) {
                        tracing::info!("Container {} verified running", internal_id);
                        let _ = event_tx.send(LifecycleEvent::Started(internal_id.clone()));
                    } else {
                        tracing::error!("Container {} not running after start", internal_id);
                        let exit_code = state_info.exit_code.unwrap_or(-1);
                        tracing::error!("Container exited with code: {}", exit_code);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to verify container {}: {}", internal_id, e);
            }
        }

        tracing::info!("Container {} installation complete", internal_id);
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
    // Not used anymore
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
