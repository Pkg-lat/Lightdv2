//! Token management for API and WebSocket authentication
//!
//! Generates temporary tokens with a TTL, an owner, and optional single-use.
//! The owner is what makes a token *scoped*: a tenant token may only act on
//! containers belonging to that owner.

use serde::{Deserialize, Serialize};
use sled::Db;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Identity used for tokens minted by the master config token.
pub const ADMIN_OWNER: &str = "system";

/// Longest TTL a caller may request, in seconds (30 days). Unbounded TTLs let a
/// single minted token outlive any rotation of the master token.
pub const MAX_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub token: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub remove_on_use: bool,
    pub used: bool,
    /// Owner this token acts as. `ADMIN_OWNER` for master-derived tokens; the
    /// issuing admin's chosen owner for tenant tokens.
    #[serde(default)]
    pub owner: String,
}

impl TokenData {
    /// Tokens carrying the system owner act with admin authority.
    pub fn is_admin(&self) -> bool {
        self.owner == ADMIN_OWNER
    }
}

pub struct TokenManager {
    db: Arc<Db>,
}

impl TokenManager {
    pub fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = sled::open(db_path)?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Generate a new token owned by `owner`.
    pub fn generate_token_for(
        &self,
        owner: String,
        ttl_seconds: u64,
        remove_on_use: bool,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        // Clamp the TTL so a caller cannot mint an effectively permanent token.
        let ttl_seconds = ttl_seconds.min(MAX_TTL_SECONDS);

        // Generate random token
        let token = format!("lightd_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));

        let token_data = TokenData {
            token: token.clone(),
            created_at: now,
            expires_at: now + ttl_seconds,
            remove_on_use,
            used: false,
            owner,
        };

        // Store in database
        let serialized = serde_json::to_vec(&token_data)?;
        self.db.insert(token.as_bytes(), serialized)?;

        // The token value itself is never logged: it is a live credential and
        // application logs are widely readable.
        tracing::info!(
            "Generated token (owner: {}, TTL {}s, remove_on_use: {})",
            token_data.owner,
            ttl_seconds,
            remove_on_use
        );

        Ok(token)
    }

    /// Fetch a live token's data, marking it used when `mark_used` is set.
    ///
    /// Expiry and single-use are enforced against the stored record, and removal
    /// happens in the same step so a single-use token cannot be redeemed twice.
    pub fn validate_token(
        &self,
        token: &str,
        mark_used: bool,
    ) -> Result<Option<TokenData>, Box<dyn std::error::Error + Send + Sync>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        // Get token from database
        let data = match self.db.get(token.as_bytes())? {
            Some(bytes) => bytes,
            None => {
                tracing::warn!("Rejected token: not found");
                return Ok(None);
            }
        };

        let mut token_data: TokenData = serde_json::from_slice(&data)?;

        // Check if expired
        if now > token_data.expires_at {
            tracing::warn!("Rejected token: expired");
            self.db.remove(token.as_bytes())?;
            return Ok(None);
        }

        // Single-use tokens are consumed here, in the same call that reads them.
        //
        // This previously wrote `used = true` and then removed the key, while the
        // WebSocket re-validated the same token on every event — so the re-check
        // always failed and tore the connection down. Removal *is* the consumption
        // record, so there is no separate flag to get out of sync.
        if token_data.remove_on_use && mark_used {
            self.db.remove(token.as_bytes())?;
            tracing::info!("Consumed single-use token");
            token_data.used = true;
            return Ok(Some(token_data));
        }

        Ok(Some(token_data))
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