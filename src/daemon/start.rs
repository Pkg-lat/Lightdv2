use crate::config::config::Config;

pub async fn print_banner_async() {
    let config = Config::load("config.json")
        .expect("Failed to load config.json");
    
    let ascii_art = format!(
        r#"
      __    _       __    __      __
     / /   (_)___ _/ /_  / /_____/ /
    / /   / / __ `/ __ \/ __/ __  / 
   / /___/ / /_/ / / / / /_/ /_/ / Metro
  /_____/_/\__, /_/ /_/\__/\__,_/  Version 2
          /____/                           

Lightd Daemon v{}
(c) 2025-present Nadhi.dev
"#,
        config.get_version()
    );

    println!("{}", ascii_art);
}

/// Check storage
/// It's dumb to start lightd and have these js not work.
pub async fn check_storage() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load("config.json")?;
    
    let paths = vec![
        &config.storage.base_path,
        &config.storage.containers_path,
        &config.storage.volumes_path,
    ];
    
    for path in paths {
        let path_buf = std::path::Path::new(path);
        tracing::info!("Trying for {}", path);
        
        // Check if path exists
        if !path_buf.exists() {
            tracing::info!("Creating storage directory: {}", path);
            tokio::fs::create_dir_all(path).await?;
        }
        
        // Check if path is a directory
        if !path_buf.is_dir() {
            tracing::error!("Storage path is not a directory: {}", path);
            return Err(format!("Storage path is not a directory: {}", path).into());
        }
        
        // Check if path is readable and writable
        let metadata = tokio::fs::metadata(path).await?;
        if metadata.permissions().readonly() {
            tracing::error!("Storage path is not writable: {}", path);
            return Err(format!("Storage path is not writable: {}", path).into());
        }
        
        tracing::info!("Storage path ready: {}", path);

    }
    
    Ok(())
}
