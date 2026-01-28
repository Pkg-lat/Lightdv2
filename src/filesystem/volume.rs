//! This is only for volumes
//! move all other stuff to handler to handle files

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub id: String,
    pub path: PathBuf,
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(unused)]
pub struct VolumeMetadata {
    pub volumes: Vec<Volume>,
}

impl Volume {
    pub fn new(base_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let id = Uuid::new_v4().to_string();
        let path = PathBuf::from(base_path).join(&id);
        
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        Ok(Self {
            id,
            path,
            created_at,
        })
    }

    pub async fn create(&self) -> Result<(), Box<dyn std::error::Error>> {
        tokio::fs::create_dir_all(&self.path).await?;
        tracing::info!("Created volume: {} at {:?}", self.id, self.path);
        Ok(())
    }

    #[allow(unused)]
    pub async fn chown(&self, uid: u32, gid: u32) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::chown;
            chown(&self.path, Some(uid), Some(gid))?;
            tracing::info!("Changed ownership of volume {} to {}:{}", self.id, uid, gid);
        }
        
        #[cfg(not(unix))]
        {
            tracing::warn!("chown not supported on this platform");
        }
        
        Ok(())
    }

    pub async fn list_files(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut files = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.path).await?;

        while let Some(entry) = entries.next_entry().await? {
            if let Some(name) = entry.file_name().to_str() {
                files.push(name.to_string());
            }
        }

        Ok(files)
    }

    pub fn get_path(&self) -> &Path {
        &self.path
    }
}
