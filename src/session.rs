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
///
/// This is the main entry point for obtaining SSH connections. It automatically
/// manages connection pooling, caching, and lifecycle.
///
/// # Example
///
/// ```rust,no_run
/// use rneter::session::MANAGER;
/// # async fn example() {
/// let sender = MANAGER.get(
///     "admin".to_string(),
///     "192.168.1.1".to_string(),
///     22,
///     "password".to_string(),
///     None,
///     handler,
/// ).await.unwrap();
/// # }
/// ```
pub static MANAGER: Lazy<SshConnectionManager> = Lazy::new(SshConnectionManager::new);

/// A shared SSH client instance with state machine tracking.
///
/// This struct wraps an SSH client connection and integrates it with a
/// device handler for intelligent state management and command execution.
pub struct SharedSshClient {
    client: Client,
    sender: Sender<String>,
    recv: Receiver<String>,
    handler: DeviceHandler,
    enable_password: Option<String>,
    prompt: String,
}

/// Configuration for a command to execute on a device.
///
/// This struct defines all parameters needed to execute a command,
/// including the target device mode and timeout settings.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Command {
    /// Type of command (e.g., "show", "config")
    pub cmd_type: String,

    /// Target device mode/state for command execution
    pub mode: String,

    /// The actual command string to execute
    pub command: String,

    /// Optional template for command output parsing
    pub template: String,

    /// Timeout in seconds for command execution
    pub timeout: u64,
}

/// A job representing a command execution request.
///
/// This struct is used internally for passing command execution requests
/// through async channels with a oneshot responder for the result.
pub struct CmdJob {
    /// The command to execute
    pub data: Command,

    /// Optional system name for state-specific command execution
    pub sys: Option<String>,

    /// Oneshot channel sender for returning the execution result
    pub responder: oneshot::Sender<Result<Output, ConnectError>>,
}

/// The output result of a command execution.
///
/// Contains both the cleaned command output and execution status.
pub struct Output {
    /// Whether the command executed successfully (no errors detected)
    pub success: bool,

    /// Cleaned output content with prompt and echo removed
    pub content: String,

    /// Complete raw output including prompts and echoed command
    pub all: String,
}

/// SSH connection pool manager.
///
/// Manages a cache of SSH connections with automatic reconnection and
/// connection pooling. Connections are cached for 5 minutes of inactivity.
///
/// # Example
///
/// ```rust,no_run
/// use rneter::session::SshConnectionManager;
///
/// # async fn example() {
/// let manager = SshConnectionManager::new();
/// let sender = manager.get(
///     "admin".to_string(),
///     "192.168.1.1".to_string(),
///     22,
///     "password".to_string(),
///     None, // No enable password
///     handler,
/// ).await.unwrap();
/// # }
/// ```
#[derive(Clone)]
pub struct SshConnectionManager {
    cache: Cache<String, (mpsc::Sender<CmdJob>, Arc<RwLock<SharedSshClient>>)>,
}

impl SshConnectionManager {
    /// Creates a new SSH connection manager.
    ///
    /// The manager caches up to 100 connections and automatically evicts
    /// connections that have been idle for more than 5 minutes.
    pub fn new() -> Self {
        // Cache up to 100 connections, evict after 5 minutes of inactivity
        let cache = Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(5 * 60)) // Evict after 5 minutes idle
            .build();

        Self { cache }
    }

    /// Gets a cached SSH client or creates a new one.
    ///
    /// This method first checks the cache for an existing healthy connection.
    /// If found and the connection is still active, it returns the sender for
    /// that connection. Otherwise, it creates a new connection, caches it, and
    /// returns a sender.
    ///
    /// # Arguments
    ///
    /// * `user` - SSH username
    /// * `addr` - Device IP address or hostname
    /// * `port` - SSH port (typically 22)
    /// * `password` - SSH password
    /// * `enable_password` - Optional enable/privileged mode password
    /// * `handler` - Device state machine handler
    ///
    /// # Returns
    ///
    /// A sender channel for submitting command jobs to the connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH connection cannot be established.
    pub async fn get(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        let device_addr = format!("{user}@{addr}:{port}");

        // Check if a healthy, usable connection exists in the cache
        if let Some((sender, client)) = self.cache.get(&device_addr).await {
            debug!("Cache hit: {}", device_addr);
            if client.read().await.is_connected() {
                return Ok(sender);
            } else {
                // If the connection is closed, remove it from the cache
                debug!("Cached connection {} is closed. Removing.", device_addr);
                self.cache.invalidate(&device_addr).await;
            }
        } else {
            debug!("Cache miss, creating new connection for {}...", device_addr);
        }

        // Create a new client. The `new` function now automatically detects prompts
        // and ensures the shell is ready
        let ssh_client =
            SharedSshClient::new(user, addr, port, password, enable_password, handler).await?;
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
                        client_guard
                            .write_with_mode(&job.data.command, &job.data.mode, job.sys.as_ref())
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
}

