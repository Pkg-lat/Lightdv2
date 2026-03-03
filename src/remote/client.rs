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
    #[serde(rename = "billing")]
    Billing {
        server: String,
        memory_gb: f64,
        cpu_vcpus: f64,
        storage_gb: f64,
        egress_gb: f64,
        duration_hours: f64,
        estimated_cost: f64,
        timestamp: u64,
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
    
    /// Send billing usage update to remote
    pub async fn send_billing_update(
        &self,
        internal_id: &str,
        memory_gb: f64,
        cpu_vcpus: f64,
        storage_gb: f64,
        egress_gb: f64,
        duration_hours: f64,
        estimated_cost: f64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let event = RemoteEvent::Billing {
            server: internal_id.to_string(),
            memory_gb,
            cpu_vcpus,
            storage_gb,
            egress_gb,
            duration_hours,
            estimated_cost,
            timestamp,
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
    #[allow(dead_code)]
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
    pub fn start_health_check(&self) {
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
    
    /// Send billing usage update (non-blocking)
    pub fn notify_billing(
        &self,
        internal_id: String,
        memory_gb: f64,
        cpu_vcpus: f64,
        storage_gb: f64,
        egress_gb: f64,
        duration_hours: f64,
        estimated_cost: f64,
    ) {
        let client = self.client.clone();
        
        tokio::spawn(async move {
            if let Err(e) = client.send_billing_update(
                &internal_id,
                memory_gb,
                cpu_vcpus,
                storage_gb,
                egress_gb,
                duration_hours,
                estimated_cost,
            ).await {
                tracing::error!("Remote: {}", e);
            }
        });
    }
    
    #[allow(unused)]
    /// Get client for direct access
    pub fn client(&self) -> Arc<RemoteClient> {
        self.client.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;
    use tokio::time::{timeout, Duration};

    #[derive(Debug)]
    struct CapturedRequest {
        path: String,
        authorization: String,
        body: String,
    }

    async fn read_http_request(stream: &mut TcpStream) -> Result<CapturedRequest, Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = Vec::new();
        let mut headers_end = None;

        while headers_end.is_none() {
            let mut chunk = [0u8; 1024];
            let bytes_read = stream.read(&mut chunk).await?;
            if bytes_read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..bytes_read]);

            if let Some(index) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                headers_end = Some(index + 4);
            }
        }

        let headers_end = headers_end.ok_or("invalid HTTP request")?;
        let header_text = String::from_utf8(buffer[..headers_end].to_vec())?;

        let mut lines = header_text.lines();
        let request_line = lines.next().ok_or("missing request line")?;
        let path = request_line
            .split_whitespace()
            .nth(1)
            .ok_or("missing request path")?
            .to_string();

        let mut content_length = 0usize;
        let mut authorization = String::new();

        for line in lines {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("content-length:") {
                let value = line.split(':').nth(1).ok_or("invalid content-length")?.trim();
                content_length = value.parse()?;
            } else if lower.starts_with("authorization:") {
                authorization = line.split(':').nth(1).unwrap_or("").trim().to_string();
            }
        }

        let mut body = buffer[headers_end..].to_vec();
        while body.len() < content_length {
            let mut chunk = vec![0u8; content_length - body.len()];
            let bytes_read = stream.read(&mut chunk).await?;
            if bytes_read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..bytes_read]);
        }

        Ok(CapturedRequest {
            path,
            authorization,
            body: String::from_utf8(body)?,
        })
    }

    #[tokio::test]
    async fn remote_sync_manager_sends_callbacks() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let (tx, mut rx) = mpsc::channel::<CapturedRequest>(8);

        tokio::spawn(async move {
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let req = read_http_request(&mut socket).await.expect("read request");
                tx.send(req).await.expect("send request to test channel");

                let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
                socket.write_all(response).await.expect("write response");
            }
        });

        let manager = RemoteSyncManager::new(format!("http://{}", addr), "test-token".to_string());

        manager.notify_status("srv-1".to_string(), "ready".to_string());
        manager.notify_error(
            "srv-2".to_string(),
            "failed".to_string(),
            Some("boom".to_string()),
        );
        manager.notify_billing(
            "srv-3".to_string(),
            2.0,
            1.0,
            10.0,
            0.5,
            1.0,
            0.42,
        );

        let mut bodies = Vec::new();
        for _ in 0..3 {
            let request = timeout(Duration::from_secs(3), rx.recv())
                .await
                .expect("request timeout")
                .expect("request missing");

            assert_eq!(request.path, "/update");
            assert_eq!(request.authorization, "Bearer test-token");
            bodies.push(request.body);
        }

        let parsed: Vec<Value> = bodies
            .iter()
            .map(|body| serde_json::from_str(body).expect("valid JSON body"))
            .collect();

        assert!(parsed.iter().any(|event| {
            event.get("event").and_then(Value::as_str) == Some("update")
                && event.get("server").and_then(Value::as_str) == Some("srv-1")
                && event.get("status").and_then(Value::as_str) == Some("ready")
        }));

        assert!(parsed.iter().any(|event| {
            event.get("event").and_then(Value::as_str) == Some("update")
                && event.get("server").and_then(Value::as_str) == Some("srv-2")
                && event.get("error").and_then(Value::as_str) == Some("failed")
                && event.get("data").and_then(Value::as_str) == Some("boom")
        }));

        assert!(parsed.iter().any(|event| {
            event.get("event").and_then(Value::as_str) == Some("billing")
                && event.get("server").and_then(Value::as_str) == Some("srv-3")
        }));
    }
}
