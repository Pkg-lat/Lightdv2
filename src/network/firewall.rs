//! Container firewall system using Docker bridge networks and iptables
//! 
//! Provides DDoS protection and security rules isolated from host network

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;
use sled::Db;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FirewallAction {
    Accept,
    Drop,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallRule {
    pub id: String,
    pub container_id: String,
    pub source_ip: Option<String>,
    pub source_port: Option<u16>,
    pub dest_port: Option<u16>,
    pub protocol: Protocol,
    pub action: FirewallAction,
    pub rate_limit: Option<RateLimit>,
    pub description: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RateLimit {
    pub requests: u32,
    pub per_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DDoSProtection {
    pub enabled: bool,
    pub syn_flood_protection: bool,
    pub connection_limit: Option<u32>,
    pub rate_limit: Option<RateLimit>,
}

pub struct FirewallManager {
    db: Arc<Db>,
    rules: Arc<RwLock<Vec<FirewallRule>>>,
}

impl FirewallManager {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = sled::open(db_path)?;
        let rules = Arc::new(RwLock::new(Vec::new()));
        
        // Load existing rules from database
        let mut loaded_rules = Vec::new();
        for item in db.iter() {
            let (key, value) = item?;
            let key_str = String::from_utf8(key.to_vec())?;
            if key_str.starts_with("rule:") {
                let rule: FirewallRule = serde_json::from_slice(&value)?;
                loaded_rules.push(rule);
            }
        }
        
        if !loaded_rules.is_empty() {
            tracing::info!("Loaded {} firewall rules from database", loaded_rules.len());
            *rules.blocking_write() = loaded_rules;
        }
        
        Ok(Self {
            db: Arc::new(db),
            rules,
        })
    }
    
    /// Create a custom Docker bridge network for a container
    pub async fn create_container_network(
        &self,
        container_id: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let network_name = format!("lightd-net-{}", container_id);
        
        // Check if network already exists
        let check = Command::new("docker")
            .args(&["network", "inspect", &network_name])
            .output();
        
        if check.is_ok() && check.unwrap().status.success() {
            tracing::info!("Network {} already exists", network_name);
            return Ok(network_name);
        }
        
        // Create isolated bridge network
        let output = Command::new("docker")
            .args(&[
                "network", "create",
                "--driver", "bridge",
                "--internal=false",
                "--opt", "com.docker.network.bridge.name=lightd0",
                &network_name,
            ])
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to create network: {}", error).into());
        }
        
        tracing::info!("Created isolated network: {}", network_name);
        Ok(network_name)
    }
    
    /// Remove container network
    pub async fn remove_container_network(
        &self,
        container_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let network_name = format!("lightd-net-{}", container_id);
        
        let output = Command::new("docker")
            .args(&["network", "rm", &network_name])
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            // Don't fail if network doesn't exist
            if !error.contains("not found") {
                return Err(format!("Failed to remove network: {}", error).into());
            }
        }
        
        tracing::info!("Removed network: {}", network_name);
        Ok(())
    }
    
    /// Add a firewall rule
    pub async fn add_rule(
        &self,
        rule: FirewallRule,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Validate rule
        self.validate_rule(&rule)?;
        
        // Apply iptables rule if enabled
        if rule.enabled {
            self.apply_iptables_rule(&rule, true).await?;
        }
        
        // Store in database
        let key = format!("rule:{}", rule.id);
        let value = serde_json::to_vec(&rule)?;
        self.db.insert(key.as_bytes(), value)?;
        
        // Add to in-memory cache
        let mut rules = self.rules.write().await;
        rules.push(rule.clone());
        
        tracing::info!("Added firewall rule: {}", rule.id);
        Ok(())
    }
    
    /// Remove a firewall rule
    pub async fn remove_rule(
        &self,
        rule_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get rule from cache
        let mut rules = self.rules.write().await;
        let rule = rules.iter().find(|r| r.id == rule_id)
            .ok_or("Rule not found")?
            .clone();
        
        // Remove iptables rule if it was enabled
        if rule.enabled {
            self.apply_iptables_rule(&rule, false).await?;
        }
        
        // Remove from database
        let key = format!("rule:{}", rule_id);
        self.db.remove(key.as_bytes())?;
        
        // Remove from cache
        rules.retain(|r| r.id != rule_id);
        
        tracing::info!("Removed firewall rule: {}", rule_id);
        Ok(())
    }
    
    /// Enable/disable a rule
    pub async fn toggle_rule(
        &self,
        rule_id: &str,
        enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut rules = self.rules.write().await;
        let rule = rules.iter_mut().find(|r| r.id == rule_id)
            .ok_or("Rule not found")?;
        
        if rule.enabled == enabled {
            return Ok(());
        }
        
        // Apply or remove iptables rule
        self.apply_iptables_rule(rule, enabled).await?;
        
        rule.enabled = enabled;
        
        // Update database
        let key = format!("rule:{}", rule_id);
        let value = serde_json::to_vec(&*rule)?;
        self.db.insert(key.as_bytes(), value)?;
        
        tracing::info!("Toggled firewall rule {}: {}", rule_id, enabled);
        Ok(())
    }
    
    /// Get all rules for a container
    pub async fn get_container_rules(
        &self,
        container_id: &str,
    ) -> Vec<FirewallRule> {
        let rules = self.rules.read().await;
        rules.iter()
            .filter(|r| r.container_id == container_id)
            .cloned()
            .collect()
    }
    
    /// Enable DDoS protection for a container
    pub async fn enable_ddos_protection(
        &self,
        container_id: &str,
        protection: DDoSProtection,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let network_name = format!("lightd-net-{}", container_id);
        
        if !protection.enabled {
            return Ok(());
        }
        
        // SYN flood protection
        if protection.syn_flood_protection {
            self.apply_syn_flood_protection(&network_name).await?;
        }
        
        // Connection limit
        if let Some(limit) = protection.connection_limit {
            self.apply_connection_limit(&network_name, limit).await?;
        }
        
        // Rate limiting
        if let Some(ref rate) = protection.rate_limit {
            self.apply_rate_limit(&network_name, rate).await?;
        }
        
        // Store DDoS config in database
        let key = format!("ddos:{}", container_id);
        let value = serde_json::to_vec(&protection)?;
        self.db.insert(key.as_bytes(), value)?;
        
        tracing::info!("Enabled DDoS protection for container: {}", container_id);
        Ok(())
    }
    
    /// Apply iptables rule
    async fn apply_iptables_rule(
        &self,
        rule: &FirewallRule,
        add: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let chain = format!("LIGHTD-{}", rule.container_id.to_uppercase());
        let action_flag = if add { "-A" } else { "-D" };
        
        // Ensure chain exists
        if add {
            let _ = Command::new("iptables")
                .args(&["-N", &chain])
                .output();
        }
        
        let mut args = vec![action_flag.to_string(), chain];
        
        // Protocol
        if rule.protocol != Protocol::All {
            args.push("-p".to_string());
            args.push(format!("{:?}", rule.protocol).to_lowercase());
        }
        
        // Source IP
        if let Some(ref ip) = rule.source_ip {
            args.push("-s".to_string());
            args.push(ip.clone());
        }
        
        // Source port
        if let Some(port) = rule.source_port {
            args.push("--sport".to_string());
            args.push(port.to_string());
        }
        
        // Destination port
        if let Some(port) = rule.dest_port {
            args.push("--dport".to_string());
            args.push(port.to_string());
        }
        
        // Rate limiting
        if let Some(ref rate) = rule.rate_limit {
            args.push("-m".to_string());
            args.push("limit".to_string());
            args.push("--limit".to_string());
            args.push(format!("{}/{}", rate.requests, rate.per_seconds));
        }
        
        // Action
        args.push("-j".to_string());
        args.push(format!("{:?}", rule.action).to_uppercase());
        
        let output = Command::new("iptables")
            .args(&args)
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to apply iptables rule: {}", error).into());
        }
        
        tracing::debug!("Applied iptables rule: {:?}", args);
        Ok(())
    }
    
    /// Apply SYN flood protection
    async fn apply_syn_flood_protection(
        &self,
        network_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let chain = format!("LIGHTD-SYN-{}", network_name);
        
        // Create chain
        let _ = Command::new("iptables")
            .args(&["-N", &chain])
            .output();
        
        // Limit SYN packets
        let output = Command::new("iptables")
            .args(&[
                "-A", &chain,
                "-p", "tcp",
                "--syn",
                "-m", "limit",
                "--limit", "10/s",
                "--limit-burst", "20",
                "-j", "ACCEPT",
            ])
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to apply SYN flood protection: {}", error).into());
        }
        
        // Drop excessive SYN packets
        Command::new("iptables")
            .args(&["-A", &chain, "-p", "tcp", "--syn", "-j", "DROP"])
            .output()?;
        
        tracing::info!("Applied SYN flood protection for {}", network_name);
        Ok(())
    }
    
    /// Apply connection limit
    async fn apply_connection_limit(
        &self,
        network_name: &str,
        limit: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let chain = format!("LIGHTD-CONN-{}", network_name);
        
        // Create chain
        let _ = Command::new("iptables")
            .args(&["-N", &chain])
            .output();
        
        let output = Command::new("iptables")
            .args(&[
                "-A", &chain,
                "-p", "tcp",
                "-m", "connlimit",
                "--connlimit-above", &limit.to_string(),
                "-j", "REJECT",
                "--reject-with", "tcp-reset",
            ])
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to apply connection limit: {}", error).into());
        }
        
        tracing::info!("Applied connection limit {} for {}", limit, network_name);
        Ok(())
    }
    
    /// Apply rate limit
    async fn apply_rate_limit(
        &self,
        network_name: &str,
        rate: &RateLimit,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let chain = format!("LIGHTD-RATE-{}", network_name);
        
        // Create chain
        let _ = Command::new("iptables")
            .args(&["-N", &chain])
            .output();
        
        let output = Command::new("iptables")
            .args(&[
                "-A", &chain,
                "-m", "limit",
                "--limit", &format!("{}/{}", rate.requests, rate.per_seconds),
                "-j", "ACCEPT",
            ])
            .output()?;
        
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to apply rate limit: {}", error).into());
        }
        
        // Drop packets exceeding rate
        Command::new("iptables")
            .args(&["-A", &chain, "-j", "DROP"])
            .output()?;
        
        tracing::info!("Applied rate limit for {}", network_name);
        Ok(())
    }
    
    /// Validate firewall rule
    fn validate_rule(&self, rule: &FirewallRule) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Validate container_id is not empty
        if rule.container_id.is_empty() {
            return Err("Container ID cannot be empty".into());
        }
        
        // Validate ports are in valid range
        if let Some(port) = rule.source_port {
            if port == 0 {
                return Err("Invalid source port".into());
            }
        }
        
        if let Some(port) = rule.dest_port {
            if port == 0 {
                return Err("Invalid destination port".into());
            }
        }
        
        // Validate rate limit
        if let Some(ref rate) = rule.rate_limit {
            if rate.requests == 0 || rate.per_seconds == 0 {
                return Err("Invalid rate limit values".into());
            }
        }
        
        Ok(())
    }
    
    /// Clean up all rules for a container
    pub async fn cleanup_container_rules(
        &self,
        container_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let rules = self.get_container_rules(container_id).await;
        
        for rule in rules {
            self.remove_rule(&rule.id).await?;
        }
        
        // Remove DDoS config
        let key = format!("ddos:{}", container_id);
        self.db.remove(key.as_bytes())?;
        
        // Remove network
        self.remove_container_network(container_id).await?;
        
        // Remove iptables chains
        let chain = format!("LIGHTD-{}", container_id.to_uppercase());
        let _ = Command::new("iptables")
            .args(&["-F", &chain])
            .output();
        let _ = Command::new("iptables")
            .args(&["-X", &chain])
            .output();
        
        tracing::info!("Cleaned up firewall rules for container: {}", container_id);
        Ok(())
    }
}
