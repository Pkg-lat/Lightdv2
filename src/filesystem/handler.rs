use super::volume::{Volume};
use super::security;
use super::quota::QuotaManager;
use super::fileinfo::{FileObject, list_directory_detailed};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::fs::File;
use std::io::Write;
use zip::ZipArchive;
use tar::Archive;
use flate2::read::GzDecoder;
use bzip2::read::BzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use bzip2::write::BzEncoder;
use zip::write::{FileOptions, ZipWriter};
use std::io::{Read, Seek};

pub struct VolumeHandler {
    volumes: Arc<RwLock<Vec<Volume>>>,
    base_path: String,
    quota_manager: Arc<QuotaManager>,
}

impl VolumeHandler {
    pub fn new(base_path: String) -> Self {
        let quota_manager = Arc::new(QuotaManager::new(PathBuf::from(&base_path)));
        Self {
            volumes: Arc::new(RwLock::new(Vec::new())),
            base_path,
            quota_manager,
        }
    }

    pub async fn create_volume(&self) -> Result<Volume, Box<dyn std::error::Error>> {
        let volume = Volume::new(&self.base_path)?;
        volume.create().await?;

        let mut volumes = self.volumes.write().await;
        volumes.push(volume.clone());

        tracing::info!("Volume created with ID: {}", volume.id);
        Ok(volume)
    }
    
    pub async fn create_volume_with_quota(&self, size_mb: Option<u64>) -> Result<Volume, Box<dyn std::error::Error>> {
        let quota_size = size_mb.unwrap_or(1024); // Default 1GB
        let volume = Volume::new_with_quota(&self.base_path, quota_size)?;
        
        // Create volume with OS-level quota
        let _path = self.quota_manager.create_volume_with_quota(&volume.id, Some(quota_size))
            .await
            .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
        
        let mut volumes = self.volumes.write().await;
        volumes.push(volume.clone());

        tracing::info!("Volume created with ID: {} and {}MB quota", volume.id, quota_size);
        Ok(volume)
    }
    
    pub async fn get_volume_quota(&self, id: &str) -> Result<super::quota::DiskQuota, Box<dyn std::error::Error>> {
        self.quota_manager.get_quota_usage(id)
            .await
            .map_err(|e| e.to_string().into())
    }
    
    #[allow(dead_code)]
    pub async fn check_volume_quota(&self, id: &str) -> Result<bool, Box<dyn std::error::Error>> {
        self.quota_manager.check_quota_exceeded(id)
            .await
            .map_err(|e| e.to_string().into())
    }
    
    pub async fn resize_volume(&self, id: &str, new_size_mb: u64) -> Result<(), Box<dyn std::error::Error>> {
        self.quota_manager.resize_volume(id, new_size_mb)
            .await
            .map_err(|e| e.to_string().into())
    }

    pub async fn get_volume(&self, id: &str) -> Option<Volume> {
        let volumes = self.volumes.read().await;
        volumes.iter().find(|v| v.id == id).cloned()
    }

    pub async fn list_volumes(&self) -> Vec<Volume> {
        let volumes = self.volumes.read().await;
        volumes.clone()
    }

    pub async fn delete_volume(&self, id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut volumes = self.volumes.write().await;
        
        if let Some(pos) = volumes.iter().position(|v| v.id == id) {
            let volume = volumes.remove(pos);
            
            // Delete with quota manager if volume has quota
            if volume.quota_mb.is_some() {
                self.quota_manager.delete_volume(id)
                    .await
                    .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;
            } else {
                tokio::fs::remove_dir_all(&volume.path).await?;
            }
            
            tracing::info!("Deleted volume: {}", id);
            Ok(())
        } else {
            Err("Volume not found".into())
        }
    }

    pub async fn list_volume_files(&self, id: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        if let Some(volume) = self.get_volume(id).await {
            volume.list_files().await
        } else {
            Err("Volume not found".into())
        }
    }
    
    pub async fn list_volume_files_detailed(&self, id: &str, path: Option<&str>) -> Result<Vec<FileObject>, Box<dyn std::error::Error>> {
        if let Some(volume) = self.get_volume(id).await {
            let target_path = if let Some(p) = path {
                volume.get_path().join(p.trim_start_matches('/'))
            } else {
                volume.get_path().to_path_buf()
            };
            
            // Validate path is within volume
            let canonical = target_path.canonicalize()?;
            let volume_canonical = volume.get_path().canonicalize()?;
            
            if !canonical.starts_with(&volume_canonical) {
                return Err("Path traversal detected".into());
            }
            
            list_directory_detailed(&target_path).await.map_err(|e| e.to_string().into())
        } else {
            Err("Volume not found".into())
        }
    }

