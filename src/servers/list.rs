//! Server listing with pagination
//! 
//! CLI command to list all containers/servers with pagination.

use crate::config::config::Config;
use crate::container::manager::ContainerManager;
use crate::container::state::InstallState;

const PAGE_SIZE: usize = 5;

#[allow(unused_variables)]
pub async fn list_servers(page: i64) {
    // Load config
    let config = match Config::load("config.json") {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            return;
        }
    };
    
    // Initialize container manager
    let containers_db_path = format!("{}/containers.db", config.storage.base_path);
    let manager = match ContainerManager::new(&containers_db_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to initialize container manager: {}", e);
            return;
        }
    };
    
    // Get all containers
    let containers = match manager.list_containers().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to list containers: {}", e);
            return;
        }
    };
    
    let total_count = containers.len();
    let total_pages = (total_count + PAGE_SIZE - 1) / PAGE_SIZE;
    
    if total_count == 0 {
        println!("╔═══════════════════════════════════════════════════════════════════╗");
        println!("║                          No Servers Found                         ║");
        println!("╚═══════════════════════════════════════════════════════════════════╝");
        return;
    }
    
    // Calculate page bounds
    let page = if page < 1 { 1 } else { page as usize };
    let page = if page > total_pages { total_pages } else { page };
    
    let start_idx = (page - 1) * PAGE_SIZE;
    let end_idx = std::cmp::min(start_idx + PAGE_SIZE, total_count);
    
    // Print header
    println!();
    println!("╔═══════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                                    SERVER LIST                                        ║ n");
    println!("╠═══════════════════════════════════════════════════════════════════════════════════════╣");
    println!("║  Page {}/{} | Showing {}-{} of {} servers                                             ║",
        page, total_pages, start_idx + 1, end_idx, total_count);
    println!("╠═══════════════════════════════════════════════════════════════════════════════════════╣");
    
    // Print each server
    for (i, container) in containers.iter().skip(start_idx).take(PAGE_SIZE).enumerate() {
        let idx = start_idx + i + 1;
        
        // Determine status
        let status = match container.install_state {
            InstallState::Ready => "✓ Ready",
            InstallState::Installing => "⟳ Installing",
            InstallState::Failed => "✗ Failed",
        };
        
        let container_id_display = container.container_id
            .as_ref()
            .map(|id| if id.len() > 12 { &id[..12] } else { id })
            .unwrap_or("N/A");
        
        // Format creation time
        let created = format_timestamp(container.created_at);
        
        println!("║                                                                                           ║");
        println!("║  [{:>3}] Internal ID: {:<40}                      ║", idx, truncate(&container.internal_id, 40));
        println!("║        Docker ID:   {:<12}  Status: {:<15}                              ║", container_id_display, status);
        println!("║        Volume:      {:<40}                      ║", truncate(&container.volume_id, 40));
        println!("║        Created:     {:<25}                                              ║", created);
        
        // Show ports if any
        if !container.ports.is_empty() {
            let ports_str: Vec<String> = container.ports.iter()
                .take(3)
                .map(|p| format!("{}:{}/{}", p.ip, p.port, p.protocol))
                .collect();
            let ports_display = if container.ports.len() > 3 {
                format!("{} (+{})", ports_str.join(", "), container.ports.len() - 3)
            } else {
                ports_str.join(", ")
            };
            println!("║        Ports:       {:<60}     ║", truncate(&ports_display, 60));
        }
        
        // Show resource limits if set
        let mut limits = Vec::new();
        if let Some(mem) = container.limits.memory {
            limits.push(format!("Mem: {}MB", mem / 1024 / 1024));
        }
        if let Some(cpu) = container.limits.cpu {
            limits.push(format!("CPU: {:.1}", cpu));
        }
        if !limits.is_empty() {
            println!("║        Limits:      {:<60}     ║", limits.join(", "));
        }
        
        if i < PAGE_SIZE - 1 && start_idx + i + 1 < total_count {
            println!("║  ─────────────────────────────────────────────────────────────────────────────────────  ║");
        }
    }
    
    println!("║                                                                                           ║");
    println!("╠═══════════════════════════════════════════════════════════════════════════════════════════╣");
    println!("║  Navigation: lightd --servers <page>                                                       ║");
    if page > 1 {
        println!("║    Previous: lightd --servers {}                                                            ║", page - 1);
    }
    if page < total_pages {
        println!("║    Next:     lightd --servers {}                                                            ║", page + 1);
    }
    println!("╚═══════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();
}

/// Format a Unix timestamp to a readable date string
fn format_timestamp(ts: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    
    let datetime = UNIX_EPOCH + Duration::from_secs(ts);
    let now = std::time::SystemTime::now();
    
    let elapsed = now.duration_since(datetime).unwrap_or(Duration::ZERO);
    
    if elapsed.as_secs() < 60 {
        "Just now".to_string()
    } else if elapsed.as_secs() < 3600 {
        format!("{} minutes ago", elapsed.as_secs() / 60)
    } else if elapsed.as_secs() < 86400 {
        format!("{} hours ago", elapsed.as_secs() / 3600)
    } else {
        format!("{} days ago", elapsed.as_secs() / 86400)
    }
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}