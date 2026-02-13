//! SSH connection management and command execution.
//!
//! This module provides connection pooling, automatic prompt detection, and
//! command execution for network devices over SSH. It manages the lifecycle
//! of SSH connections and handles device state transitions.
//!
//! # Main Components
//!
//! - [`SshConnectionManager`] - Connection pool manager (singleton via `MANAGER`)
//! - [`SharedSshClient`] - Individual SSH connection with state tracking
//! - [`Command`] - Command configuration for device execution
//! - [`Output`] - Command execution results

use async_ssh2_tokio::client::{AuthMethod, Client};
use async_ssh2_tokio::{Config, ServerCheckMethod};
use log::{debug, trace};
use moka::future::Cache;
use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};

use russh::{ChannelMsg, Preferred};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::{RwLock, oneshot};

use crate::config;
use crate::error::ConnectError;

use super::device::{DeviceHandler, IGNORE_START_LINE};

/// Global singleton SSH connection manager.
pub static MANAGER: Lazy<SshConnectionManager> = Lazy::new(SshConnectionManager::new);

/// Security level used for SSH algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SecurityLevel {
    /// Strict modern algorithms (default).
    Secure,
    /// Good security with broader compatibility.
    Balanced,
    /// Maximum compatibility with legacy devices.
    LegacyCompatible,
}

/// Connection security options for SSH establishment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSecurityOptions {
    /// SSH algorithm policy.
    pub level: SecurityLevel,
    /// Server host key verification method.
    pub server_check: ServerCheckMethod,
}

impl Default for ConnectionSecurityOptions {
    fn default() -> Self {
        Self::secure_default()
    }
}

impl ConnectionSecurityOptions {
    /// Secure-by-default profile (recommended).
    pub fn secure_default() -> Self {
        Self {
            level: SecurityLevel::Secure,
            server_check: ServerCheckMethod::DefaultKnownHostsFile,
        }
    }

    /// Balanced profile for mixed environments.
    pub fn balanced() -> Self {
        Self {
            level: SecurityLevel::Balanced,
            server_check: ServerCheckMethod::DefaultKnownHostsFile,
        }
    }

    /// Legacy compatibility profile for older devices.
    pub fn legacy_compatible() -> Self {
        Self {
            level: SecurityLevel::LegacyCompatible,
            server_check: ServerCheckMethod::NoCheck,
        }
    }

    fn preferred(&self) -> Preferred {
        match self.level {
            SecurityLevel::Secure => Preferred {
                kex: Cow::Borrowed(config::SECURE_KEX_ORDER),
                key: Cow::Borrowed(config::SECURE_KEY_TYPES),
                cipher: Cow::Borrowed(config::SECURE_CIPHERS),
                mac: Cow::Borrowed(config::SECURE_MAC_ALGORITHMS),
                compression: Cow::Borrowed(config::DEFAULT_COMPRESSION_ALGORITHMS),
            },
            SecurityLevel::Balanced => Preferred {
                kex: Cow::Borrowed(config::BALANCED_KEX_ORDER),
                key: Cow::Borrowed(config::BALANCED_KEY_TYPES),
                cipher: Cow::Borrowed(config::BALANCED_CIPHERS),
                mac: Cow::Borrowed(config::BALANCED_MAC_ALGORITHMS),
                compression: Cow::Borrowed(config::DEFAULT_COMPRESSION_ALGORITHMS),
            },
            SecurityLevel::LegacyCompatible => Preferred {
                kex: Cow::Borrowed(config::LEGACY_KEX_ORDER),
                key: Cow::Borrowed(config::LEGACY_KEY_TYPES),
                cipher: Cow::Borrowed(config::LEGACY_CIPHERS),
                mac: Cow::Borrowed(config::LEGACY_MAC_ALGORITHMS),
                compression: Cow::Borrowed(config::DEFAULT_COMPRESSION_ALGORITHMS),
            },
        }
    }
}

