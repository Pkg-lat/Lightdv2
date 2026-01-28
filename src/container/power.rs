use super::manager::ContainerManager;
use bollard::Docker;
use bollard::container::{StartContainerOptions, KillContainerOptions, RestartContainerOptions};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum PowerAction {
    Start,
    Kill,
    Restart,
}

#[derive(Debug, Clone)]
pub enum PowerEvent {
    Starting(String),
    Started(String),
    Killing(String),
    Killed(String),
    Restarting(String),
    Restarted(String),
    Error(String, String),
}

pub struct PowerManager {
    manager: Arc<ContainerManager>,
    docker: Docker,
    event_tx: mpsc::UnboundedSender<PowerEvent>,
}

impl PowerManager {
    pub fn new(
        manager: Arc<ContainerManager>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<PowerEvent>), Box<dyn std::error::Error + Send + Sync>> {
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

    pub async fn execute_action(
        &self,
        internal_id: String,
        action: PowerAction,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = self.manager.clone();
        let docker = self.docker.clone();
        let event_tx = self.event_tx.clone();

        // Spawn async non-blocking job
        tokio::spawn(async move {
            if let Err(e) = Self::execute_power_action(
                manager,
                docker,
                event_tx.clone(),
                internal_id.clone(),
                action,
            )
            .await
            {
                let _ = event_tx.send(PowerEvent::Error(internal_id.clone(), e.to_string()));
                tracing::error!("Power action failed for {}: {}", internal_id, e);
            }
        });

        Ok(())
    }

    async fn execute_power_action(
        manager: Arc<ContainerManager>,
        docker: Docker,
        event_tx: mpsc::UnboundedSender<PowerEvent>,
        internal_id: String,
        action: PowerAction,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get container state
        let state = match manager.get_container(&internal_id).await {
            Ok(Some(state)) => state,
            Ok(None) => return Err("Container not found".into()),
            Err(e) => return Err(format!("Failed to get container: {}", e).into()),
        };

        let container_id = state
            .container_id
            .ok_or("Pending")?;

        match action {
            PowerAction::Start => {
                let _ = event_tx.send(PowerEvent::Starting(internal_id.clone()));
                tracing::info!("Starting container: {}", internal_id);

                docker
                    .start_container(&container_id, None::<StartContainerOptions<String>>)
                    .await?;

                let _ = event_tx.send(PowerEvent::Started(internal_id.clone()));
                tracing::info!("Container started: {}", internal_id);
            }
            PowerAction::Kill => {
                let _ = event_tx.send(PowerEvent::Killing(internal_id.clone()));
                tracing::info!("Killing container: {}", internal_id);

                // Instant kill with SIGKILL
                docker
                    .kill_container(
                        &container_id,
                        Some(KillContainerOptions { signal: "SIGKILL" }),
                    )
                    .await?;

                let _ = event_tx.send(PowerEvent::Killed(internal_id.clone()));
                tracing::info!("Container killed: {}", internal_id);
            }
            PowerAction::Restart => {
                let _ = event_tx.send(PowerEvent::Restarting(internal_id.clone()));
                tracing::info!("Restarting container: {}", internal_id);

                docker
                    .restart_container(&container_id, None::<RestartContainerOptions>)
                    .await?;

                let _ = event_tx.send(PowerEvent::Restarted(internal_id.clone()));
                tracing::info!("Container restarted: {}", internal_id);
            }
        }

        Ok(())
    }
}