impl Default for SshConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSshClient {
    /// Creates a new SSH client connection.
    ///
    /// Establishes an SSH connection, sets up a PTY, starts a shell, and waits
    /// for the initial prompt to appear. This ensures the connection is fully
    /// ready before returning.
    ///
    /// # Arguments
    ///
    /// * `user` - SSH username
    /// * `addr` - Device IP address or hostname  
    /// * `port` - SSH port
    /// * `password` - SSH password
    /// * `enable_password` - Optional enable password for privileged mode
    /// * `handler` - Device state machine handler
    ///
    /// # Returns
    ///
    /// A ready-to-use `SharedSshClient` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails or the initial prompt is not detected
    /// within 60 seconds.
    async fn new(
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Result<SharedSshClient, ConnectError> {
        let device_addr = format!("{user}@{addr}:{port}");

        let config = Config {
            preferred: Preferred {
                kex: Cow::Borrowed(config::ALL_KEX_ORDER),
                key: Cow::Borrowed(&config::ALL_KEY_TYPES),
                cipher: Cow::Borrowed(config::ALL_CIPHERS),
                mac: Cow::Borrowed(config::ALL_MAC_ALGORITHMS),
                compression: Cow::Borrowed(config::ALL_COMPRESSION_ALGORITHMS),
            },
            inactivity_timeout: Some(Duration::from_secs(60)),
            ..Default::default()
        };

        let client = Client::connect_with_config(
            (addr, port),
            &user,
            AuthMethod::with_password(&password),
            ServerCheckMethod::NoCheck,
            config,
        )
        .await?;
        debug!("{}  TCP connection successful", device_addr);

        let mut channel = client.get_channel().await?;
        channel
            .request_pty(false, "xterm", 800, 600, 0, 0, &[])
            .await?;
        channel.request_shell(false).await?;
        debug!("{}  Shell request successful", device_addr);

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
                                if let Ok(s) = std::str::from_utf8(data) {
                                    if sender_to_user.send(s.to_string()).await.is_err() {
                                        debug!("{} Shell output receiver has been dropped. Closing task.", device_addr);
                                        break;
                                    }
                                }
                            }
                            ChannelMsg::ExitStatus { exit_status } => {
                                debug!("{} Shell has exited with status code: {}", device_addr, exit_status);
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
            debug!("{} SSH I/O task has ended.", device_addr);
        });

        let mut buffer = String::new();
        let mut params = HashMap::new();
        if let Some(enable) = enable_password.as_ref() {
            params.insert("EnablePassword".to_string(), format!("{}\n", enable));
        }
        let mut prompt = String::new();

        let mut handler = handler;

        handler.dyn_param = params;

        // Wait for prompt output
        let _ = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(data) = receiver_from_shell.recv().await {
                    trace!("{:?}", data);
                    buffer.push_str(&data);

                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer.drain(..=newline_pos).collect::<String>();
                        let trimmed_line = line.trim_end();

                        handler.read(trimmed_line);
                    }