/// A shared SSH client instance with state machine tracking.
pub struct SharedSshClient {
    client: Client,
    sender: Sender<String>,
    recv: Receiver<String>,
    handler: Option<DeviceHandler>,
    prompt: String,

    /// SHA-256 hash of the password, used for connection parameter comparison
    password_hash: [u8; 32],

    /// SHA-256 hash of the enable password (if present)
    enable_password_hash: Option<[u8; 32]>,

    /// Initial output captured upon connection (used for device type identification)
    initial_output: Option<String>,

    /// Effective security options used when the connection was established.
    security_options: ConnectionSecurityOptions,
}

/// Configuration for a command to execute on a device.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Command {
    /// Command type - Identifies the functional category of the command
    /// Common values:
    /// - "show": Query commands, used to retrieve device status information
    /// - "config": Configuration commands, used to modify device settings
    /// - "exec": Execution commands, used to perform specific operations
    /// - "debug": Debug commands, used for troubleshooting
    pub cmd_type: String,

    /// Execution mode - Specifies the device mode in which the command should run
    /// Common values:
    /// - "Login": User mode (limited privileges)
    /// - "Enable": Privileged mode (admin privileges)
    /// - "Config": Configuration mode (for modifying settings)
    /// - Specific mode names depend on the device type and vendor
    pub mode: String,

    /// The actual command content to execute on the device
    /// Examples:
    /// - "show version" - Display device version information
    /// - "show interface status" - Display interface status
    /// - "configure terminal" - Enter configuration mode
    /// - "interface GigabitEthernet0/1" - Enter interface configuration
    pub command: String,

    /// Output parsing template - Name of the template used to structure the command output
    /// Supports TextFSM templates to convert unstructured text output into structured data
    /// Examples:
    /// - "cisco_ios_show_version" - Parse Cisco device version info
    /// - "cisco_ios_show_interface" - Parse interface status info
    /// - If empty, the raw text output is returned
    pub template: String,

    /// Single command timeout (seconds) - Maximum execution time for this command
    /// If None, defaults to 60 seconds
    /// If command execution exceeds this value, it will be forcibly terminated
    pub timeout: Option<u64>,
}

/// A job representing a command execution request.
pub struct CmdJob {
    pub data: Command,
    pub sys: Option<String>,
    /// Oneshot channel sender for returning the execution result
    pub responder: oneshot::Sender<Result<Output, ConnectError>>,
}

/// The output result of a command execution.
pub struct Output {
    pub success: bool,
    pub content: String,
    pub all: String,
}

/// SSH connection pool manager.
///
/// Manages a cache of SSH connections with automatic reconnection and
/// connection pooling. Connections are cached for 5 minutes of inactivity.
#[derive(Clone)]
pub struct SshConnectionManager {
    cache: Cache<String, (mpsc::Sender<CmdJob>, Arc<RwLock<SharedSshClient>>)>,
}

impl SshConnectionManager {
    /// Creates a new SSH connection manager.
    pub fn new() -> Self {
        // Cache up to 100 connections. Evict after 5 minutes of inactivity.
        let cache = Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(5 * 60)) // Evict after 5 minutes idle
            .build();

