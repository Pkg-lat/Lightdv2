//! Container user management
//! 
//! Creates and manages the lightd+ user for running containers
//! Ensures containers don't run as root for security

use std::process::Command;
use serde::{Deserialize, Serialize};

const LIGHTD_USER: &str = "lightd+";
const LIGHTD_UID: u32 = 1000;
const LIGHTD_GID: u32 = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerUser {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
}

impl Default for ContainerUser {
    fn default() -> Self {
        Self {
            username: LIGHTD_USER.to_string(),
            uid: LIGHTD_UID,
            gid: LIGHTD_GID,
        }
    }
}

pub struct UserManager;

impl UserManager {
    /// Ensure the lightd+ user exists on the system
    pub fn ensure_user_exists() -> Result<ContainerUser, Box<dyn std::error::Error + Send + Sync>> {
        let user = ContainerUser::default();
        
        #[cfg(target_os = "linux")]
        {
            Self::ensure_user_linux(&user)?;
        }
        
        #[cfg(target_os = "macos")]
        {
            Self::ensure_user_macos(&user)?;
        }
        
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            tracing::warn!("User management not supported on this platform");
        }
        
        Ok(user)
    }
    
    /// Create user on Linux
    #[cfg(target_os = "linux")]
    fn ensure_user_linux(user: &ContainerUser) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if user exists
        let check = Command::new("id")
            .arg(&user.username)
            .output();
        
        if check.is_ok() && check.unwrap().status.success() {
            tracing::info!("User {} already exists", user.username);
            return Ok(());
        }
        
        // Create group first
        let group_output = Command::new("groupadd")
            .args(&[
                "-g", &user.gid.to_string(),
                &user.username,
            ])
            .output();
        
        if let Ok(output) = group_output {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Ignore "group already exists" error
                if !stderr.contains("already exists") {
                    tracing::warn!("Failed to create group: {}", stderr);
                }
            }
        }
        
        // Create user
        let output = Command::new("useradd")
            .args(&[
                "-u", &user.uid.to_string(),
                "-g", &user.gid.to_string(),
                "-M", // No home directory
                "-s", "/usr/sbin/nologin", // No login shell
                "-c", "Lightd Container User",
                &user.username,
            ])
            .output()?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("already exists") {
                tracing::info!("User {} already exists", user.username);
                return Ok(());
            }
            return Err(format!("Failed to create user: {}", stderr).into());
        }
        
        tracing::info!("Created user {} with UID {} and GID {}", user.username, user.uid, user.gid);
        Ok(())
    }
    
    /// Create user on macOS
    #[cfg(target_os = "macos")]
    fn ensure_user_macos(user: &ContainerUser) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Check if user exists
        let check = Command::new("dscl")
            .args(&[".", "-read", &format!("/Users/{}", user.username)])
            .output();
        
        if check.is_ok() && check.unwrap().status.success() {
            tracing::info!("User {} already exists", user.username);
            return Ok(());
        }
        
        // Find next available UID (starting from 1000)
        let uid = Self::find_available_uid_macos()?;
        
        // Create strings that will live long enough
        let user_path = format!("/Users/{}", user.username);
        let uid_str = uid.to_string();
        
        // Create user using dscl
        let commands = vec![
            vec![".", "-create", &user_path],
            vec![".", "-create", &user_path, "UserShell", "/usr/bin/false"],
            vec![".", "-create", &user_path, "RealName", "Lightd Container User"],
            vec![".", "-create", &user_path, "UniqueID", &uid_str],
            vec![".", "-create", &user_path, "PrimaryGroupID", "20"], // staff group
            vec![".", "-create", &user_path, "NFSHomeDirectory", "/var/empty"],
        ];
        
        for args in commands {
            let output = Command::new("dscl")
                .args(&args)
                .output()?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to create user: {}", stderr).into());
            }
        }
        
        tracing::info!("Created user {} with UID {}", user.username, uid);
        Ok(())
    }
    
    /// Find available UID on macOS
    #[cfg(target_os = "macos")]
    fn find_available_uid_macos() -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        // Start from 1000 and find first available
        for uid in 1000..2000 {
            let check = Command::new("dscl")
                .args(&[".", "-search", "/Users", "UniqueID", &uid.to_string()])
                .output()?;
            
            let stdout = String::from_utf8_lossy(&check.stdout);
            if stdout.trim().is_empty() {
                return Ok(uid);
            }
        }
        
        Err("No available UID found".into())
    }
    
    /// Get the Docker user string (uid:gid)
    pub fn get_docker_user_string(user: &ContainerUser) -> String {
        format!("{}:{}", user.uid, user.gid)
    }
    
    /// Set ownership of a path to the lightd+ user
    pub fn chown_path(path: &str, user: &ContainerUser) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(unix)]
        {
            let output = Command::new("chown")
                .args(&[
                    "-R",
                    &format!("{}:{}", user.uid, user.gid),
                    path,
                ])
                .output()?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to chown: {}", stderr).into());
            }
            
            tracing::debug!("Changed ownership of {} to {}:{}", path, user.uid, user.gid);
        }
        
        Ok(())
    }
    
    /// Verify user exists and get info
    #[allow(dead_code)]
    pub fn verify_user(username: &str) -> Result<ContainerUser, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(unix)]
        {
            let output = Command::new("id")
                .args(&["-u", username])
                .output()?;
            
            if !output.status.success() {
                return Err(format!("User {} does not exist", username).into());
            }
            
            let uid_str = String::from_utf8_lossy(&output.stdout);
            let uid: u32 = uid_str.trim().parse()?;
            
            let output = Command::new("id")
                .args(&["-g", username])
                .output()?;
            
            let gid_str = String::from_utf8_lossy(&output.stdout);
            let gid: u32 = gid_str.trim().parse()?;
            
            Ok(ContainerUser {
                username: username.to_string(),
                uid,
                gid,
            })
        }
        
        #[cfg(not(unix))]
        {
            Err("User verification not supported on this platform".into())
        }
    }
    
    /// Delete the lightd+ user (cleanup)
    #[allow(dead_code)]
    pub fn delete_user(username: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_os = "linux")]
        {
            let output = Command::new("userdel")
                .arg(username)
                .output()?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to delete user: {}", stderr).into());
            }
            
            // Also delete group
            let _ = Command::new("groupdel")
                .arg(username)
                .output();
        }
        
        #[cfg(target_os = "macos")]
        {
            let output = Command::new("dscl")
                .args(&[".", "-delete", &format!("/Users/{}", username)])
                .output()?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to delete user: {}", stderr).into());
            }
        }
        
        tracing::info!("Deleted user {}", username);
        Ok(())
    }
}
