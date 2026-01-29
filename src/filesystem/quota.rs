//! Disk quota management for volumes
//! 
//! Uses OS-level mechanisms to enforce disk limits:
//! - macOS: Disk images (DMG) with fixed size
//! - Linux: Filesystem quotas or loop devices

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs;

const DEFAULT_QUOTA_MB: u64 = 1024; // 1GB default

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskQuota {
    pub size_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct QuotaConfig {
    pub volume_id: String,
    pub size_mb: u64,
    pub quota_type: QuotaType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum QuotaType {
    #[cfg(target_os = "macos")]
    DiskImage,
    #[cfg(target_os = "linux")]
    LoopDevice,
    Directory, // Fallback - just track size
}

pub struct QuotaManager {
    base_path: PathBuf,
}

impl QuotaManager {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Create a volume with disk quota
    pub async fn create_volume_with_quota(
        &self,
        volume_id: &str,
        size_mb: Option<u64>,
    ) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        let size = size_mb.unwrap_or(DEFAULT_QUOTA_MB);
        let volume_path = self.base_path.join(volume_id);

        #[cfg(target_os = "macos")]
        {
            self.create_macos_disk_image(volume_id, &volume_path, size)
                .await?;
        }

        #[cfg(target_os = "linux")]
        {
            self.create_linux_quota_volume(volume_id, &volume_path, size)
                .await?;
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            // Fallback: just create directory
            fs::create_dir_all(&volume_path).await?;
            tracing::warn!(
                "Disk quotas not supported on this platform, created directory without quota"
            );
        }

        Ok(volume_path)
    }

    /// Create disk image on macOS
    #[cfg(target_os = "macos")]
    async fn create_macos_disk_image(
        &self,
        volume_id: &str,
        volume_path: &Path,
        size_mb: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let dmg_path = self.base_path.join(format!("{}.dmg", volume_id));
        let mount_point = volume_path;

        // Create sparse disk image
        let output = Command::new("hdiutil")
            .args(&[
                "create",
                "-size",
                &format!("{}m", size_mb),
                "-fs",
                "HFS+",
                "-volname",
                volume_id,
                "-type",
                "SPARSE",
                dmg_path.to_str().unwrap(),
            ])
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to create disk image: {}", error).into());
        }

        // Mount the disk image
        fs::create_dir_all(mount_point).await?;

        let sparse_dmg = format!("{}.sparseimage", dmg_path.to_str().unwrap());
        let output = Command::new("hdiutil")
            .args(&[
                "attach",
                &sparse_dmg,
                "-mountpoint",
                mount_point.to_str().unwrap(),
                "-nobrowse",
            ])
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to mount disk image: {}", error).into());
        }

        tracing::info!(
            "Created macOS disk image for volume {} with {}MB quota",
            volume_id,
            size_mb
        );

        Ok(())
    }

    /// Create quota volume on Linux using loop device
    #[cfg(target_os = "linux")]
    async fn create_linux_quota_volume(
        &self,
        volume_id: &str,
        volume_path: &Path,
        size_mb: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let img_path = self.base_path.join(format!("{}.img", volume_id));

        // Create sparse file
        let output = Command::new("dd")
            .args(&[
                "if=/dev/zero",
                &format!("of={}", img_path.to_str().unwrap()),
                "bs=1M",
                &format!("count=0"),
                &format!("seek={}", size_mb),
            ])
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to create sparse file: {}", error).into());
        }

        // Create ext4 filesystem
        let output = Command::new("mkfs.ext4")
            .args(&["-F", img_path.to_str().unwrap()])
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to create filesystem: {}", error).into());
        }

        // Create mount point
        fs::create_dir_all(volume_path).await?;

        // Mount the loop device
        let output = Command::new("mount")
            .args(&[
                "-o",
                "loop",
                img_path.to_str().unwrap(),
                volume_path.to_str().unwrap(),
            ])
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to mount loop device: {}", error).into());
        }

        tracing::info!(
            "Created Linux loop device for volume {} with {}MB quota",
            volume_id,
            size_mb
        );

        Ok(())
    }

    /// Get disk usage for a volume
    pub async fn get_quota_usage(
        &self,
        volume_id: &str,
    ) -> Result<DiskQuota, Box<dyn std::error::Error + Send + Sync>> {
        let volume_path = self.base_path.join(volume_id);

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let output = Command::new("df")
                .args(&["-m", volume_path.to_str().unwrap()])
                .output()?;

            if !output.status.success() {
                return Err("Failed to get disk usage".into());
            }

            let output_str = String::from_utf8_lossy(&output.stdout);
            let lines: Vec<&str> = output_str.lines().collect();

            if lines.len() < 2 {
                return Err("Invalid df output".into());
            }

            // Parse df output: Filesystem Size Used Avail Capacity
            let parts: Vec<&str> = lines[1].split_whitespace().collect();
            if parts.len() < 4 {
                return Err("Invalid df output format".into());
            }

            let size_mb = parts[1].parse::<u64>().unwrap_or(0);
            let used_mb = parts[2].parse::<u64>().unwrap_or(0);
            let available_mb = parts[3].parse::<u64>().unwrap_or(0);

            Ok(DiskQuota {
                size_mb,
                used_mb,
                available_mb,
            })
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            // Fallback: calculate directory size
            let used_mb = self.calculate_directory_size(&volume_path).await? / (1024 * 1024);
            Ok(DiskQuota {
                size_mb: DEFAULT_QUOTA_MB,
                used_mb,
                available_mb: DEFAULT_QUOTA_MB.saturating_sub(used_mb),
            })
        }
    }

    /// Check if volume is out of space
    #[allow(dead_code)]
    pub async fn check_quota_exceeded(
        &self,
        volume_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let quota = self.get_quota_usage(volume_id).await?;
        
        // Consider quota exceeded if less than 10MB available or 95% full
        let is_exceeded = quota.available_mb < 10 || 
                         (quota.used_mb as f64 / quota.size_mb as f64) > 0.95;
        
        if is_exceeded {
            tracing::warn!(
                "Volume {} quota exceeded: {}/{}MB used",
                volume_id,
                quota.used_mb,
                quota.size_mb
            );
        }
        
        Ok(is_exceeded)
    }

    /// Calculate directory size recursively
    #[allow(dead_code)]
    async fn calculate_directory_size(
        &self,
        path: &Path,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let mut total_size = 0u64;

        if !path.exists() {
            return Ok(0);
        }

        let mut entries = fs::read_dir(path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;

            if metadata.is_file() {
                total_size += metadata.len();
            } else if metadata.is_dir() {
                total_size += Box::pin(self.calculate_directory_size(&entry.path())).await?;
            }
        }

        Ok(total_size)
    }

    /// Unmount and delete volume
    pub async fn delete_volume(
        &self,
        volume_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let volume_path = self.base_path.join(volume_id);

        #[cfg(target_os = "macos")]
        {
            // Unmount disk image
            let _ = Command::new("hdiutil")
                .args(&["detach", volume_path.to_str().unwrap(), "-force"])
                .output();

            // Delete disk image files
            let dmg_path = self.base_path.join(format!("{}.dmg", volume_id));
            let sparse_path = format!("{}.sparseimage", dmg_path.to_str().unwrap());
            let _ = fs::remove_file(&sparse_path).await;
        }

        #[cfg(target_os = "linux")]
        {
            // Unmount loop device
            let _ = Command::new("umount")
                .args(&["-f", volume_path.to_str().unwrap()])
                .output();

            // Delete image file
            let img_path = self.base_path.join(format!("{}.img", volume_id));
            let _ = fs::remove_file(&img_path).await;
        }

        // Remove mount point
        if volume_path.exists() {
            fs::remove_dir_all(&volume_path).await?;
        }

        tracing::info!("Deleted volume {}", volume_id);
        Ok(())
    }

    /// Resize volume quota (if supported)
    pub async fn resize_volume(
        &self,
        volume_id: &str,
        new_size_mb: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_os = "macos")]
        {
            let volume_path = self.base_path.join(volume_id);
            let dmg_path = self.base_path.join(format!("{}.dmg.sparseimage", volume_id));

            // Unmount
            let _ = Command::new("hdiutil")
                .args(&["detach", volume_path.to_str().unwrap()])
                .output();

            // Resize
            let output = Command::new("hdiutil")
                .args(&[
                    "resize",
                    "-size",
                    &format!("{}m", new_size_mb),
                    dmg_path.to_str().unwrap(),
                ])
                .output()?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to resize disk image: {}", error).into());
            }

            // Remount
            let _ = Command::new("hdiutil")
                .args(&[
                    "attach",
                    dmg_path.to_str().unwrap(),
                    "-mountpoint",
                    volume_path.to_str().unwrap(),
                    "-nobrowse",
                ])
                .output();

            tracing::info!("Resized volume {} to {}MB", volume_id, new_size_mb);
        }

        #[cfg(target_os = "linux")]
        {
            let volume_path = self.base_path.join(volume_id);
            let img_path = self.base_path.join(format!("{}.img", volume_id));

            // Unmount
            let _ = Command::new("umount")
                .args(&[volume_path.to_str().unwrap()])
                .output();

            // Resize image file
            let output = Command::new("dd")
                .args(&[
                    "if=/dev/zero",
                    &format!("of={}", img_path.to_str().unwrap()),
                    "bs=1M",
                    &format!("count=0"),
                    &format!("seek={}", new_size_mb),
                ])
                .output()?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to resize image: {}", error).into());
            }

            // Resize filesystem
            let output = Command::new("resize2fs")
                .args(&[img_path.to_str().unwrap()])
                .output()?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to resize filesystem: {}", error).into());
            }

            // Remount
            let _ = Command::new("mount")
                .args(&[
                    "-o",
                    "loop",
                    img_path.to_str().unwrap(),
                    volume_path.to_str().unwrap(),
                ])
                .output();

            tracing::info!("Resized volume {} to {}MB", volume_id, new_size_mb);
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            return Err("Resize not supported on this platform".into());
        }

        Ok(())
    }
}