        Self { cache }
    }

    /// Gets a cached SSH client or creates a new one.
    ///
    /// This method first checks the cache for an existing healthy connection.
    /// If found and the connection parameters match, it reuses the connection.
    /// Otherwise, it creates a new connection, caches it, and returns the sender.
    pub async fn get(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        self.get_with_security(
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
            ConnectionSecurityOptions::default(),
        )
        .await
    }

    /// Gets a cached SSH client or creates a new one with explicit security options.
    pub async fn get_with_security(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
        security_options: ConnectionSecurityOptions,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        let device_addr = format!("{user}@{addr}:{port}");

        // Check if a healthy, usable connection exists in the cache
        if let Some((sender, client)) = self.cache.get(&device_addr).await {
            debug!("Cache hit: {}", device_addr);

            let client_guard = client.read().await;
            if client_guard.is_connected() {
                // Check if connection parameters match
                if client_guard.matches_connection_params(
                    &password,
                    &enable_password,
                    &Some(&handler),
                    &security_options,
                ) {
                    debug!("Cached connection params match, reusing: {}", device_addr);
                    return Ok(sender);
                } else {
                    debug!(
                        "Cached connection params mismatch, recreating: {}",
                        device_addr
                    );
                    // Release read lock
                    drop(client_guard);

                    // Safely disconnect the old connection
                    match self
                        .safely_disconnect_cached_connection(&device_addr, client.clone())
                        .await
                    {
                        Ok(_) => debug!("Old connection safely disconnected: {}", device_addr),
                        Err(e) => debug!(
                            "Error disconnecting old connection: {} - {}",
                            device_addr, e
                        ),
                    }

                    // Remove from cache
                    self.cache.invalidate(&device_addr).await;
                }
            } else {
                // If connection is closed, remove from cache
                debug!("Cached connection {} is closed. Removing.", device_addr);
                self.cache.invalidate(&device_addr).await;
            }
        } else {
            debug!("Cache miss, creating new connection for {}...", device_addr);
        }

        // Create a new client. `new` automatically detects prompt and ensures shell is ready.
        let ssh_client = SharedSshClient::new(
            user,
            addr,
            port,
            password,
            enable_password,
            Some(handler),
            security_options,
        )
        .await?;
        let client_arc = Arc::new(RwLock::new(ssh_client));

        let (tx, mut rx) = mpsc::channel::<CmdJob>(32);

        let client_clone = client_arc.clone();

        tokio::spawn(async move {
            loop {
                if let Some(job) = rx.recv().await {
                    if !client_clone.read().await.is_connected() {
                        let _ = job.responder.send(Err(ConnectError::ConnectClosedError));
                        break;
                    }
                    let res = {
                        let mut client_guard = client_clone.write().await;
                        let timeout = Duration::from_secs(job.data.timeout.unwrap_or(60));
                        client_guard
                            .write_with_mode_and_timeout(
                                &job.data.command,
                                &job.data.mode,
                                job.sys.as_ref(),
                                timeout,
                            )
                            .await
                    };

                    let _ = job.responder.send(res);
                }
            }
        });

        self.cache
            .insert(device_addr.clone(), (tx.clone(), client_arc))
            .await;
        debug!("New connection for {} has been cached.", device_addr);

        Ok(tx)
    }

    /// Gets the initial output from a connection.
    pub async fn get_initial_output(
        &self,
        user: String,
        addr: String,
        port: u16,
    ) -> Result<Option<String>, ConnectError> {
        let device_addr = format!("{user}@{addr}:{port}_no_handler");

        // Get connection from cache
        if let Some((_, client)) = self.cache.get(&device_addr).await {
            let client_guard = client.read().await;

            if !client_guard.is_connected() {
                return Err(ConnectError::ConnectClosedError);
            }

            Ok(client_guard.get_initial_output().cloned())
        } else {
            Err(ConnectError::InternalServerError(format!(
                "Connection not found: {}",
                device_addr
            )))
        }
    }

    /// Safely disconnects a cached connection.
    async fn safely_disconnect_cached_connection(
        &self,
        device_addr: &str,
        client_arc: Arc<RwLock<SharedSshClient>>,
    ) -> Result<(), ConnectError> {
        debug!("Safely disconnecting cached connection: {}", device_addr);

        // Get write lock to ensure exclusive access
        let mut client_guard = client_arc.write().await;

        // Check if connection is still active
        if !client_guard.is_connected() {
            debug!("Connection {} already disconnected, skipping", device_addr);
            return Ok(());
        }

        // Safely close connection
        match client_guard.close().await {
            Ok(_) => {
                debug!("Connection {} safely closed", device_addr);
                Ok(())
            }
            Err(e) => {
                debug!("Error closing connection {}: {}", device_addr, e);
                // Consider success even on error as connection will be dropped
                Ok(())
            }
        }
    }
}

