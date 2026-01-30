//! SFTP credentials management
//! 
//! Manages per-container SFTP credentials with password hashing

use serde::{Deserialize, Serialize};
use sled::Db;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpCredentials {
    pub container_id: String,
    pub username: String,
    pub password_hash: String,
    pub volume_id: String,
    pub created_at: u64,
    pub updated_at: u64,
}

pub struct CredentialsManager {
    db: Arc<Db>,
}

impl CredentialsManager {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = sled::open(db_path)?;
        Ok(Self { db: Arc::new(db) })
    }
    
    /// Generate new SFTP credentials for a container
    pub fn generate_credentials(
        &self,
        container_id: &str,
        volume_id: &str,
        custom_username: Option<String>,
        custom_password: Option<String>,
    ) -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        
        // Generate username (default: container_id or custom)
        let username = custom_username.unwrap_or_else(|| container_id.to_string());
        
        // Generate password (default: random or custom)
        let password = custom_password.unwrap_or_else(|| {
            use rand::Rng;
            let charset: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*";
            let mut rng = rand::thread_rng();
            (0..24)
                .map(|_| {
                    let idx = rng.gen_range(0..charset.len());
                    charset[idx] as char
                })
                .collect()
        });
        
        // Hash password
        let password_hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST)?;
        
        let credentials = SftpCredentials {
            container_id: container_id.to_string(),
            username: username.clone(),
            password_hash,
            volume_id: volume_id.to_string(),
            created_at: now,
            updated_at: now,
        };
        
        // Store in database (key: container_id)
        let serialized = serde_json::to_vec(&credentials)?;
        self.db.insert(container_id.as_bytes(), serialized)?;
        
        tracing::info!("Generated SFTP credentials for container: {}", container_id);
        
        Ok((username, password))
    }
    
    /// Get credentials for a container
    pub fn get_credentials(
        &self,
        container_id: &str,
    ) -> Result<Option<SftpCredentials>, Box<dyn std::error::Error + Send + Sync>> {
        match self.db.get(container_id.as_bytes())? {
            Some(bytes) => {
                let creds: SftpCredentials = serde_json::from_slice(&bytes)?;
                Ok(Some(creds))
            }
            None => Ok(None),
        }
    }
    
    /// Verify username and password
    pub fn verify_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<SftpCredentials>, Box<dyn std::error::Error + Send + Sync>> {
        // Search for credentials by username
        for item in self.db.iter() {
            let (_, value) = item?;
            if let Ok(creds) = serde_json::from_slice::<SftpCredentials>(&value) {
                if creds.username == username {
                    // Verify password
                    if bcrypt::verify(password, &creds.password_hash)? {
                        return Ok(Some(creds));
                    } else {
                        return Ok(None);
                    }
                }
            }
        }
        
        Ok(None)
    }
    
    /// Delete credentials for a container
    pub fn delete_credentials(
        &self,
        container_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.db.remove(container_id.as_bytes())?;
        tracing::info!("Deleted SFTP credentials for container: {}", container_id);
        Ok(())
    }
}