    pub async fn write_file(&self, id: &str, filename: &str, content: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(volume) = self.get_volume(id).await {
            // Validate path to prevent traversal
            let safe_path = security::validate_write_path(volume.get_path(), filename)?;
            
            // Ensure parent directory exists
            if let Some(parent) = safe_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            
            tokio::fs::write(&safe_path, content).await?;
            tracing::info!("Wrote file {} to volume {}", filename, id);
            Ok(safe_path)
        } else {
            Err("Volume not found".into())
        }
    }

    pub async fn create_folder(&self, id: &str, root: &str, name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(volume) = self.get_volume(id).await {
            // Combine root and name for validation
            let full_path = if root == "/" {
                name.to_string()
            } else {
                format!("{}/{}", root.trim_start_matches('/').trim_end_matches('/'), name)
            };
            
            // Validate path to prevent traversal
            let safe_path = security::validate_write_path(volume.get_path(), &full_path)?;
            
            tokio::fs::create_dir_all(&safe_path).await?;
            tracing::info!("Created folder {} at {} in volume {}", name, root, id);
            Ok(safe_path)
        } else {
            Err("Volume not found".into())
        }
    }

    pub async fn copy(&self, id: &str, source: &str, destination: &str, is_folder: bool) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(volume) = self.get_volume(id).await {
            // Validate both source and destination paths
            let source_path = security::validate_read_path(volume.get_path(), source.trim_start_matches('/'))?;
            let dest_path = security::validate_write_path(volume.get_path(), destination.trim_start_matches('/'))?;
            
            if !source_path.exists() {
                return Err("Source path does not exist".into());
            }
            
            if is_folder {
                Box::pin(copy_dir_recursive(&source_path, &dest_path)).await?;
                tracing::info!("Copied folder from {} to {} in volume {}", source, destination, id);
            } else {
                if let Some(parent) = dest_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::copy(&source_path, &dest_path).await?;
                tracing::info!("Copied file from {} to {} in volume {}", source, destination, id);
            }
            
            Ok(dest_path)
        } else {
            Err("Volume not found".into())
        }
    }

    pub async fn decompress(&self, id: &str, root: &str, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(volume) = self.get_volume(id).await {
            let base_path = if root == "/" {
                volume.get_path().to_path_buf()
            } else {
                volume.get_path().join(root.trim_start_matches('/'))
            };
            
            let archive_path = base_path.join(file);
            
            if !archive_path.exists() {
                return Err("Archive file does not exist".into());
            }
            
            let extract_path = base_path.clone();
            
            // Determine archive type by extension
            if file.ends_with(".zip") {
                let extract_clone = extract_path.clone();
                let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let file = File::open(&archive_path).map_err(|e| e.to_string())?;
                    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;
                    archive.extract(&extract_clone).map_err(|e| e.to_string())?;
                    Ok(())
                }).await.map_err(|e| e.to_string())?;
                result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                tracing::info!("Extracted ZIP archive {} in volume {}", file, id);
            } else if file.ends_with(".tar.gz") || file.ends_with(".tgz") {
                let extract_clone = extract_path.clone();
                let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let file = File::open(&archive_path).map_err(|e| e.to_string())?;
                    let decoder = GzDecoder::new(file);
                    let mut archive = Archive::new(decoder);
                    archive.unpack(&extract_clone).map_err(|e| e.to_string())?;
                    Ok(())
                }).await.map_err(|e| e.to_string())?;
                result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                tracing::info!("Extracted TAR.GZ archive {} in volume {}", file, id);
            } else if file.ends_with(".tar.bz2") || file.ends_with(".tbz2") {
                let extract_clone = extract_path.clone();
                let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let file = File::open(&archive_path).map_err(|e| e.to_string())?;
                    let decoder = BzDecoder::new(file);
                    let mut archive = Archive::new(decoder);
                    archive.unpack(&extract_clone).map_err(|e| e.to_string())?;
                    Ok(())
                }).await.map_err(|e| e.to_string())?;
                result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                tracing::info!("Extracted TAR.BZ2 archive {} in volume {}", file, id);
            } else if file.ends_with(".tar") {
                let extract_clone = extract_path.clone();
                let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                    let file = File::open(&archive_path).map_err(|e| e.to_string())?;
                    let mut archive = Archive::new(file);
                    archive.unpack(&extract_clone).map_err(|e| e.to_string())?;
                    Ok(())
                }).await.map_err(|e| e.to_string())?;
                result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                tracing::info!("Extracted TAR archive {} in volume {}", file, id);
            } else {
                return Err("Unsupported archive format".into());
            }
            
            Ok(extract_path)
        } else {
            Err("Volume not found".into())
        }
    }

    pub async fn compress(&self, id: &str, sources: Vec<String>, output: &str, format: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Some(volume) = self.get_volume(id).await {
            let volume_path = volume.get_path().to_path_buf();
            
            // Validate all source paths exist
            let mut source_paths = Vec::new();
            for source in &sources {
                let path = volume_path.join(source.trim_start_matches('/'));
                if !path.exists() {
                    return Err(format!("Source path does not exist: {}", source).into());
                }
                source_paths.push(path);
            }
            
            let output_path = volume_path.join(output.trim_start_matches('/'));
            
            // Ensure output directory exists
            if let Some(parent) = output_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            
            let volume_clone = volume_path.clone();
            
            match format {
                "zip" => {
                    let output_clone = output_path.clone();
                    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                        let file = File::create(&output_clone).map_err(|e| e.to_string())?;
                        let mut zip = ZipWriter::new(file);
                        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
                        
                        for source_path in source_paths {
                            if source_path.is_file() {
                                add_file_to_zip(&mut zip, &source_path, &volume_clone, &options)?;
                            } else {
                                add_dir_to_zip(&mut zip, &source_path, &volume_clone, &options)?;
                            }
                        }
                        
                        zip.finish().map_err(|e| e.to_string())?;
                        Ok(())
                    }).await.map_err(|e| e.to_string())?;
                    result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                    tracing::info!("Created ZIP archive {} in volume {}", output, id);
                }
                "tar" => {
                    let output_clone = output_path.clone();
                    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                        let file = File::create(&output_clone).map_err(|e| e.to_string())?;
                        let mut tar = tar::Builder::new(file);
                        
                        for source_path in source_paths {
                            let rel_path = source_path.strip_prefix(&volume_clone)
                                .map_err(|e| e.to_string())?;
                            if source_path.is_file() {
                                tar.append_path_with_name(&source_path, rel_path).map_err(|e| e.to_string())?;
                            } else {
                                tar.append_dir_all(rel_path, &source_path).map_err(|e| e.to_string())?;
                            }
                        }
                        
                        tar.finish().map_err(|e| e.to_string())?;
                        Ok(())
                    }).await.map_err(|e| e.to_string())?;
                    result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                    tracing::info!("Created TAR archive {} in volume {}", output, id);
                }
                "tar.gz" => {
                    let output_clone = output_path.clone();
                    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                        let file = File::create(&output_clone).map_err(|e| e.to_string())?;
                        let encoder = GzEncoder::new(file, Compression::default());
                        let mut tar = tar::Builder::new(encoder);
                        
                        for source_path in source_paths {
                            let rel_path = source_path.strip_prefix(&volume_clone)
                                .map_err(|e| e.to_string())?;
                            if source_path.is_file() {
                                tar.append_path_with_name(&source_path, rel_path).map_err(|e| e.to_string())?;
                            } else {
                                tar.append_dir_all(rel_path, &source_path).map_err(|e| e.to_string())?;
                            }
                        }
                        
                        tar.finish().map_err(|e| e.to_string())?;
                        Ok(())
                    }).await.map_err(|e| e.to_string())?;
                    result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                    tracing::info!("Created TAR.GZ archive {} in volume {}", output, id);
                }
                "tar.bz2" => {
                    let output_clone = output_path.clone();
                    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
                        let file = File::create(&output_clone).map_err(|e| e.to_string())?;
                        let encoder = BzEncoder::new(file, bzip2::Compression::default());
                        let mut tar = tar::Builder::new(encoder);
                        
                        for source_path in source_paths {
                            let rel_path = source_path.strip_prefix(&volume_clone)
                                .map_err(|e| e.to_string())?;
                            if source_path.is_file() {
                                tar.append_path_with_name(&source_path, rel_path).map_err(|e| e.to_string())?;
                            } else {
                                tar.append_dir_all(rel_path, &source_path).map_err(|e| e.to_string())?;
                            }
                        }
                        
                        tar.finish().map_err(|e| e.to_string())?;
                        Ok(())
                    }).await.map_err(|e| e.to_string())?;
                    result.map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
                    tracing::info!("Created TAR.BZ2 archive {} in volume {}", output, id);
                }
                _ => return Err("Unsupported compression format".into()),
            }
            
            Ok(output_path)
        } else {
            Err("Volume not found".into())
        }
    }
}

async fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    tokio::fs::create_dir_all(dst).await?;
    
    let mut entries = tokio::fs::read_dir(src).await?;
    
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if file_type.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    
    Ok(())
}

fn add_file_to_zip<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    path: &PathBuf,
    base: &PathBuf,
    options: &FileOptions,
) -> Result<(), String> {
    let name = path.strip_prefix(base).map_err(|e| e.to_string())?;
    zip.start_file(name.to_string_lossy().to_string(), *options)
        .map_err(|e| e.to_string())?;
    
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
    zip.write_all(&buffer).map_err(|e| e.to_string())?;
    
    Ok(())
}

fn add_dir_to_zip<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    path: &PathBuf,
    base: &PathBuf,
    options: &FileOptions,
) -> Result<(), String> {
    let entries = std::fs::read_dir(path).map_err(|e| e.to_string())?;
    
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();
        
        if entry_path.is_file() {
            add_file_to_zip(zip, &entry_path, base, options)?;
        } else if entry_path.is_dir() {
            add_dir_to_zip(zip, &entry_path, base, options)?;
        }
    }
    
    Ok(())
}
