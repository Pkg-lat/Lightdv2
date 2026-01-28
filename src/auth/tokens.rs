//! Token management for WebSocket authentication
//! 
//! Generates temporary tokens for WebSocket connections with TTL and optional single-use.

use serde::{Deserialize, Serialize};
use sled::Db;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub token: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub remove_on_use: bool,
    pub used: bool,
}

pub struct TokenManager {
    db: Arc<Db>,
}

impl TokenManager {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = sled::open(db_path)?;
        Ok(Self { db: Arc::new(db) })
    }
    
    /// Generate a new token
    pub fn generate_token(
        &self,
        ttl_seconds: u64,
        remove_on_use: bool,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        
        // Generate random token
        let token = format!("lightd_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
        
        let token_data = TokenData {
            token: token.clone(),
            created_at: now,
            expires_at: now + ttl_seconds,
            remove_on_use,
            used: false,
        };
        
        // Store in database
        let serialized = serde_json::to_vec(&token_data)?;
        self.db.insert(token.as_bytes(), serialized)?;
        
        tracing::info!("Generated token with TTL {}s, remove_on_use: {}", ttl_seconds, remove_on_use);
        
        Ok(token)
    }
    
    /// Validate a token and optionally mark as used
    #[allow(unused)]
    pub fn validate_token(
        &self,
        token: &str,
        mark_used: bool,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        
        // Get token from database
        let data = match self.db.get(token.as_bytes())? {
            Some(bytes) => bytes,
            None => {
                tracing::warn!("Token not found: {}", token);
                return Ok(false);
            }
        };
        
        let mut token_data: TokenData = serde_json::from_slice(&data)?;
        
        // Check if expired
        if now > token_data.expires_at {
            tracing::warn!("Token expired: {}", token);
            self.db.remove(token.as_bytes())?;
            return Ok(false);
        }
        
        // Check if already used
        if token_data.used && token_data.remove_on_use {
            tracing::warn!("Token already used: {}", token);
            self.db.remove(token.as_bytes())?;
            return Ok(false);
        }
        
        // Mark as used if requested
        if mark_used && token_data.remove_on_use {
            token_data.used = true;
            let serialized = serde_json::to_vec(&token_data)?;
            self.db.insert(token.as_bytes(), serialized)?;
            
            // Remove immediately if remove_on_use
            self.db.remove(token.as_bytes())?;
            tracing::info!("Token used and removed: {}", token);
        }
        
        Ok(true)
    }
    
    /// Clean up expired tokens
    pub fn cleanup_expired(&self) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        
        let mut removed = 0;
        
        for item in self.db.iter() {
            let (key, value) = item?;
            if let Ok(token_data) = serde_json::from_slice::<TokenData>(&value) {
                if now > token_data.expires_at {
                    self.db.remove(&key)?;
                    removed += 1;
                }
            }
        }
        
        if removed > 0 {
            tracing::info!("Cleaned up {} expired tokens", removed);
        }
        
        Ok(removed)
    }
}