                    if !buffer.is_empty() {
                        if handler.read_prompt(&buffer) {
                            prompt.push_str(&buffer);
                            handler.read(&buffer);
                            return Ok(());
                        }
                        if let Some((c, _)) = handler.read_need_write(&buffer) {
                            handler.read(&buffer);
                            sender_to_shell.send(c).await?;
                        }
                    }
                } else {
                    // Channel has closed
                    return Err(ConnectError::ChannelDisconnectError);
                }
            }
        })
        .await;

        Ok(Self {
            client,
            sender: sender_to_shell,
            recv: receiver_from_shell,
            handler,
            prompt,
            enable_password,
        })
    }

    /// Checks if the underlying SSH client is still connected.
    ///
    /// # Returns
    ///
    /// `true` if the connection is active, `false` if it has been closed.
    pub fn is_connected(&self) -> bool {
        !self.client.is_closed()
    }

    /// Executes a command and waits for its complete output by matching the prompt.
    ///
    /// This method sends a command to the device, reads the output line by line,
    /// and waits until the prompt is detected, indicating the command has completed.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute
    ///
    /// # Returns
    ///
    /// An `Output` containing the command results.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The channel is disconnected
    /// - The command times out (60 seconds)
    /// - An error state is detected
    pub async fn write(&mut self, command: &str) -> Result<Output, ConnectError> {
        let Self {
            recv,
            prompt,
            .. // ignore other fields like `client`
        } = self;

        // 1. Clear any residual data from the receiver
        while recv.try_recv().is_ok() {}

        // 2. Send the command to the remote shell
        let full_command = format!("{}\n", command);

        self.sender.send(full_command).await?;

        // 3. Receive data
        let mut clean_output = String::new();
        let mut line_buffer = String::new(); // Buffer for assembling data into complete lines
        let command_timeout = Duration::from_secs(60); // Overall timeout for the command

        let mut line = String::new();

        let result = tokio::time::timeout(command_timeout, async {
            let mut is_error = false;
            loop {
                if let Some(data) = recv.recv().await {
                    // trace!("{:?}", data);
                    line_buffer.push_str(&data);

                    while let Some(newline_pos) = line_buffer.find('\n') {
                        line.clear(); // Clear buffer for reuse
                        line.extend(line_buffer.drain(..=newline_pos));
                        let trim_start = IGNORE_START_LINE.replace(&line, "");
                        let trimmed_line = trim_start.trim_end();

                        self.handler.read(trimmed_line);

                        if self.handler.error() {
                            is_error = true;
                        }

                        clean_output.push_str(&trim_start);
                    }

                    // Stage 2: Check remaining incomplete line in line_buffer (likely the prompt)
                    // This check is crucial for handling prompts without newlines
                    if !line_buffer.is_empty() {
                        if self.handler.read_prompt(&line_buffer) {
                            self.handler.read(&line_buffer);
                            clean_output.push_str(&line_buffer);
                            *prompt = line_buffer;
                            if is_error {
                                return Ok(false);
                            }
                            return Ok(true);
                        }
                        if let Some((c, is_record)) = self.handler.read_need_write(&line_buffer) {
                            self.handler.read(&line_buffer);
                            if !is_record {
                                line_buffer.clear();
                            }
                            trace!("Input required: '{:?}'", c);
                            self.sender.send(c).await?;
                        }
                    }
                } else {
                    // Channel has closed
                    return Err(ConnectError::ChannelDisconnectError);
                }
            }
        })
        .await;

        if result.is_err() {
            return Err(ConnectError::ExecTimeout(clean_output));
        }

        let success;

        match result.unwrap() {
            Ok(b) => success = b,
            Err(err) => {
                return Err(err);
            }
        }

        let all = clean_output;

        let mut content = all.as_str();

        if !command.is_empty() && content.starts_with(command) {
            content = content
                .strip_prefix(command)
                .unwrap_or(content)
                .trim_start_matches(|c| c == '\n' || c == '\r');
        }

        let content = if let Some(pos) = content.rfind('\n') {
            &content[..pos]
        } else {
            ""
        };

        Ok(Output {
            success: success,
            content: content.to_string(),
            all: all,
        })
    }

    /// Executes a command in a specific device mode.
    ///
    /// This method handles automatic state transitions before executing the command.
    /// If the device is not in the target mode, it will automatically execute the
    /// necessary commands to transition to that mode.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute
    /// * `mode` - The target device mode/state
    /// * `sys` - Optional system name for system-specific states
    ///
    /// # Returns
    ///
    /// An `Output` containing the combined output of all transition commands
    /// and the final command.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - State transition fails
    /// - The target state is unreachable
    /// - Command execution fails
    pub async fn write_with_mode(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
    ) -> Result<Output, ConnectError> {
        let trans_cmds = self.handler.trans_state_write(mode, sys)?;
        let mut all = self.prompt.clone();
        for (t_cmd, target_state) in trans_cmds {
            debug!("trans state command: {}", t_cmd);
            let mut mode_output = self.write(&t_cmd).await?;
            all.push_str(mode_output.all.as_str());
            if mode_output.success == false {
                mode_output.all = all;
                return Ok(mode_output);
            }

            if !self.handler.current_state().eq(&target_state) {
                mode_output.success = false;
                mode_output.all = all;
                return Ok(mode_output);
            }
        }
        let mut cmd_output = self.write(command).await?;

        all.push_str(cmd_output.all.as_str());

        cmd_output.all = all;
        Ok(cmd_output)
    }
}
