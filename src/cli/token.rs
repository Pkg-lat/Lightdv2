//! Token management CLI commands
//! 
//! Commands to view, reset, and set API tokens.

use crate::config::config::Config;

pub async fn handle_token_command(command: &str) {
    match command.to_lowercase().as_str() {
        "what" => show_current_token().await,
        "reset" => reset_token().await,
        "set" => set_custom_token().await,
        _ => show_help(),
    }
}

async fn show_current_token() {
    match Config::load("config.json") {
        Ok(config) => {
            println!("╔═══════════════════════════════════════════════════════════════════╗");
            println!("║                          Current API Token                        ║");
            println!("╠═══════════════════════════════════════════════════════════════════╣");
            println!("║  Token: {:<58} ║", config.authorization.token);
            println!("║  Enabled: {:<56} ║", if config.authorization.enabled { "Yes" } else { "No" });
            println!("╚═══════════════════════════════════════════════════════════════════╝");
        }
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
        }
    }
}

async fn reset_token() {
    let new_token = format!("lightd_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
    
    match Config::load("config.json") {
        Ok(mut config) => {
            config.authorization.token = new_token.clone();
            
            match serde_json::to_string_pretty(&config) {
                Ok(json) => {
                    match std::fs::write("config.json", json) {
                        Ok(_) => {
                            println!("╔═══════════════════════════════════════════════════════════════════╗");
                            println!("║                       Token Reset Successful                      ║");
                            println!("╠═══════════════════════════════════════════════════════════════════╣");
                            println!("║  New Token: {:<54} ║", new_token);
                            println!("╠═══════════════════════════════════════════════════════════════════╣");
                            println!("║  ⚠️  IMPORTANT: Update your applications with the new token!      ║");
                            println!("╚═══════════════════════════════════════════════════════════════════╝");
                        }
                        Err(e) => {
                            eprintln!("Failed to write config: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to serialize config: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
        }
    }
}

async fn set_custom_token() {
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                          Set Custom Token                         ║");
    println!("╠═══════════════════════════════════════════════════════════════════╣");
    println!("║  Enter your custom token (must start with 'lightd_'):            ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");
    
    use std::io::{self, Write};
    print!("Token: ");
    io::stdout().flush().unwrap();
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let custom_token = input.trim();
    
    if !custom_token.starts_with("lightd_") {
        eprintln!("Error: Token must start with 'lightd_'");
        return;
    }
    
    if custom_token.len() < 20 {
        eprintln!("Error: Token too short (minimum 20 characters)");
        return;
    }
    
    match Config::load("config.json") {
        Ok(mut config) => {
            config.authorization.token = custom_token.to_string();
            
            match serde_json::to_string_pretty(&config) {
                Ok(json) => {
                    match std::fs::write("config.json", json) {
                        Ok(_) => {
                            println!("✓ Token updated successfully!");
                        }
                        Err(e) => {
                            eprintln!("Failed to write config: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to serialize config: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
        }
    }
}

fn show_help() {
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                       Token Management Help                       ║");
    println!("╠═══════════════════════════════════════════════════════════════════╣");
    println!("║  lightd --token what   Show current API token                     ║");
    println!("║  lightd --token reset  Generate new random token                  ║");
    println!("║  lightd --token set    Set custom token (interactive)             ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝");
}