impl Default for SshConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSshClient {
    /// Calculates SHA-256 hash of the password.
    fn calculate_password_hash(password: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        hasher.finalize().into()
    }

    /// Calculates SHA-256 hash of the enable password (if present).
    fn calculate_enable_password_hash(enable_password: &Option<String>) -> Option<[u8; 32]> {
        enable_password.as_ref().map(|pwd| {
            let mut hasher = Sha256::new();
            hasher.update(pwd.as_bytes());
            hasher.finalize().into()
        })
    }

    /// Checks if connection parameters match (used for cache validation).
    pub fn matches_connection_params(
        &self,
        password: &str,
        enable_password: &Option<String>,
        handler: &Option<&DeviceHandler>,
        security_options: &ConnectionSecurityOptions,
    ) -> bool {
        // Compare password hash
        let password_hash = Self::calculate_password_hash(password);
        if self.password_hash != password_hash {
            debug!("Password hash mismatch");
            return false;
        }

        // Compare enable password hash
        let enable_password_hash = Self::calculate_enable_password_hash(enable_password);
        if self.enable_password_hash != enable_password_hash {
            debug!("Enable password hash mismatch");
            return false;
        }

        // Compare handler (compare core configuration)
        match (&self.handler, handler) {
            (Some(self_handler), Some(other_handler)) => {
                if !self_handler.is_equivalent(other_handler) {
                    debug!("Device handler configuration mismatch");
                    return false;
                }
            }
            (None, None) => {
                // Both have no handler, match
            }
            _ => {
                debug!("Handler presence mismatch");
                return false;
            }
        }

        if &self.security_options != security_options {
            debug!("Security options mismatch");
            return false;
        }

        true
    }

