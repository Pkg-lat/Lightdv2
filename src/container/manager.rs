use super::state::{ContainerState, InstallState};
use sled::Db;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ContainerManager {
    db: Arc<Db>,
    states: Arc<RwLock<()>>, // Mutex for state updates
}

impl ContainerManager {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = sled::open(db_path)?;
        Ok(Self {
            db: Arc::new(db),
            states: Arc::new(RwLock::new(())),
        })
    }

    pub async fn create_container(
        &self,
        internal_id: String,
        volume_id: String,
        startup_command: String,
    ) -> Result<ContainerState, Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        let mut state = ContainerState::new(internal_id.clone(), volume_id, startup_command);
        state.is_installing = true;
        state.install_state = InstallState::Installing;

        let serialized = serde_json::to_vec(&state)?;
        self.db.insert(internal_id.as_bytes(), serialized)?;

        tracing::info!("Created container state for internal_id: {}", internal_id);
        Ok(state)
    }

    pub async fn get_container(
        &self,
        internal_id: &str,
    ) -> Result<Option<ContainerState>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(data) = self.db.get(internal_id.as_bytes())? {
            let state: ContainerState = serde_json::from_slice(&data)?;
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }

    pub async fn update_container(
        &self,
        mut state: ContainerState,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        state.update_timestamp();
        let serialized = serde_json::to_vec(&state)?;
        self.db.insert(state.internal_id.as_bytes(), serialized)?;

        tracing::info!("Updated container state for internal_id: {}", state.internal_id);
        Ok(())
    }

    pub async fn mark_ready(
        &self,
        internal_id: &str,
        container_id: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        if let Some(mut state) = self.get_container(internal_id).await? {
            state.is_installing = false;
            state.install_state = InstallState::Ready;
            state.container_id = Some(container_id);
            state.update_timestamp();

            let serialized = serde_json::to_vec(&state)?;
            self.db.insert(internal_id.as_bytes(), serialized)?;

            tracing::info!("Marked container {} as ready", internal_id);
            Ok(())
        } else {
            Err("Container not found".into())
        }
    }

    /// Mark a container as failed during installation
    pub async fn mark_failed(
        &self,
        internal_id: &str,
        error_message: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        if let Some(mut state) = self.get_container(internal_id).await? {
            state.is_installing = false;
            state.install_state = InstallState::Failed;
            state.update_timestamp();

            let serialized = serde_json::to_vec(&state)?;
            self.db.insert(internal_id.as_bytes(), serialized)?;

            tracing::error!("Container {} installation failed: {}", internal_id, error_message);
            Ok(())
        } else {
            Err("Container not found".into())
        }
    }

    /// Mark a container as installing (for reinstall operations)
    pub async fn mark_installing(
        &self,
        internal_id: &str,
    ) -> Result<ContainerState, Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        if let Some(mut state) = self.get_container(internal_id).await? {
            state.is_installing = true;
            state.install_state = InstallState::Installing;
            state.container_id = None; // Clear old container ID
            state.update_timestamp();

            let serialized = serde_json::to_vec(&state)?;
            self.db.insert(internal_id.as_bytes(), serialized)?;

            tracing::info!("Marked container {} as installing", internal_id);
            Ok(state)
        } else {
            Err("Container not found".into())
        }
    }

    /// Validate container state and check for corruption
    /// Returns Ok(true) if container is healthy, Ok(false) if corrupted
    pub async fn validate_container(
        &self,
        internal_id: &str,
    ) -> Result<(bool, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
        match self.get_container(internal_id).await {
            Ok(Some(state)) => {
                let mut issues: Vec<String> = Vec::new();

                // Check for stuck installing state (older than 10 minutes)
                if state.is_installing {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    
                    if now - state.updated_at > 600 {
                        issues.push("Container stuck in installing state".to_string());
                    }
                }

                // Check for Ready state without container_id
                if state.install_state == InstallState::Ready && state.container_id.is_none() {
                    issues.push("Container marked ready but has no container ID".to_string());
                }

                // Check for empty required fields
                if state.internal_id.is_empty() {
                    issues.push("Container has empty internal_id".to_string());
                }
                if state.volume_id.is_empty() {
                    issues.push("Container has empty volume_id".to_string());
                }
                if state.startup_command.is_empty() {
                    issues.push("Container has empty startup_command".to_string());
                }

                if issues.is_empty() {
                    Ok((true, None))
                } else {
                    let issue_msg = issues.join("; ");
                    tracing::warn!("Container {} validation failed: {}", internal_id, issue_msg);
                    Ok((false, Some(issue_msg)))
                }
            }
            Ok(None) => {
                Err("Container not found".into())
            }
            Err(e) => {
                // DB corruption or parse error
                tracing::error!("Container {} data is corrupted: {}", internal_id, e);
                Ok((false, Some(format!("Data corruption: {}", e))))
            }
        }
    }

    /// Update startup command for a container
    pub async fn update_startup_command(
        &self,
        internal_id: &str,
        startup_command: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        if let Some(mut state) = self.get_container(internal_id).await? {
            state.startup_command = startup_command;
            state.update_timestamp();

            let serialized = serde_json::to_vec(&state)?;
            self.db.insert(internal_id.as_bytes(), serialized)?;

            tracing::info!("Updated startup command for container {}", internal_id);
            Ok(())
        } else {
            Err("Container not found".into())
        }
    }

    /// Update start pattern for a container (for detecting when server is fully started)
    pub async fn update_start_pattern(
        &self,
        internal_id: &str,
        start_pattern: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        if let Some(mut state) = self.get_container(internal_id).await? {
            state.start_pattern = start_pattern;
            state.update_timestamp();

            let serialized = serde_json::to_vec(&state)?;
            self.db.insert(internal_id.as_bytes(), serialized)?;

            tracing::info!("Updated start pattern for container {}", internal_id);
            Ok(())
        } else {
            Err("Container not found".into())
        }
    }

    pub async fn list_containers(&self) -> Result<Vec<ContainerState>, Box<dyn std::error::Error + Send + Sync>> {
        let mut containers = Vec::new();

        for item in self.db.iter() {
            let (_, value) = item?;
            let state: ContainerState = serde_json::from_slice(&value)?;
            containers.push(state);
        }

        Ok(containers)
    }

    pub async fn delete_container(
        &self,
        internal_id: &str,
    ) -> Result<ContainerState, Box<dyn std::error::Error + Send + Sync>> {
        let _lock = self.states.write().await;

        if let Some(state) = self.get_container(internal_id).await? {
            self.db.remove(internal_id.as_bytes())?;
            tracing::info!("Deleted container state for internal_id: {}", internal_id);
            Ok(state)
        } else {
            Err("Container not found".into())
        }
    }
}
