//! Resource usage tracking for billing
//! 
//! Monitors container resource usage in real-time with async processing
//! Scales efficiently regardless of container count

use bollard::Docker;
use bollard::container::StatsOptions;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use futures::StreamExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingRates {
    pub memory_per_gb_hour: f64,
    pub cpu_per_vcpu_hour: f64,
    pub storage_per_gb_hour: f64,
    pub egress_per_gb: f64,
}

impl Default for BillingRates {
    fn default() -> Self {
        Self {
            memory_per_gb_hour: 0.01,
            cpu_per_vcpu_hour: 0.02,
            storage_per_gb_hour: 0.0001,
            egress_per_gb: 0.05,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub container_id: String,
    pub memory_bytes: u64,
    pub cpu_usage_seconds: f64,
    pub network_egress_bytes: u64,
    pub storage_bytes: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub memory_gb: f64,
    pub cpu_vcpus: f64,
    pub storage_gb: f64,
    pub egress_gb: f64,
    pub duration_hours: f64,
}

pub struct BillingTracker {
    docker: Docker,
    rates: Arc<RwLock<BillingRates>>,
    usage_data: Arc<RwLock<HashMap<String, Vec<ResourceUsage>>>>,
    interval_ms: u64,
}

impl BillingTracker {
    pub fn new(rates: BillingRates, interval_ms: u64) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let docker = Docker::connect_with_local_defaults()?;
        
        Ok(Self {
            docker,
            rates: Arc::new(RwLock::new(rates)),
            usage_data: Arc::new(RwLock::new(HashMap::new())),
            interval_ms,
        })
    }
    
    /// Start monitoring all containers
    pub async fn start_monitoring(self: Arc<Self>) {
        let tracker = self.clone();
        
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_millis(tracker.interval_ms));
            
            loop {
                tick.tick().await;
                
                if let Err(e) = tracker.collect_metrics().await {
                    tracing::error!("Failed to collect metrics: {}", e);
                }
            }
        });
        
        tracing::info!("Billing tracker started with {}ms interval", self.interval_ms);
    }
    
    /// Collect metrics from all running containers
    async fn collect_metrics(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // List all containers
        let containers = self.docker.list_containers::<String>(None).await?;
        
        for container in containers {
            if let Some(id) = container.id {
                // Only monitor lightd containers
                if let Some(names) = container.names {
                    if names.iter().any(|n| n.contains("lightd-")) {
                        if let Err(e) = self.collect_container_metrics(&id).await {
                            tracing::warn!("Failed to collect metrics for {}: {}", id, e);
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Collect metrics for a specific container
    async fn collect_container_metrics(&self, container_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut stats_stream = self.docker.stats(
            container_id,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );
        
        if let Some(Ok(stats)) = stats_stream.next().await {
            let memory_bytes = stats.memory_stats.usage.unwrap_or(0);
            
            // Calculate CPU usage
            let cpu_delta = stats.cpu_stats.cpu_usage.total_usage as f64
                - stats.precpu_stats.cpu_usage.total_usage as f64;
            let system_delta = stats.cpu_stats.system_cpu_usage.unwrap_or(0) as f64
                - stats.precpu_stats.system_cpu_usage.unwrap_or(0) as f64;
            
            let cpu_usage_seconds = if system_delta > 0.0 {
                (cpu_delta / system_delta) * stats.cpu_stats.online_cpus.unwrap_or(1) as f64
            } else {
                0.0
            };
            
            // Network egress
            let mut egress_bytes = 0u64;
            if let Some(networks) = stats.networks {
                for (_, network) in networks {
                    egress_bytes += network.tx_bytes;
                }
            }
            
            // Storage (from blkio)
            let mut storage_bytes = 0u64;
            if let Some(blkio_stats) = stats.blkio_stats.io_service_bytes_recursive {
                for entry in blkio_stats {
                    storage_bytes += entry.value;
                }
            }
            
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            
            let usage = ResourceUsage {
                container_id: container_id.to_string(),
                memory_bytes,
                cpu_usage_seconds,
                network_egress_bytes: egress_bytes,
                storage_bytes,
                timestamp,
            };
            
            // Store usage data
            let mut data = self.usage_data.write().await;
            data.entry(container_id.to_string())
                .or_insert_with(Vec::new)
                .push(usage);
            
            // Keep only last 24 hours of data
            let cutoff = timestamp - (24 * 3600);
            if let Some(entries) = data.get_mut(container_id) {
                entries.retain(|u| u.timestamp > cutoff);
            }
        }
        
        Ok(())
    }
    
    /// Get usage snapshot for a container over a time period
    pub async fn get_usage_snapshot(
        &self,
        container_id: &str,
        duration_hours: f64,
    ) -> Result<UsageSnapshot, Box<dyn std::error::Error + Send + Sync>> {
        let data = self.usage_data.read().await;
        
        let entries = data.get(container_id)
            .ok_or("No usage data for container")?;
        
        if entries.is_empty() {
            return Err("No usage data available".into());
        }
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        
        let cutoff = now - (duration_hours * 3600.0) as u64;
        
        let relevant_entries: Vec<_> = entries.iter()
            .filter(|e| e.timestamp > cutoff)
            .collect();
        
        if relevant_entries.is_empty() {
            return Err("No usage data in specified time range".into());
        }
        
        // Calculate averages
        let avg_memory = relevant_entries.iter()
            .map(|e| e.memory_bytes as f64)
            .sum::<f64>() / relevant_entries.len() as f64;
        
        let avg_cpu = relevant_entries.iter()
            .map(|e| e.cpu_usage_seconds)
            .sum::<f64>() / relevant_entries.len() as f64;
        
        let avg_storage = relevant_entries.iter()
            .map(|e| e.storage_bytes as f64)
            .sum::<f64>() / relevant_entries.len() as f64;
        
        // Total egress (cumulative)
        let total_egress = relevant_entries.last()
            .map(|e| e.network_egress_bytes)
            .unwrap_or(0) as f64;
        
        Ok(UsageSnapshot {
            memory_gb: avg_memory / (1024.0 * 1024.0 * 1024.0),
            cpu_vcpus: avg_cpu,
            storage_gb: avg_storage / (1024.0 * 1024.0 * 1024.0),
            egress_gb: total_egress / (1024.0 * 1024.0 * 1024.0),
            duration_hours,
        })
    }
    
    /// Calculate cost for a usage snapshot
    pub async fn calculate_cost(&self, snapshot: &UsageSnapshot) -> f64 {
        let rates = self.rates.read().await;
        
        let memory_cost = snapshot.memory_gb * snapshot.duration_hours * rates.memory_per_gb_hour;
        let cpu_cost = snapshot.cpu_vcpus * snapshot.duration_hours * rates.cpu_per_vcpu_hour;
        let storage_cost = snapshot.storage_gb * snapshot.duration_hours * rates.storage_per_gb_hour;
        let egress_cost = snapshot.egress_gb * rates.egress_per_gb;
        
        memory_cost + cpu_cost + storage_cost + egress_cost
    }
    
    /// Get current billing rates
    pub async fn get_rates(&self) -> BillingRates {
        self.rates.read().await.clone()
    }
    
    /// Update billing rates
    #[allow(dead_code)]
    pub async fn update_rates(&self, rates: BillingRates) {
        let mut current_rates = self.rates.write().await;
        *current_rates = rates;
        tracing::info!("Updated billing rates");
    }
    
    /// Get all tracked containers
    pub async fn get_tracked_containers(&self) -> Vec<String> {
        let data = self.usage_data.read().await;
        data.keys().cloned().collect()
    }
    
    /// Clear usage data for a container
    #[allow(dead_code)]
    pub async fn clear_container_data(&self, container_id: &str) {
        let mut data = self.usage_data.write().await;
        data.remove(container_id);
        tracing::info!("Cleared billing data for container: {}", container_id);
    }
}
