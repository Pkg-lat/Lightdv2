//! SFTP session handler with chroot isolation
//! 
//! Handles SFTP sessions with path isolation to container volumes

use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use russh::server::{Auth, Handler, Msg, Session};
use russh::{Channel, ChannelId, CryptoVec};
use russh_keys::key::PublicKey;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};
use std::path::PathBuf;
use std::sync::Arc;

use super::credentials::CredentialsManager;
use super::protocol::SftpProtocol;

pub struct SftpSession {
    pub username: Option<String>,
    pub volume_path: Option<PathBuf>,
    pub credentials_manager: Arc<CredentialsManager>,
    pub base_volumes_path: String,
    pub sftp_protocol: Option<Arc<SftpProtocol>>,
}

impl SftpSession {
    pub fn new(credentials_manager: Arc<CredentialsManager>, base_volumes_path: String) -> Self {
        Self {
            username: None,
            volume_path: None,
            credentials_manager,
            base_volumes_path,
            sftp_protocol: None,
        }
    }
    
    /// Parse SFTP packet
    fn parse_sftp_packet<'a>(&self, data: &'a [u8]) -> Option<(u8, u32, &'a [u8])> {
        if data.len() < 5 {
            return None;
        }
        
        let packet_type = data[0];
        let request_id = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
        let payload = &data[5..];
        
        Some((packet_type, request_id, payload))
    }
    
    /// Send SFTP status response
    async fn send_status(
        &self,
        session: &mut Session,
        channel: ChannelId,
        request_id: u32,
        code: StatusCode,
        message: &str,
    ) {
        let mut response = BytesMut::new();
        response.put_u8(101); // SSH_FXP_STATUS
        response.put_u32(request_id);
        response.put_u32(code as u32);
        response.put_u32(message.len() as u32);
        response.put_slice(message.as_bytes());
        response.put_u32(0); // language tag (empty)
        
        let mut packet = BytesMut::new();
        packet.put_u32(response.len() as u32);
        packet.put_slice(&response);
        
        let _ = session.data(channel, CryptoVec::from_slice(&packet));
    }
    
    /// Send SFTP handle response
    async fn send_handle(
        &self,
        session: &mut Session,
        channel: ChannelId,
        request_id: u32,
        handle: &str,
    ) {
        let mut response = BytesMut::new();
        response.put_u8(102); // SSH_FXP_HANDLE
        response.put_u32(request_id);
        response.put_u32(handle.len() as u32);
        response.put_slice(handle.as_bytes());
        
        let mut packet = BytesMut::new();
        packet.put_u32(response.len() as u32);
        packet.put_slice(&response);
        
        let _ = session.data(channel, CryptoVec::from_slice(&packet));
    }
    
    /// Send SFTP data response
    async fn send_data(
        &self,
        session: &mut Session,
        channel: ChannelId,
        request_id: u32,
        data: &[u8],
    ) {
        let mut response = BytesMut::new();
        response.put_u8(103); // SSH_FXP_DATA
        response.put_u32(request_id);
        response.put_u32(data.len() as u32);
        response.put_slice(data);
        
        let mut packet = BytesMut::new();
        packet.put_u32(response.len() as u32);
        packet.put_slice(&response);
        
        let _ = session.data(channel, CryptoVec::from_slice(&packet));
    }
    
    /// Format Unix-style longname from attributes (e.g. "drwxr-xr-x" or "-rw-r--r--")
    fn format_longname(name: &str, attrs: &FileAttributes) -> String {
        let perms = attrs.permissions.unwrap_or(0o100644);
        let file_type = if (perms & 0o170000) == 0o040000 { 'd' } else { '-' };
        let mode = perms & 0o777;
        let rwx = |m: u32, r: u32, w: u32, x: u32| {
            format!(
                "{}{}{}",
                if (m & r) != 0 { 'r' } else { '-' },
                if (m & w) != 0 { 'w' } else { '-' },
                if (m & x) != 0 { 'x' } else { '-' },
            )
        };
        let perm_str = format!(
            "{}{}{}",
            rwx(mode >> 6, 4, 2, 1),
            rwx(mode >> 3, 4, 2, 1),
            rwx(mode, 4, 2, 1),
        );
        let size = attrs.size.unwrap_or(0);
        format!("{}rwxr-xr-x 1 user user {} Jan 1 00:00 {}", 
            file_type, size, name)
            .replace("rwxr-xr-x", &perm_str)
    }
    
    /// Send SFTP name response
    async fn send_name(
        &self,
        session: &mut Session,
        channel: ChannelId,
        request_id: u32,
        entries: Vec<(String, FileAttributes)>,
    ) {
        let mut response = BytesMut::new();
        response.put_u8(104); // SSH_FXP_NAME
        response.put_u32(request_id);
        response.put_u32(entries.len() as u32);
        
        for (name, attrs) in entries {
            response.put_u32(name.len() as u32);
            response.put_slice(name.as_bytes());
            
            let longname = Self::format_longname(&name, &attrs);
            response.put_u32(longname.len() as u32);
            response.put_slice(longname.as_bytes());
            
            // Attributes: include size and permissions so clients can show them
            let mut flags = 0u32;
            if attrs.size.is_some() {
                flags |= 0x00000001; // SSH_FILEXFER_ATTR_SIZE
            }
            if attrs.permissions.is_some() {
                flags |= 0x00000004; // SSH_FILEXFER_ATTR_PERMISSIONS
            }
            response.put_u32(flags);
            if let Some(size) = attrs.size {
                response.put_u64(size);
            }
            if let Some(perms) = attrs.permissions {
                response.put_u32(perms);
            }
        }
        
        let mut packet = BytesMut::new();
        packet.put_u32(response.len() as u32);
        packet.put_slice(&response);
        
        let _ = session.data(channel, CryptoVec::from_slice(&packet));
    }
    
    /// Send SFTP attrs response
    async fn send_attrs(
        &self,
        session: &mut Session,
        channel: ChannelId,
        request_id: u32,
        attrs: FileAttributes,
    ) {
        let mut response = BytesMut::new();
        response.put_u8(105); // SSH_FXP_ATTRS
        response.put_u32(request_id);
        
        // Flags
        let mut flags = 0u32;
        if attrs.size.is_some() {
            flags |= 0x00000001; // SSH_FILEXFER_ATTR_SIZE
        }
        if attrs.permissions.is_some() {
            flags |= 0x00000004; // SSH_FILEXFER_ATTR_PERMISSIONS
        }
        response.put_u32(flags);
        
        // Size
        if let Some(size) = attrs.size {
            response.put_u64(size);
        }
        
        // Permissions
        if let Some(perms) = attrs.permissions {
            response.put_u32(perms);
        }
        
        let mut packet = BytesMut::new();
        packet.put_u32(response.len() as u32);
        packet.put_slice(&response);
        
        let _ = session.data(channel, CryptoVec::from_slice(&packet));
    }
}