    /// Safely closes the connection.
    pub async fn close(&mut self) -> Result<(), ConnectError> {
        debug!("Safely closing SSH connection...");

        // 1. Stop receiving new data
        self.recv.close();

        // 2. Try sending exit command (if connected)
        if self.is_connected() {
            // Send exit command to attempt graceful exit
            if let Err(e) = self.sender.send("exit\n".to_string()).await {
                debug!("Failed to send exit command: {:?}", e);
            }

            // Give some time for command execution
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // 3. Close underlying SSH client
        // async-ssh2-tokio Client currently closes automatically on drop
        // but we can explicitly call disconnect if available/needed

        debug!("SSH connection safely closed");
        Ok(())
    }

    async fn new(
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        mut handler: Option<DeviceHandler>,
        security_options: ConnectionSecurityOptions,
    ) -> Result<SharedSshClient, ConnectError> {
        let device_addr = format!("{user}@{addr}:{port}");

        let config = Config {
            preferred: security_options.preferred(),
            inactivity_timeout: Some(Duration::from_secs(60)),
            ..Default::default()
        };

        let client = Client::connect_with_config(
            (addr, port),
            &user,
            AuthMethod::with_password(&password),
            security_options.server_check.clone(),
            config,
        )
        .await?;
        debug!("{} TCP connection successful", device_addr);

        let mut channel = client.get_channel().await?;
        channel
            .request_pty(false, "xterm", 800, 600, 0, 0, &[])
            .await?;
        channel.request_shell(false).await?;
        debug!("{} Shell request successful", device_addr);

        let (sender_to_shell, mut receiver_from_user) = mpsc::channel::<String>(256);
        let (sender_to_user, mut receiver_from_shell) = mpsc::channel::<String>(256);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(data) = receiver_from_user.recv() => {
                        if let Err(e) = channel.data(data.as_bytes()).await {
                            debug!("{} Failed to send data to shell: {:?}", device_addr, e);
                            break;
                        }
                    },
                    Some(msg) = channel.wait() => {
                        match msg {
                            ChannelMsg::Data { ref data } => {
                                if let Ok(s) = std::str::from_utf8(data)
                                    && sender_to_user.send(s.to_string()).await.is_err() {
                                        debug!("{} Shell output receiver dropped. Closing task.", device_addr);
                                        break;
                                    }
                            }
                            ChannelMsg::ExitStatus { exit_status } => {
                                debug!("{} Shell exited with status code: {}", device_addr, exit_status);
                                let _ = channel.eof().await;
                                break;
                            }
                            ChannelMsg::Eof => {
                                debug!("{} Shell sent EOF.", device_addr);
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
            let _ = MANAGER.cache.invalidate(&device_addr).await;
            debug!("{} SSH I/O task ended.", device_addr);
        });

        let mut buffer = String::new();
        let mut prompt = String::new();
        let mut initial_output = String::new();

        // If handler exists, perform initial setup
        if let Some(ref mut h) = handler {
            let mut params = HashMap::new();
            if let Some(enable) = enable_password.as_ref() {
                params.insert("EnablePassword".to_string(), format!("{}\n", enable));
            }
            h.dyn_param = params;
        }

        // Wait for prompt output or collect initial output
        let _ = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(data) = receiver_from_shell.recv().await {
                    trace!("{:?}", data);
                    buffer.push_str(&data);
                    initial_output.push_str(&data);

                    // If handler exists, perform normal prompt recognition flow
                    if let Some(ref mut h) = handler {
                        while let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer.drain(..=newline_pos).collect::<String>();
                            let trimmed_line = line.trim_end();

                            h.read(trimmed_line);
                        }

                        if !buffer.is_empty() {
                            if h.read_prompt(&buffer) {
                                prompt.push_str(&buffer);
                                h.read(&buffer);
                                return Ok(());
                            }
                            if let Some((c, _)) = h.read_need_write(&buffer) {
                                h.read(&buffer);
                                sender_to_shell.send(c).await?;
                            }
                        }
                    } else {
                        // If no handler, just collect data (handled in new_without_handler scenarios)
                        while let Some(newline_pos) = buffer.find('\n') {
                            buffer.drain(..=newline_pos);
                        }
                    }
                } else {
                    // Channel closed
                    return Err(ConnectError::ChannelDisconnectError);
                }
            }
        })
        .await;

        // Calculate and store password hash
        let password_hash = Self::calculate_password_hash(&password);
        let enable_password_hash = Self::calculate_enable_password_hash(&enable_password);

        Ok(Self {
            client,
            sender: sender_to_shell,
            recv: receiver_from_shell,
            handler,
            prompt,
            password_hash,
            enable_password_hash,
            initial_output: if initial_output.is_empty() {
                None
            } else {
                Some(initial_output)
            },
            security_options,
        })
    }

    /// Gets the initial output captured during connection establishment.
    pub fn get_initial_output(&self) -> Option<&String> {
        self.initial_output.as_ref()
    }

    /// Checks if the underlying SSH connection is still active.
    pub fn is_connected(&self) -> bool {
        !self.client.is_closed()
    }

    /// Executes a command and waits for the full output by matching the prompt.
    ///
    /// Uses the default timeout of 60 seconds.
    pub async fn write(&mut self, command: &str) -> Result<Output, ConnectError> {
        self.write_with_timeout(command, Duration::from_secs(60))
            .await
    }

    /// Executes a command with a custom timeout.
    pub async fn write_with_timeout(
        &mut self,
        command: &str,
        timeout: Duration,
    ) -> Result<Output, ConnectError> {
        // Ensure handler exists
        let handler = self.handler.as_mut().ok_or_else(|| {
            ConnectError::InternalServerError("Connection handler not initialized".to_string())
        })?;

        let recv = &mut self.recv;
        let prompt = &mut self.prompt;

        // 1. Clear any residual data in the receiver
        while recv.try_recv().is_ok() {}

        // 2. Send command to remote shell
        let full_command = format!("{}\n", command);

        self.sender.send(full_command).await?;

        // 3. Receive data
        let mut clean_output = String::new();
        let mut line_buffer = String::new(); // Accumulates data into complete lines

        let mut line = String::new();

        let result = tokio::time::timeout(timeout, async {
            let mut is_error = false;
            loop {
                if let Some(data) = recv.recv().await {
                    line_buffer.push_str(&data);

                    while let Some(newline_pos) = line_buffer.find('\n') {
                        line.clear(); // Clear buffer for reuse
                        line.extend(line_buffer.drain(..=newline_pos));
                        let trim_start = IGNORE_START_LINE.replace(&line, "");
                        let trimmed_line = trim_start.trim_end();

                        handler.read(trimmed_line);

                        if handler.error() {
                            is_error = true;
                        }

                        clean_output.push_str(&trim_start);
                    }

                    // Stage 2: Check remaining incomplete line in buffer (likely the prompt)
                    // Critical for prompts without newlines
                    if !line_buffer.is_empty() {
                        if handler.read_prompt(&line_buffer) {
                            handler.read(&line_buffer);
                            clean_output.push_str(&line_buffer);
                            *prompt = line_buffer;
                            if is_error {
                                return Ok(false);
                            }
                            return Ok(true);
                        }
                        if let Some((c, is_record)) = handler.read_need_write(&line_buffer) {
                            handler.read(&line_buffer);
                            if !is_record {
                                line_buffer.clear();
                            }
                            trace!("Input required: '{:?}'", c);
                            self.sender.send(c).await?;
                        }
                    }
                } else {
                    // Channel closed
                    return Err(ConnectError::ChannelDisconnectError);
                }
            }
        })
        .await;

        if result.is_err() {
            return Err(ConnectError::ExecTimeout(clean_output));
        }

        let success = match result.unwrap() {
            Ok(b) => b,
            Err(err) => {
                return Err(err);
            }
        };

        let all = clean_output;

        let mut content = all.as_str();

        // Remove the echoed command from the beginning of the output
        if !command.is_empty() && content.starts_with(command) {
            content = content
                .strip_prefix(command)
                .unwrap_or(content)
                .trim_start_matches(['\n', '\r']);
        }

        // Remove the trailing prompt
        let content = if let Some(pos) = content.rfind('\n') {
            &content[..pos]
        } else {
            ""
        };

        Ok(Output {
            success,
            content: content.to_string(),
            all,
        })
    }

    /// Executes a command in a specific device mode.
    ///
    /// Automatically handles state transitions to reach the target mode.
    pub async fn write_with_mode(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
    ) -> Result<Output, ConnectError> {
        self.write_with_mode_and_timeout(command, mode, sys, Duration::from_secs(60))
            .await
    }

    /// Executes a command in a specific device mode with a custom timeout.
    pub async fn write_with_mode_and_timeout(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
        timeout: Duration,
    ) -> Result<Output, ConnectError> {
        // Ensure handler exists
        let handler = self.handler.as_ref().ok_or_else(|| {
            ConnectError::InternalServerError("Connection handler not initialized".to_string())
        })?;

        let temp_mode = mode.to_ascii_lowercase();
        let mode = temp_mode.as_str();

        let trans_cmds = handler.trans_state_write(mode, sys)?;
        let mut all = self.prompt.clone();

        // Execute transition commands
        for (t_cmd, target_state) in trans_cmds {
            debug!("Trans state command: {}", t_cmd);
            let mut mode_output = self.write_with_timeout(&t_cmd, timeout).await?;
            all.push_str(mode_output.all.as_str());
            if !mode_output.success {
                mode_output.all = all;
                return Ok(mode_output);
            }

            let handler = self.handler.as_ref().unwrap(); // Handler definitely exists here
            if !handler.current_state().eq(&target_state) {
                mode_output.success = false;
                mode_output.all = all;
                return Ok(mode_output);
            }
        }

        // Execute the actual command
        let mut cmd_output = self.write_with_timeout(command, timeout).await?;

        all.push_str(cmd_output.all.as_str());

        cmd_output.all = all;
        Ok(cmd_output)
    }
}
