//! Remote API client for syncing with management server
//! 
//! Sends updates about container status, errors, and configuration to remote.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event")]
pub enum RemoteEvent {
    #[serde(rename = "update")]
    Update {
        server: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: u16,
    pub endpoint: String,
}

pub struct RemoteClient {
    url: String,
    token: String,
    client: reqwest::Client,
}

impl RemoteClient {
    pub fn new(url: String, token: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        
        Self {
            url,
            token,
            client,
        }
    }
    
    /// Check if remote is healthy and active
    pub async fn check_health(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let health_url = format!("{}/health", self.url);
        
        let response = self.client
            .get(&health_url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Ok(false);
        }
        
        let health: HealthResponse = response.json().await?;
        
        Ok(health.status == 200 && health.endpoint == "active")
    }
    
    /// Send status update to remote
    pub async fn send_status_update(
        &self,
        internal_id: &str,
        status: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let event = RemoteEvent::Update {
            server: internal_id.to_string(),
            status: Some(status.to_string()),
            error: None,
            data: None,
        };
        
        self.send_event(event).await
    }
    
    /// Send error update to remote
    pub async fn send_error_update(
        &self,
        internal_id: &str,
        error: &str,
        data: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let event = RemoteEvent::Update {
            server: internal_id.to_string(),
            status: None,
            error: Some(error.to_string()),
            data,
        };
        
        self.send_event(event).await
    }
    
    /// Send generic event to remote
    async fn send_event(
        &self,
        event: RemoteEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let update_url = format!("{}/update", self.url);
        
        let response = self.client
            .post(&update_url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(&event)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(format!("Remote returned status: {}", response.status()).into());
        }
        
        tracing::debug!("Sent event to remote: {:?}", event);
        Ok(())
    }
    
    /// Get config from remote
    #[allow(unused)]
    pub async fn get_config(&self) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let config_url = format!("{}/config", self.url);
        
        let response = self.client
            .get(&config_url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(format!("Remote returned status: {}", response.status()).into());
        }
        
        let config: serde_json::Value = response.json().await?;
        Ok(config)
    }
    
    /// Send current config to remote
    pub async fn send_config(
        &self,
        config: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let config_url = format!("{}/config", self.url);
        
        let response = self.client
            .post(&config_url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .json(config)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(format!("Remote returned status: {}", response.status()).into());
        }
        
        Ok(())
    }
}

/// Remote sync manager that handles background syncing
pub struct RemoteSyncManager {
    client: Arc<RemoteClient>,
}

impl RemoteSyncManager {
    pub fn new(url: String, token: String) -> Self {
        Self {
            client: Arc::new(RemoteClient::new(url, token)),
        }
    }
    
    /// Start health check loop
    pub async fn start_health_check(&self) {
        let client = self.client.clone();
        
        tokio::spawn(async move {
            loop {
                match client.check_health().await {
                    Ok(true) => {
                        tracing::debug!("Remote health check: OK");
                    }
                    Ok(false) => {
                        tracing::warn!("Remote health check: Failed");
                    }
                    Err(e) => {
                        tracing::error!("Remote health check error: {}", e);
                    }
                }
                
                // Check every 30 seconds
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
    }
    
    /// Send status update (non-blocking)
    pub fn notify_status(&self, internal_id: String, status: String) {
        let client = self.client.clone();
        
        tokio::spawn(async move {
            if let Err(e) = client.send_status_update(&internal_id, &status).await {
                tracing::error!("Failed to send status update to remote: {}", e);
            }
        });
    }
    
    /// Send error update (non-blocking)
    pub fn notify_error(&self, internal_id: String, error: String, data: Option<String>) {
        let client = self.client.clone();
        
        tokio::spawn(async move {
            if let Err(e) = client.send_error_update(&internal_id, &error, data).await {
                tracing::error!("Failed to send error update to remote: {}", e);
            }
        });
    }
    
    #[allow(unused)]
    /// Get client for direct access
    pub fn client(&self) -> Arc<RemoteClient> {
        self.client.clone()
    }
}
