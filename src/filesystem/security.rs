//! Security utilities for filesystem operations
//! 
//! Prevents path traversal attacks and ensures all operations stay within volume boundaries.

use std::path::{Path, PathBuf};

/// Validate and sanitize a path to prevent directory traversal attacks
/// 
/// This function ensures that:
/// 1. No `..` components that could escape the volume
/// 2. No absolute paths
/// 3. No symlinks that point outside the volume
/// 4. The final resolved path is within the volume root
pub fn validate_path(volume_root: &Path, user_path: &str) -> Result<PathBuf, String> {
    // Reject empty paths
    if user_path.trim().is_empty() {
        return Err("Path cannot be empty".to_string());
    }
    
    // Reject absolute paths
    if user_path.starts_with('/') || user_path.starts_with('\\') {
        return Err("Absolute paths are not allowed".to_string());
    }
    
    // Check for Windows drive letters (C:, D:, etc.)
    if user_path.len() >= 2 && user_path.chars().nth(1) == Some(':') {
        return Err("Drive letters are not allowed".to_string());
    }
    
    // Reject paths with `..` components
    if user_path.contains("..") {
        return Err("Path traversal (..) is not allowed".to_string());
    }
    
    // Build the full path
    let full_path = volume_root.join(user_path);
    
    // Canonicalize to resolve any symlinks and get absolute path
    // This will fail if the path doesn't exist, which is fine for validation
    let canonical_root = volume_root.canonicalize()
        .map_err(|e| format!("Failed to resolve volume root: {}", e))?;
    
    // For paths that don't exist yet, we need to check the parent
    let path_to_check = if full_path.exists() {
        full_path.canonicalize()
            .map_err(|e| format!("Failed to resolve path: {}", e))?
    } else {
        // Check parent directory exists and is within bounds
        let mut check_path = full_path.clone();
        while !check_path.exists() && check_path.parent().is_some() {
            check_path = check_path.parent().unwrap().to_path_buf();
        }
        
        if check_path.exists() {
            let canonical_parent = check_path.canonicalize()
                .map_err(|e| format!("Failed to resolve parent path: {}", e))?;
            
            // Ensure parent is within volume
            if !canonical_parent.starts_with(&canonical_root) {
                return Err("Path escapes volume boundary".to_string());
            }
        }
        
        // Return the non-canonical path for creation
        full_path.clone()
    };
    
    // Ensure the resolved path is within the volume root
    if path_to_check.starts_with(&canonical_root) {
        Ok(full_path)
    } else {
        Err("Path escapes volume boundary".to_string())
    }
}

/// Validate a path for reading (must exist and be within volume)
pub fn validate_read_path(volume_root: &Path, user_path: &str) -> Result<PathBuf, String> {
    let path = validate_path(volume_root, user_path)?;
    
    if !path.exists() {
        return Err("Path does not exist".to_string());
    }
    
    // Double-check with canonical path
    let canonical_root = volume_root.canonicalize()
        .map_err(|e| format!("Failed to resolve volume root: {}", e))?;
    let canonical_path = path.canonicalize()
        .map_err(|e| format!("Failed to resolve path: {}", e))?;
    
    if !canonical_path.starts_with(&canonical_root) {
        return Err("Path escapes volume boundary (symlink detected)".to_string());
    }
    
    Ok(path)
}

/// Validate a path for writing (parent must exist and be within volume)
pub fn validate_write_path(volume_root: &Path, user_path: &str) -> Result<PathBuf, String> {
    let path = validate_path(volume_root, user_path)?;
    
    // Check parent directory
    if let Some(parent) = path.parent() {
        if parent.exists() {
            let canonical_root = volume_root.canonicalize()
                .map_err(|e| format!("Failed to resolve volume root: {}", e))?;
            let canonical_parent = parent.canonicalize()
                .map_err(|e| format!("Failed to resolve parent: {}", e))?;
            
            if !canonical_parent.starts_with(&canonical_root) {
                return Err("Parent directory escapes volume boundary".to_string());
            }
        }
    }
    
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    
    #[test]
    fn test_reject_parent_directory() {
        let root = PathBuf::from("/tmp/test_volume");
        assert!(validate_path(&root, "../etc/passwd").is_err());
        assert!(validate_path(&root, "foo/../../etc/passwd").is_err());
        assert!(validate_path(&root, "./../../etc/passwd").is_err());
    }
    
    #[test]
    fn test_reject_absolute_paths() {
        let root = PathBuf::from("/tmp/test_volume");
        assert!(validate_path(&root, "/etc/passwd").is_err());
        assert!(validate_path(&root, "/tmp/test").is_err());
    }
    
    #[test]
    fn test_accept_valid_paths() {
        let root = PathBuf::from("/tmp/test_volume");
        assert!(validate_path(&root, "foo/bar.txt").is_ok());
        assert!(validate_path(&root, "data/config.json").is_ok());
        assert!(validate_path(&root, "test.txt").is_ok());
    }
    
    #[test]
    fn test_reject_empty_path() {
        let root = PathBuf::from("/tmp/test_volume");
        assert!(validate_path(&root, "").is_err());
        assert!(validate_path(&root, "   ").is_err());
    }
}
