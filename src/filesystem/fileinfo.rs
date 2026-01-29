//! File information and metadata
//! 
//! Provides comprehensive file details including MIME types via magic number detection

use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileObject {
    pub object: String,
    pub attributes: FileAttributes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttributes {
    pub name: String,
    pub mode: String,
    pub mode_bits: String,
    pub size: u64,
    pub is_file: bool,
    pub is_symlink: bool,
    pub mimetype: String,
    pub created_at: String,
    pub modified_at: String,
}

impl FileObject {
    /// Create a FileObject from a path
    pub async fn from_path(path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let metadata = fs::metadata(path).await?;
        
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        
        let is_file = metadata.is_file();
        let is_symlink = metadata.is_symlink();
        let size = metadata.len();
        
        // Get file mode (Unix permissions)
        #[cfg(unix)]
        let (mode, mode_bits) = {
            use std::os::unix::fs::PermissionsExt;
            let mode_num = metadata.permissions().mode();
            let mode_str = Self::format_mode(mode_num);
            let mode_bits = format!("{:o}", mode_num & 0o777);
            (mode_str, mode_bits)
        };
        
        #[cfg(not(unix))]
        let (mode, mode_bits) = {
            let readonly = metadata.permissions().readonly();
            if is_file {
                if readonly {
                    ("-r--r--r--".to_string(), "444".to_string())
                } else {
                    ("-rw-rw-rw-".to_string(), "666".to_string())
                }
            } else {
                if readonly {
                    ("dr-xr-xr-x".to_string(), "555".to_string())
                } else {
                    ("drwxrwxrwx".to_string(), "777".to_string())
                }
            }
        };
        
        // Detect MIME type
        let mimetype = if is_symlink {
            "inode/symlink".to_string()
        } else if !is_file {
            "inode/directory".to_string()
        } else {
            Self::detect_mimetype(path).await
        };
        
        // Get timestamps
        let created_at = metadata
            .created()
            .ok()
            .and_then(|t| DateTime::<Utc>::from(t).to_rfc3339().into())
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| DateTime::<Utc>::from(t).to_rfc3339().into())
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        
        Ok(Self {
            object: "file_object".to_string(),
            attributes: FileAttributes {
                name,
                mode,
                mode_bits,
                size,
                is_file,
                is_symlink,
                mimetype,
                created_at,
                modified_at,
            },
        })
    }
    
    /// Detect MIME type using magic number detection
    async fn detect_mimetype(path: &Path) -> String {
        // Read first 8KB for magic number detection
        match fs::read(path).await {
            Ok(bytes) => {
                // Use infer crate for magic number detection
                if let Some(kind) = infer::get(&bytes) {
                    return kind.mime_type().to_string();
                }
                
                // Fallback to extension-based detection
                Self::mimetype_from_extension(path)
            }
            Err(_) => {
                // If we can't read the file, use extension
                Self::mimetype_from_extension(path)
            }
        }
    }
    
    /// Get MIME type from file extension (fallback)
    fn mimetype_from_extension(path: &Path) -> String {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        
        match extension.to_lowercase().as_str() {
            // Text files
            "txt" => "text/plain",
            "md" => "text/markdown",
            "json" => "application/json",
            "xml" => "application/xml",
            "yaml" | "yml" => "application/x-yaml",
            "toml" => "application/toml",
            "ini" | "conf" | "cfg" => "text/plain",
            
            // Scripts
            "sh" | "bash" => "application/x-sh",
            "py" => "text/x-python",
            "js" => "application/javascript",
            "ts" => "application/typescript",
            "rb" => "text/x-ruby",
            "php" => "application/x-php",
            "pl" => "text/x-perl",
            
            // Web
            "html" | "htm" => "text/html",
            "css" => "text/css",
            
            // Programming
            "c" => "text/x-c",
            "cpp" | "cc" | "cxx" => "text/x-c++",
            "h" | "hpp" => "text/x-c",
            "rs" => "text/x-rust",
            "go" => "text/x-go",
            "java" => "text/x-java",
            
            // Archives
            "zip" => "application/zip",
            "tar" => "application/x-tar",
            "gz" => "application/gzip",
            "bz2" => "application/x-bzip2",
            "xz" => "application/x-xz",
            "7z" => "application/x-7z-compressed",
            "rar" => "application/vnd.rar",
            
            // Images
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "svg" => "image/svg+xml",
            "webp" => "image/webp",
            "ico" => "image/x-icon",
            
            // Documents
            "pdf" => "application/pdf",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "ppt" => "application/vnd.ms-powerpoint",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            
            // Databases
            "db" | "sqlite" | "sqlite3" => "application/x-sqlite3",
            "sql" => "application/sql",
            
            // Logs
            "log" => "text/plain",
            
            _ => "application/octet-stream",
        }.to_string()
    }
    
    /// Format Unix mode to string (e.g., "-rw-r--r--")
    #[cfg(unix)]
    fn format_mode(mode: u32) -> String {
        let file_type = match mode & 0o170000 {
            0o040000 => 'd', // directory
            0o120000 => 'l', // symlink
            0o100000 => '-', // regular file
            0o060000 => 'b', // block device
            0o020000 => 'c', // character device
            0o010000 => 'p', // FIFO
            0o140000 => 's', // socket
            _ => '?',
        };
        
        let user = [
            if mode & 0o400 != 0 { 'r' } else { '-' },
            if mode & 0o200 != 0 { 'w' } else { '-' },
            if mode & 0o100 != 0 { 'x' } else { '-' },
        ];
        
        let group = [
            if mode & 0o040 != 0 { 'r' } else { '-' },
            if mode & 0o020 != 0 { 'w' } else { '-' },
            if mode & 0o010 != 0 { 'x' } else { '-' },
        ];
        
        let other = [
            if mode & 0o004 != 0 { 'r' } else { '-' },
            if mode & 0o002 != 0 { 'w' } else { '-' },
            if mode & 0o001 != 0 { 'x' } else { '-' },
        ];
        
        format!(
            "{}{}{}{}{}{}{}{}{}{}",
            file_type,
            user[0], user[1], user[2],
            group[0], group[1], group[2],
            other[0], other[1], other[2]
        )
    }
}

/// List files in a directory with full metadata
pub async fn list_directory_detailed(
    path: &Path,
) -> Result<Vec<FileObject>, Box<dyn std::error::Error + Send + Sync>> {
    let mut files = Vec::new();
    let mut entries = fs::read_dir(path).await?;
    
    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();
        match FileObject::from_path(&entry_path).await {
            Ok(file_obj) => files.push(file_obj),
            Err(e) => {
                tracing::warn!("Failed to get file info for {:?}: {}", entry_path, e);
            }
        }
    }
    
    // Sort: directories first, then by name
    files.sort_by(|a, b| {
        match (a.attributes.is_file, b.attributes.is_file) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => a.attributes.name.cmp(&b.attributes.name),
        }
    });
    
    Ok(files)
}
