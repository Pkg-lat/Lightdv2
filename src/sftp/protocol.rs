//! SFTP protocol handler
//! 
//! Implements the SSH File Transfer Protocol (SFTP) for file operations

use russh_sftp::protocol::{FileAttributes, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

/// SFTP file handle
pub struct SftpHandle {
    pub path: PathBuf,
    pub file: Option<tokio::fs::File>,
    pub is_dir: bool,
    pub dir_entries: Option<Vec<tokio::fs::DirEntry>>,
    pub dir_index: usize,
}

/// SFTP protocol handler
pub struct SftpProtocol {
    pub volume_path: PathBuf,
    pub handles: Arc<Mutex<HashMap<String, SftpHandle>>>,
    pub handle_counter: Arc<Mutex<u32>>,
}

impl SftpProtocol {
    pub fn new(volume_path: PathBuf) -> Self {
        // Canonicalize volume root so path containment checks are reliable
        let volume_path = volume_path
            .canonicalize()
            .unwrap_or_else(|_| volume_path);
        Self {
            volume_path,
            handles: Arc::new(Mutex::new(HashMap::new())),
            handle_counter: Arc::new(Mutex::new(0)),
        }
    }
    
    /// Volume id (last path component) for normalizing client paths like /volume_id/...
    fn volume_id(&self) -> Option<&std::ffi::OsStr> {
        self.volume_path.file_name()
    }
    
    /// Normalize path: strip leading /volume_id so client root "/" or "/volume_id" maps to volume root
    fn normalize_requested_path<'a>(&self, requested_path: &'a str) -> std::borrow::Cow<'a, str> {
        let path = requested_path.trim_start_matches('/');
        if path.is_empty() {
            return std::borrow::Cow::Borrowed(".");
        }
        if let Some(vid) = self.volume_id() {
            if let Some(vid_str) = vid.to_str() {
                if path == vid_str || path.starts_with(&format!("{}/", vid_str)) {
                    let rest = path.strip_prefix(vid_str).unwrap_or(path).trim_start_matches('/');
                    return std::borrow::Cow::Borrowed(if rest.is_empty() { "." } else { rest });
                }
            }
        }
        std::borrow::Cow::Borrowed(path)
    }
    
    /// Generate next handle ID
    async fn next_handle(&self) -> String {
        let mut counter = self.handle_counter.lock().await;
        *counter += 1;
        format!("handle_{}", *counter)
    }
    
    /// Resolve and validate path within chroot
    fn resolve_path(&self, requested_path: &str) -> Result<PathBuf, String> {
        let normalized = self.normalize_requested_path(requested_path);
        let norm = normalized.as_ref();
        let requested = Path::new(norm);
        
        let full_path = if norm == "." || norm.is_empty() {
            self.volume_path.clone()
        } else if requested.is_absolute() {
            return Err("Invalid path".to_string());
        } else {
            self.volume_path.join(requested)
        };
        
        // Canonicalize when path exists; otherwise build path from canonical parent
        let canonical = match full_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                let parent = full_path.parent().ok_or("Invalid path")?;
                let name = full_path.file_name().ok_or("Invalid path")?;
                let parent_canonical = parent.canonicalize()
                    .map_err(|_| "Invalid path".to_string())?;
                parent_canonical.join(name)
            }
        };
        
        if !canonical.starts_with(&self.volume_path) {
            tracing::warn!("Path traversal attempt: {:?} -> {:?}", requested_path, canonical);
            return Err("Access denied: path outside volume".to_string());
        }
        
        Ok(canonical)
    }
    
    /// Convert file metadata to SFTP attributes
    async fn file_attributes(path: &Path) -> Result<FileAttributes, std::io::Error> {
        let metadata = fs::metadata(path).await?;
        
        // Calculate proper permissions with file type bits
        let permissions = if metadata.is_dir() {
            0o040755  // Directory: S_IFDIR (040000) + rwxr-xr-x (0755)
        } else {
            0o100644  // Regular file: S_IFREG (0100000) + rw-r--r-- (0644)
        };
        
        Ok(FileAttributes {
            size: Some(metadata.len()),
            permissions: Some(permissions),
            ..Default::default()
        })
    }
    
    /// Handle SFTP OPEN request
    pub async fn handle_open(
        &self,
        path: &str,
        flags: OpenFlags,
    ) -> Result<String, String> {
        let resolved_path = self.resolve_path(path)?;
        
        tracing::debug!("SFTP OPEN: {:?} with flags {:?}", resolved_path, flags);
        
        match fs::metadata(&resolved_path).await {
            Ok(meta) => {
                if meta.is_dir() {
                    return Err(format!(
                        "Cannot open directory as file: {}",
                        resolved_path.display()
                    ));
                }
            }
            Err(e) if !flags.contains(OpenFlags::CREATE) => {
                return Err(format!(
                    "No such file or directory: {} ({})",
                    resolved_path.display(),
                    e
                ));
            }
            _ => {}
        }
        
        if flags.contains(OpenFlags::CREATE) {
            if let Some(parent) = resolved_path.parent() {
                fs::create_dir_all(parent).await
                    .map_err(|e| format!("Failed to create parent directory: {}", e))?;
            }
        }
        
        // Open file based on flags
        let file = if flags.contains(OpenFlags::CREATE) {
            fs::OpenOptions::new()
                .read(flags.contains(OpenFlags::READ))
                .write(flags.contains(OpenFlags::WRITE))
                .create(true)
                .truncate(flags.contains(OpenFlags::TRUNCATE))
                .open(&resolved_path)
                .await
                .map_err(|e| format!("Failed to open file: {} ({})", resolved_path.display(), e))?
        } else {
            fs::OpenOptions::new()
                .read(flags.contains(OpenFlags::READ))
                .write(flags.contains(OpenFlags::WRITE))
                .open(&resolved_path)
                .await
                .map_err(|e| format!("Failed to open file: {} ({})", resolved_path.display(), e))?
        };
        
        // Create handle
        let handle_id = self.next_handle().await;
        let handle = SftpHandle {
            path: resolved_path,
            file: Some(file),
            is_dir: false,
            dir_entries: None,
            dir_index: 0,
        };
        
        self.handles.lock().await.insert(handle_id.clone(), handle);
        
        Ok(handle_id)
    }
    
    /// Handle SFTP READ request
    pub async fn handle_read(
        &self,
        handle: &str,
        offset: u64,
        len: u32,
    ) -> Result<Vec<u8>, String> {
        let mut handles = self.handles.lock().await;
        let handle_data = handles.get_mut(handle)
            .ok_or_else(|| "Invalid handle".to_string())?;
        
        let file = handle_data.file.as_mut()
            .ok_or_else(|| "Handle is not a file".to_string())?;
        
        // Seek to offset
        file.seek(std::io::SeekFrom::Start(offset)).await
            .map_err(|e| format!("Seek failed: {}", e))?;
        
        // Read data
        let mut buffer = vec![0u8; len as usize];
        let bytes_read = file.read(&mut buffer).await
            .map_err(|e| format!("Read failed: {}", e))?;
        
        buffer.truncate(bytes_read);
        Ok(buffer)
    }
    
    /// Handle SFTP WRITE request
    pub async fn handle_write(
        &self,
        handle: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<(), String> {
        let mut handles = self.handles.lock().await;
        let handle_data = handles.get_mut(handle)
            .ok_or_else(|| "Invalid handle".to_string())?;
        
        let file = handle_data.file.as_mut()
            .ok_or_else(|| "Handle is not a file".to_string())?;
        
        // Seek to offset
        file.seek(std::io::SeekFrom::Start(offset)).await
            .map_err(|e| format!("Seek failed: {}", e))?;
        
        // Write data
        file.write_all(data).await
            .map_err(|e| format!("Write failed: {}", e))?;
        
        file.flush().await
            .map_err(|e| format!("Flush failed: {}", e))?;
        
        Ok(())
    }
    
    /// Handle SFTP CLOSE request
    pub async fn handle_close(&self, handle: &str) -> Result<(), String> {
        self.handles.lock().await.remove(handle)
            .ok_or_else(|| "Invalid handle".to_string())?;
        
        Ok(())
    }
    
    /// Handle SFTP OPENDIR request
    pub async fn handle_opendir(&self, path: &str) -> Result<String, String> {
        let resolved_path = self.resolve_path(path)?;
        
        tracing::debug!("SFTP OPENDIR: {:?}", resolved_path);
        
        // Read directory entries
        let mut dir = fs::read_dir(&resolved_path).await
            .map_err(|e| format!("Failed to open directory: {}", e))?;
        
        let mut entries = Vec::new();
        while let Some(entry) = dir.next_entry().await
            .map_err(|e| format!("Failed to read directory: {}", e))? {
            entries.push(entry);
        }
        
        // Create handle
        let handle_id = self.next_handle().await;
        let handle = SftpHandle {
            path: resolved_path,
            file: None,
            is_dir: true,
            dir_entries: Some(entries),
            dir_index: 0,
        };
        
        self.handles.lock().await.insert(handle_id.clone(), handle);
        
        Ok(handle_id)
    }
    
    /// Handle SFTP READDIR request
    pub async fn handle_readdir(&self, handle: &str) -> Result<Vec<(String, FileAttributes)>, String> {
        let mut handles = self.handles.lock().await;
        let handle_data = handles.get_mut(handle)
            .ok_or_else(|| "Invalid handle".to_string())?;
        
        if !handle_data.is_dir {
            return Err("Handle is not a directory".to_string());
        }
        
        let entries = handle_data.dir_entries.as_ref()
            .ok_or_else(|| "No directory entries".to_string())?;
        
        // Return batch of entries
        let start = handle_data.dir_index;
        let end = std::cmp::min(start + 10, entries.len());
        
        if start >= entries.len() {
            return Err("EOF".to_string());
        }
        
        let mut result = Vec::new();
        for entry in &entries[start..end] {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let attrs = Self::file_attributes(&path).await
                .map_err(|e| format!("Failed to get attributes: {}", e))?;
            result.push((name, attrs));
        }
        
        handle_data.dir_index = end;
        
        Ok(result)
    }
    
    /// Handle SFTP STAT request (follows symlinks)
    pub async fn handle_stat(&self, path: &str) -> Result<FileAttributes, String> {
        let resolved_path = self.resolve_path(path)?;
        
        tracing::debug!("SFTP STAT: {:?}", resolved_path);
        
        Self::file_attributes(&resolved_path).await
            .map_err(|e| format!("Failed to get attributes: {}", e))
    }
    
    /// Handle SFTP LSTAT request (doesn't follow symlinks)
    pub async fn handle_lstat(&self, path: &str) -> Result<FileAttributes, String> {
        let resolved_path = self.resolve_path(path)?;
        
        tracing::debug!("SFTP LSTAT: {:?}", resolved_path);
        
        // For now, same as stat since we don't have symlink support yet
        Self::file_attributes(&resolved_path).await
            .map_err(|e| format!("Failed to get attributes: {}", e))
    }
    
    /// Handle SFTP MKDIR request
    pub async fn handle_mkdir(&self, path: &str) -> Result<(), String> {
        let resolved_path = self.resolve_path(path)?;
        
        fs::create_dir_all(&resolved_path).await
            .map_err(|e| format!("Failed to create directory: {}", e))
    }
    
    /// Handle SFTP RMDIR request
    pub async fn handle_rmdir(&self, path: &str) -> Result<(), String> {
        let resolved_path = self.resolve_path(path)?;
        
        fs::remove_dir(&resolved_path).await
            .map_err(|e| format!("Failed to remove directory: {}", e))
    }
    
    /// Handle SFTP REMOVE request
    pub async fn handle_remove(&self, path: &str) -> Result<(), String> {
        let resolved_path = self.resolve_path(path)?;
        
        fs::remove_file(&resolved_path).await
            .map_err(|e| format!("Failed to remove file: {}", e))
    }
    
    /// Handle SFTP REALPATH request
    pub async fn handle_realpath(&self, path: &str) -> Result<String, String> {
        // If path is ".", return the volume root
        if path == "." || path.is_empty() {
            return Ok("/".to_string());
        }
        
        let resolved_path = self.resolve_path(path)?;
        
        // Convert absolute path to relative path from volume root
        let relative = resolved_path.strip_prefix(&self.volume_path)
            .map_err(|_| "Path outside volume".to_string())?;
        
        let path_str = if relative.as_os_str().is_empty() {
            "/".to_string()
        } else {
            format!("/{}", relative.display())
        };
        
        Ok(path_str)
    }
    
    /// Handle SFTP FSTAT request (stat on file handle)
    pub async fn handle_fstat(&self, handle: &str) -> Result<FileAttributes, String> {
        let handles = self.handles.lock().await;
        let handle_data = handles.get(handle)
            .ok_or_else(|| "Invalid handle".to_string())?;
        
        Self::file_attributes(&handle_data.path).await
            .map_err(|e| format!("Failed to get attributes: {}", e))
    }
    
    /// Handle SFTP RENAME request
    pub async fn handle_rename(&self, oldpath: &str, newpath: &str) -> Result<(), String> {
        let old_resolved = self.resolve_path(oldpath)?;
        let new_resolved = self.resolve_path(newpath)?;
        
        fs::rename(&old_resolved, &new_resolved).await
            .map_err(|e| format!("Failed to rename: {}", e))
    }
}
