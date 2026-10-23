//! SFTP server implementation
//! 
//! Runs SFTP server as part of lightd daemon with per-container isolation
//! Very fun stuff, but most of the time it doesn't work a lot.

use russh::server::{Config as SshConfig, run_stream};
use russh_keys::key::KeyPair;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

/// Maximum concurrent SFTP connections.
///
/// Each connection is a spawned task, and bcrypt verification is deliberately
/// expensive, so an unbounded accept loop lets a client exhaust tasks and CPU.
const MAX_SFTP_CONNECTIONS: usize = 256;

/// Maximum password verifications running at once.
///
/// bcrypt at DEFAULT_COST is ~100ms of pure CPU by design; verifying concurrently
/// without a bound turns an unauthenticated login flood into an easy CPU DoS.
const MAX_CONCURRENT_BCRYPT: usize = 4;

use super::credentials::CredentialsManager;
use super::session::SftpSession;

pub struct SftpServerManager {
    credentials_manager: Arc<CredentialsManager>,
    base_volumes_path: String,
    port: u16,
    /// Held for the lifetime of each connection.
    connections: Arc<Semaphore>,
    /// Bounds concurrent bcrypt work across all connections.
    bcrypt: Arc<Semaphore>,
}

impl SftpServerManager {
    pub fn new(
        credentials_manager: Arc<CredentialsManager>,
        base_volumes_path: String,
        port: u16,
    ) -> Self {
        Self {
            credentials_manager,
            base_volumes_path,
            port,
            connections: Arc::new(Semaphore::new(MAX_SFTP_CONNECTIONS)),
            bcrypt: Arc::new(Semaphore::new(MAX_CONCURRENT_BCRYPT)),
        }
    }
    
    /// Start SFTP server
    pub async fn start(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Generate host key
        let key_pair = KeyPair::generate_ed25519()
            .ok_or("Failed to generate host key")?;
        
        let config = SshConfig {
            inactivity_timeout: Some(std::time::Duration::from_secs(300)), // 5 minutes
            auth_rejection_time: std::time::Duration::from_secs(3),
            auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
            keys: vec![key_pair],
            window_size: 2097152, // 2MB window
            maximum_packet_size: 32768, // 32KB packets
            ..Default::default()
        };
        
        let config = Arc::new(config);
        
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await?;
        
        tracing::info!("SFTP server listening on {}", addr);
        println!("SFTP server running on port {}", self.port);
        
        loop {
            let (stream, peer_addr) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to accept SFTP connection: {}", e);
                    continue;
                }
            };
            
            // Set TCP keepalive
            if let Err(e) = stream.set_nodelay(true) {
                tracing::warn!("Failed to set TCP_NODELAY: {}", e);
            }
            
            // Refuse rather than queue indefinitely when at capacity.
            let permit = match self.connections.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    tracing::warn!(
                        "SFTP connection limit ({}) reached, rejecting {}",
                        MAX_SFTP_CONNECTIONS,
                        peer_addr
                    );
                    drop(stream);
                    continue;
                }
            };

            tracing::info!("SFTP connection from: {}", peer_addr);

            let session = SftpSession::new(
                self.credentials_manager.clone(),
                self.base_volumes_path.clone(),
                self.bcrypt.clone(),
            );
            
            let config = config.clone();
            
            tokio::spawn(async move {
                // Permit is released when this task ends.
                let _permit = permit;
                if let Err(e) = run_stream(config, stream, session).await {
                    tracing::error!("SFTP session error from {}: {}", peer_addr, e);
                }
                tracing::info!("SFTP session ended for {}", peer_addr);
            });
        }
    }
}