#[async_trait]
impl Handler for SftpSession {
    type Error = anyhow::Error;

    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<Auth, Self::Error> {
        tracing::info!("SFTP auth attempt for user: {}", user);
        
        // Verify credentials
        match self.credentials_manager.verify_credentials(user, password) {
            Ok(Some(creds)) => {
                tracing::info!("SFTP auth successful for user: {}", user);
                
                // Set volume path for this session
                let volume_path = PathBuf::from(&self.base_volumes_path).join(&creds.volume_id);
                self.volume_path = Some(volume_path.clone());
                self.username = Some(user.to_string());
                
                // Initialize SFTP protocol handler
                self.sftp_protocol = Some(Arc::new(SftpProtocol::new(volume_path)));
                
                Ok(Auth::Accept)
            }
            Ok(None) => {
                tracing::warn!("SFTP auth failed for user: {}", user);
                Ok(Auth::Reject {
                    proceed_with_methods: None,
                })
            }
            Err(e) => {
                tracing::error!("SFTP auth error for user {}: {}", user, e);
                Ok(Auth::Reject {
                    proceed_with_methods: None,
                })
            }
        }
    }

    async fn auth_publickey(
        &mut self,
        _user: &str,
        _public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        // Public key auth not implemented yet
        Ok(Auth::Reject {
            proceed_with_methods: None,
        })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        tracing::debug!("Channel open session request");
        // Confirm channel open
        let _ = session.channel_success(channel.id());
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" {
            tracing::info!("SFTP subsystem requested for channel {}", channel_id);
            
            // Confirm subsystem request
            let _ = session.channel_success(channel_id);
            
            // Send SFTP version (SSH_FXP_VERSION)
            let mut response = BytesMut::new();
            response.put_u8(2); // SSH_FXP_VERSION
            response.put_u32(3); // SFTP protocol version 3
            
            let mut packet = BytesMut::new();
            packet.put_u32(response.len() as u32);
            packet.put_slice(&response);
            
            let _ = session.data(channel_id, CryptoVec::from_slice(&packet));
            
            Ok(())
        } else {
            tracing::warn!("Unsupported subsystem requested: {}", name);
            let _ = session.channel_failure(channel_id);
            Err(anyhow::anyhow!("Unsupported subsystem: {}", name))
        }
    }
    
    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        tracing::debug!("Channel {} EOF", channel);
        Ok(())
    }
    
    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        tracing::debug!("Channel {} closed", channel);
        Ok(())
    }
    
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Parse SFTP packet
        if data.len() < 5 {
            return Ok(());
        }
        
        let packet_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 4 + packet_len {
            return Ok(());
        }
        
        let packet_data = &data[4..4 + packet_len];
        if packet_data.is_empty() {
            return Ok(());
        }
        
        let packet_type = packet_data[0];
        
        tracing::debug!("SFTP packet type: {} (len: {})", packet_type, packet_data.len());
        
        // Handle SFTP protocol messages
        let protocol = match &self.sftp_protocol {
            Some(p) => p.clone(),
            None => {
                tracing::error!("SFTP protocol not initialized");
                return Ok(());
            }
        };
        
        match packet_type {
            1 => {
                // SSH_FXP_INIT - already handled in subsystem_request
                tracing::debug!("Received SSH_FXP_INIT");
            }
            3 => {
                // SSH_FXP_OPEN: request_id, filename (string), pflags (uint32), attrs
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let filename_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + filename_len + 4 {
                            let filename = String::from_utf8_lossy(&packet_data[9..9 + filename_len]).to_string();
                            let pflags = u32::from_be_bytes([
                                packet_data[9 + filename_len],
                                packet_data[10 + filename_len],
                                packet_data[11 + filename_len],
                                packet_data[12 + filename_len],
                            ]);
                            let flags = OpenFlags::from_bits_truncate(pflags);
                            // If no access mode specified, default to read (common for downloads)
                            let flags = if !flags.contains(OpenFlags::READ) && !flags.contains(OpenFlags::WRITE) {
                                flags | OpenFlags::READ
                            } else {
                                flags
                            };
                            
                            match protocol.handle_open(&filename, flags).await {
                                Ok(handle) => {
                                    self.send_handle(session, channel, request_id, &handle).await;
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            4 => {
                // SSH_FXP_CLOSE
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let handle_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + handle_len {
                            let handle = String::from_utf8_lossy(&packet_data[9..9 + handle_len]).to_string();
                            
                            match protocol.handle_close(&handle).await {
                                Ok(_) => {
                                    self.send_status(session, channel, request_id, StatusCode::Ok, "OK").await;
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            11 => {
                // SSH_FXP_OPENDIR
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let path_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + path_len {
                            let path = String::from_utf8_lossy(&packet_data[9..9 + path_len]).to_string();
                            
                            match protocol.handle_opendir(&path).await {
                                Ok(handle) => {
                                    self.send_handle(session, channel, request_id, &handle).await;
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            12 => {
                // SSH_FXP_READDIR
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let handle_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + handle_len {
                            let handle = String::from_utf8_lossy(&packet_data[9..9 + handle_len]).to_string();
                            
                            match protocol.handle_readdir(&handle).await {
                                Ok(entries) => {
                                    self.send_name(session, channel, request_id, entries).await;
                                }
                                Err(_) => {
                                    // EOF
                                    self.send_status(session, channel, request_id, StatusCode::Eof, "EOF").await;
                                }
                            }
                        }
                    }
                }
            }
            16 => {
                // SSH_FXP_REALPATH
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let path_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + path_len {
                            let path = String::from_utf8_lossy(&packet_data[9..9 + path_len]).to_string();
                            
                            match protocol.handle_realpath(&path).await {
                                Ok(real_path) => {
                                    // Send as NAME response with single entry
                                    let attrs = FileAttributes::default();
                                    self.send_name(session, channel, request_id, vec![(real_path, attrs)]).await;
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            7 => {
                // SSH_FXP_STAT (follows symlinks)
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let path_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + path_len {
                            let path = String::from_utf8_lossy(&packet_data[9..9 + path_len]).to_string();
                            
                            tracing::debug!("STAT request for path: {}", path);
                            
                            match protocol.handle_stat(&path).await {
                                Ok(attrs) => {
                                    tracing::debug!("STAT success: size={:?}, perms={:?}", attrs.size, attrs.permissions);
                                    self.send_attrs(session, channel, request_id, attrs).await;
                                }
                                Err(e) => {
                                    tracing::warn!("STAT failed for {}: {}", path, e);
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            8 => {
                // SSH_FXP_LSTAT (doesn't follow symlinks)
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let path_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + path_len {
                            let path = String::from_utf8_lossy(&packet_data[9..9 + path_len]).to_string();
                            
                            tracing::debug!("LSTAT request for path: {}", path);
                            
                            match protocol.handle_lstat(&path).await {
                                Ok(attrs) => {
                                    tracing::debug!("LSTAT success: size={:?}, perms={:?}", attrs.size, attrs.permissions);
                                    self.send_attrs(session, channel, request_id, attrs).await;
                                }
                                Err(e) => {
                                    tracing::warn!("LSTAT failed for {}: {}", path, e);
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            9 => {
                // SSH_FXP_FSTAT (stat on file handle)
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let handle_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + handle_len {
                            let handle = String::from_utf8_lossy(&packet_data[9..9 + handle_len]).to_string();
                            
                            tracing::debug!("FSTAT request for handle: {}", handle);
                            
                            match protocol.handle_fstat(&handle).await {
                                Ok(attrs) => {
                                    tracing::debug!("FSTAT success: size={:?}, perms={:?}", attrs.size, attrs.permissions);
                                    self.send_attrs(session, channel, request_id, attrs).await;
                                }
                                Err(e) => {
                                    tracing::warn!("FSTAT failed for handle {}: {}", handle, e);
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            10 => {
                // SSH_FXP_SETSTAT (set file attributes)
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    // For now, just return OK (ignore attribute changes)
                    self.send_status(session, channel, request_id, StatusCode::Ok, "OK").await;
                }
            }
            17 => {
                // SSH_FXP_SYMLINK or SSH_FXP_LINK
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    // Symlinks not supported yet
                    self.send_status(session, channel, request_id, StatusCode::OpUnsupported, "Symlinks not supported").await;
                }
            }
            14 => {
                // SSH_FXP_MKDIR
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let path_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + path_len {
                            let path = String::from_utf8_lossy(&packet_data[9..9 + path_len]).to_string();
                            
                            match protocol.handle_mkdir(&path).await {
                                Ok(_) => {
                                    self.send_status(session, channel, request_id, StatusCode::Ok, "OK").await;
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            15 => {
                // SSH_FXP_RMDIR
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let path_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + path_len {
                            let path = String::from_utf8_lossy(&packet_data[9..9 + path_len]).to_string();
                            
                            match protocol.handle_rmdir(&path).await {
                                Ok(_) => {
                                    self.send_status(session, channel, request_id, StatusCode::Ok, "OK").await;
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            13 => {
                // SSH_FXP_REMOVE
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let path_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + path_len {
                            let path = String::from_utf8_lossy(&packet_data[9..9 + path_len]).to_string();
                            
                            match protocol.handle_remove(&path).await {
                                Ok(_) => {
                                    self.send_status(session, channel, request_id, StatusCode::Ok, "OK").await;
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            18 => {
                // SSH_FXP_RENAME
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let oldpath_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + oldpath_len + 4 {
                            let oldpath = String::from_utf8_lossy(&packet_data[9..9 + oldpath_len]).to_string();
                            
                            let newpath_len = u32::from_be_bytes([
                                packet_data[9 + oldpath_len],
                                packet_data[10 + oldpath_len],
                                packet_data[11 + oldpath_len],
                                packet_data[12 + oldpath_len],
                            ]) as usize;
                            
                            if packet_data.len() >= 13 + oldpath_len + newpath_len {
                                let newpath = String::from_utf8_lossy(&packet_data[13 + oldpath_len..13 + oldpath_len + newpath_len]).to_string();
                                
                                match protocol.handle_rename(&oldpath, &newpath).await {
                                    Ok(_) => {
                                        self.send_status(session, channel, request_id, StatusCode::Ok, "OK").await;
                                    }
                                    Err(e) => {
                                        self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            5 => {
                // SSH_FXP_READ
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let handle_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + handle_len + 12 {
                            let handle = String::from_utf8_lossy(&packet_data[9..9 + handle_len]).to_string();
                            
                            let offset = u64::from_be_bytes([
                                packet_data[9 + handle_len],
                                packet_data[10 + handle_len],
                                packet_data[11 + handle_len],
                                packet_data[12 + handle_len],
                                packet_data[13 + handle_len],
                                packet_data[14 + handle_len],
                                packet_data[15 + handle_len],
                                packet_data[16 + handle_len],
                            ]);
                            
                            let len = u32::from_be_bytes([
                                packet_data[17 + handle_len],
                                packet_data[18 + handle_len],
                                packet_data[19 + handle_len],
                                packet_data[20 + handle_len],
                            ]);
                            
                            match protocol.handle_read(&handle, offset, len).await {
                                Ok(data) => {
                                    if data.is_empty() {
                                        self.send_status(session, channel, request_id, StatusCode::Eof, "EOF").await;
                                    } else {
                                        self.send_data(session, channel, request_id, &data).await;
                                    }
                                }
                                Err(e) => {
                                    self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                }
                            }
                        }
                    }
                }
            }
            6 => {
                // SSH_FXP_WRITE
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1], packet_data[2], packet_data[3], packet_data[4],
                    ]);
                    
                    if packet_data.len() >= 9 {
                        let handle_len = u32::from_be_bytes([
                            packet_data[5], packet_data[6], packet_data[7], packet_data[8],
                        ]) as usize;
                        
                        if packet_data.len() >= 9 + handle_len + 12 {
                            let handle = String::from_utf8_lossy(&packet_data[9..9 + handle_len]).to_string();
                            
                            let offset = u64::from_be_bytes([
                                packet_data[9 + handle_len],
                                packet_data[10 + handle_len],
                                packet_data[11 + handle_len],
                                packet_data[12 + handle_len],
                                packet_data[13 + handle_len],
                                packet_data[14 + handle_len],
                                packet_data[15 + handle_len],
                                packet_data[16 + handle_len],
                            ]);
                            
                            let data_len = u32::from_be_bytes([
                                packet_data[17 + handle_len],
                                packet_data[18 + handle_len],
                                packet_data[19 + handle_len],
                                packet_data[20 + handle_len],
                            ]) as usize;
                            
                            if packet_data.len() >= 21 + handle_len + data_len {
                                let data = &packet_data[21 + handle_len..21 + handle_len + data_len];
                                
                                match protocol.handle_write(&handle, offset, data).await {
                                    Ok(_) => {
                                        self.send_status(session, channel, request_id, StatusCode::Ok, "OK").await;
                                    }
                                    Err(e) => {
                                        self.send_status(session, channel, request_id, StatusCode::Failure, &e).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // For now, send "operation not supported" for all other operations
                if packet_data.len() >= 5 {
                    let request_id = u32::from_be_bytes([
                        packet_data[1],
                        packet_data[2],
                        packet_data[3],
                        packet_data[4],
                    ]);
                    
                    tracing::debug!("Unsupported SFTP packet type: {}", packet_type);
                    
                    self.send_status(
                        session,
                        channel,
                        request_id,
                        StatusCode::OpUnsupported,
                        &format!("Operation {} not yet implemented", packet_type),
                    ).await;
                }
            }
        }
        
        Ok(())
    }
}
