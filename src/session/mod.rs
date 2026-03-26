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
//! - [`DeviceFileTransferRequest`] - Device-side CLI transfer configuration
//! - [`FileUploadRequest`] - SFTP upload configuration
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

pub use recording::{
    NormalizeOptions, ReplayContext, SessionEvent, SessionRecordEntry, SessionRecordLevel,
    SessionRecorder, SessionReplayer,
};
pub use security::{ConnectionSecurityOptions, SecurityLevel};
pub use transaction::{
    CommandBlockKind, RollbackPolicy, TxBlock, TxResult, TxStep, TxStepExecutionState,
    TxStepResult, TxStepRollbackState, TxWorkflow, TxWorkflowResult, failed_block_rollback_summary,
    workflow_rollback_order,
};

/// Global singleton SSH connection manager.
pub static MANAGER: Lazy<SshConnectionManager> = Lazy::new(SshConnectionManager::new);

/// Connection request describing how to reach a device and which handler to use.
pub struct ConnectionRequest {
    pub user: String,
    pub addr: String,
    pub port: u16,
    pub password: String,
    pub enable_password: Option<String>,
    pub handler: DeviceHandler,
}

impl ConnectionRequest {
    /// Build a new connection request.
    pub fn new(
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Self {
        Self {
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
        }
    }

    /// Stable cache key used by the connection manager.
    pub fn device_addr(&self) -> String {
        format!("{}@{}:{}", self.user, self.addr, self.port)
    }
}

/// Execution context shared by manager entrypoints.
#[derive(Clone, Default)]
pub struct ExecutionContext {
    /// SSH security behavior for connection establishment.
    pub security_options: ConnectionSecurityOptions,
    /// Optional system name used by templates with dynamic transitions.
    pub sys: Option<String>,
}

impl ExecutionContext {
    /// Build the default execution context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override connection security behavior.
    pub fn with_security_options(mut self, security_options: ConnectionSecurityOptions) -> Self {
        self.security_options = security_options;
        self
    }

    /// Attach the system name used during state transitions.
    pub fn with_sys(mut self, sys: Option<String>) -> Self {
        self.sys = sys;
        self
    }
}

/// A shared SSH client instance with state machine tracking.
pub struct SharedSshClient {
    client: Client,
    sender: Sender<String>,
    recv: Receiver<String>,
    handler: DeviceHandler,
    prompt: String,

    /// SHA-256 hash of the password, used for connection parameter comparison
    password_hash: [u8; 32],

    /// SHA-256 hash of the enable password (if present)
    enable_password_hash: Option<[u8; 32]>,

    /// Effective security options used when the connection was established.
    security_options: ConnectionSecurityOptions,

    /// Optional session recorder bound to this connection.
    recorder: Option<SessionRecorder>,
}

/// Structured prompt-response overrides for a single command execution.
///
/// Values are sent to the remote device as-is, so include any required trailing
/// newline when the prompt expects the response to be submitted immediately.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandDynamicParams {
    #[serde(default, alias = "EnablePassword")]
    pub enable_password: Option<String>,
    #[serde(default, alias = "SudoPassword")]
    pub sudo_password: Option<String>,
    #[serde(default, alias = "TransferRemoteHost")]
    pub transfer_remote_host: Option<String>,
    #[serde(default, alias = "TransferSourceUsername")]
    pub transfer_source_username: Option<String>,
    #[serde(default, alias = "TransferDestinationUsername")]
    pub transfer_destination_username: Option<String>,
    #[serde(default, alias = "TransferSourcePath")]
    pub transfer_source_path: Option<String>,
    #[serde(default, alias = "TransferDestinationPath")]
    pub transfer_destination_path: Option<String>,
    #[serde(default, alias = "TransferPassword")]
    pub transfer_password: Option<String>,
    #[serde(default, alias = "TransferConfirm")]
    pub transfer_confirm: Option<String>,
    #[serde(default, alias = "TransferOverwrite")]
    pub transfer_overwrite: Option<String>,
    /// Extra prompt-response pairs for template-specific interactive flows.
    #[serde(default, flatten)]
    pub extra: HashMap<String, String>,
}

impl CommandDynamicParams {
    /// Returns true when no structured or extra prompt responses are set.
    pub fn is_empty(&self) -> bool {
        self.enable_password.is_none()
            && self.sudo_password.is_none()
            && self.transfer_remote_host.is_none()
            && self.transfer_source_username.is_none()
            && self.transfer_destination_username.is_none()
            && self.transfer_source_path.is_none()
            && self.transfer_destination_path.is_none()
            && self.transfer_password.is_none()
            && self.transfer_confirm.is_none()
            && self.transfer_overwrite.is_none()
            && self.extra.is_empty()
    }

    /// Insert a template-specific prompt-response pair.
    pub fn insert_extra(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Option<String> {
        self.extra.insert(key.into(), value.into())
    }

    pub(crate) fn runtime_values(&self) -> HashMap<String, String> {
        let mut values = self.extra.clone();

        if let Some(value) = self.enable_password.as_ref() {
            values.insert("EnablePassword".to_string(), value.clone());
        }
        if let Some(value) = self.sudo_password.as_ref() {
            values.insert("SudoPassword".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_remote_host.as_ref() {
            values.insert("TransferRemoteHost".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_source_username.as_ref() {
            values.insert("TransferSourceUsername".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_destination_username.as_ref() {
            values.insert("TransferDestinationUsername".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_source_path.as_ref() {
            values.insert("TransferSourcePath".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_destination_path.as_ref() {
            values.insert("TransferDestinationPath".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_password.as_ref() {
            values.insert("TransferPassword".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_confirm.as_ref() {
            values.insert("TransferConfirm".to_string(), value.clone());
        }
        if let Some(value) = self.transfer_overwrite.as_ref() {
            values.insert("TransferOverwrite".to_string(), value.clone());
        }

        values
    }
}

/// Configuration for a command to execute on a device.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Command {
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

    /// Single command timeout (seconds) - Maximum execution time for this command
    /// If None, defaults to 60 seconds
    /// If command execution exceeds this value, it will be forcibly terminated
    pub timeout: Option<u64>,

    /// Extra dynamic prompt responses applied only to this command execution.
    ///
    /// Values should include any required trailing newline if the remote device
    /// expects the response to be submitted immediately.
    #[serde(default)]
    pub dyn_params: CommandDynamicParams,
}

/// Configuration for uploading a local file to a remote host over SFTP.
///
/// The remote SSH server must expose the `sftp` subsystem. Many Linux hosts do;
/// some network devices do not, in which case command-driven transfer workflows
/// such as `copy scp:` or `copy tftp:` may still be required instead.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileUploadRequest {
    /// Local file path on the machine running rneter.
    pub local_path: String,
    /// Destination file path on the remote host.
    pub remote_path: String,
    /// Optional SFTP operation timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Optional upload buffer size in bytes. Defaults to the upstream helper value.
    pub buffer_size: Option<usize>,
    /// Emit progress logs during upload when set.
    pub show_progress: bool,
}

impl FileUploadRequest {
    /// Build a new upload request with conservative defaults.
    pub fn new(local_path: String, remote_path: String) -> Self {
        Self {
            local_path,
            remote_path,
            timeout_secs: None,
            buffer_size: None,
            show_progress: false,
        }
    }

    /// Override the SFTP timeout in seconds.
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = Some(timeout_secs);
        self
    }

    /// Override the transfer buffer size in bytes.
    pub fn with_buffer_size(mut self, buffer_size: usize) -> Self {
        self.buffer_size = Some(buffer_size);
        self
    }

    /// Control whether progress logs should be emitted during upload.
    pub fn with_progress_reporting(mut self, show_progress: bool) -> Self {
        self.show_progress = show_progress;
        self
    }
}

fn default_transfer_mode() -> String {
    "Enable".to_string()
}

/// File transfer protocol executed by the device CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceFileTransferProtocol {
    Scp,
    Tftp,
}

/// Direction of the transfer from the device's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceFileTransferDirection {
    /// Pull a file from the external server onto the device.
    ToDevice,
    /// Push a file from the device to the external server.
    FromDevice,
}

/// High-level request for CLI-driven device file transfer workflows.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeviceFileTransferRequest {
    /// Transfer protocol used by the device.
    pub protocol: DeviceFileTransferProtocol,
    /// Whether the device is importing or exporting the file.
    pub direction: DeviceFileTransferDirection,
    /// Address or DNS name of the external SCP/TFTP server reachable from the device.
    pub server_addr: String,
    /// Path on the external SCP/TFTP server.
    pub remote_path: String,
    /// Path on the device filesystem, e.g. `flash:/image.bin`.
    pub device_path: String,
    /// Optional SCP username.
    #[serde(default)]
    pub username: Option<String>,
    /// Optional SCP password.
    #[serde(default)]
    pub password: Option<String>,
    /// Device mode used to run the transfer command. Defaults to `Enable`.
    #[serde(default = "default_transfer_mode")]
    pub mode: String,
    /// Optional transfer timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl DeviceFileTransferRequest {
    /// Build a new CLI transfer request with an `Enable`-mode default.
    pub fn new(
        protocol: DeviceFileTransferProtocol,
        direction: DeviceFileTransferDirection,
        server_addr: String,
        remote_path: String,
        device_path: String,
    ) -> Self {
        Self {
            protocol,
            direction,
            server_addr,
            remote_path,
            device_path,
            username: None,
            password: None,
            mode: default_transfer_mode(),
            timeout_secs: None,
        }
    }

    /// Attach SCP credentials for the transfer.
    pub fn with_credentials(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    /// Override the device mode used to run the transfer command.
    pub fn with_mode(mut self, mode: String) -> Self {
        self.mode = mode;
        self
    }

    /// Override the transfer timeout in seconds.
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = Some(timeout_secs);
        self
    }
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
    /// Exit code captured from shell execution when supported by the active handler.
    pub exit_code: Option<i32>,
    pub content: String,
    pub all: String,
    /// Prompt captured by the internal state machine after command execution.
    pub prompt: Option<String>,
}

/// SSH connection pool manager.
///
/// Manages a cache of SSH connections with automatic reconnection and
/// connection pooling. Connections are cached for 5 minutes of inactivity.
#[derive(Clone)]
pub struct SshConnectionManager {
    cache: Cache<String, (mpsc::Sender<CmdJob>, Arc<RwLock<SharedSshClient>>)>,
}

mod client;
mod manager;
mod recording;
mod security;
mod transaction;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    #[test]
    fn connection_request_formats_device_addr() {
        let request = ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco().expect("template"),
        );
        assert_eq!(request.device_addr(), "admin@192.168.1.1:22");
    }

    #[test]
    fn execution_context_builder_overrides_defaults() {
        let context = ExecutionContext::new()
            .with_security_options(ConnectionSecurityOptions::legacy_compatible())
            .with_sys(Some("vsys1".to_string()));
        assert_eq!(
            context.security_options,
            ConnectionSecurityOptions::legacy_compatible()
        );
        assert_eq!(context.sys.as_deref(), Some("vsys1"));
    }

    #[test]
    fn file_upload_request_builder_overrides_defaults() {
        let upload = FileUploadRequest::new(
            "./fixtures/config.txt".to_string(),
            "/tmp/config.txt".to_string(),
        )
        .with_timeout_secs(30)
        .with_buffer_size(8192)
        .with_progress_reporting(true);

        assert_eq!(upload.local_path, "./fixtures/config.txt");
        assert_eq!(upload.remote_path, "/tmp/config.txt");
        assert_eq!(upload.timeout_secs, Some(30));
        assert_eq!(upload.buffer_size, Some(8192));
        assert!(upload.show_progress);
    }

    #[test]
    fn command_default_has_empty_dyn_params() {
        let cmd = Command::default();
        assert_eq!(cmd.timeout, None);
        assert!(cmd.mode.is_empty());
        assert!(cmd.command.is_empty());
        assert!(cmd.dyn_params.is_empty());
    }

    #[test]
    fn command_dynamic_params_accept_legacy_runtime_keys() {
        let cmd: Command = serde_json::from_value(serde_json::json!({
            "mode": "Enable",
            "command": "copy scp: flash:/image.bin",
            "dyn_params": {
                "TransferRemoteHost": "198.51.100.20\n",
                "TransferPassword": "secret\n",
                "CustomPrompt": "yes\n"
            }
        }))
        .expect("deserialize command");

        assert_eq!(
            cmd.dyn_params.transfer_remote_host.as_deref(),
            Some("198.51.100.20\n")
        );
        assert_eq!(
            cmd.dyn_params.transfer_password.as_deref(),
            Some("secret\n")
        );
        assert_eq!(
            cmd.dyn_params.extra.get("CustomPrompt"),
            Some(&"yes\n".to_string())
        );
        assert_eq!(
            cmd.dyn_params.runtime_values().get("TransferRemoteHost"),
            Some(&"198.51.100.20\n".to_string())
        );
    }

    #[test]
    fn device_file_transfer_request_builder_overrides_defaults() {
        let transfer = DeviceFileTransferRequest::new(
            DeviceFileTransferProtocol::Scp,
            DeviceFileTransferDirection::ToDevice,
            "192.0.2.10".to_string(),
            "/images/new.bin".to_string(),
            "flash:/new.bin".to_string(),
        )
        .with_credentials("backup".to_string(), "secret".to_string())
        .with_mode("Config".to_string())
        .with_timeout_secs(300);

        assert_eq!(transfer.protocol, DeviceFileTransferProtocol::Scp);
        assert_eq!(transfer.direction, DeviceFileTransferDirection::ToDevice);
        assert_eq!(transfer.server_addr, "192.0.2.10");
        assert_eq!(transfer.remote_path, "/images/new.bin");
        assert_eq!(transfer.device_path, "flash:/new.bin");
        assert_eq!(transfer.username.as_deref(), Some("backup"));
        assert_eq!(transfer.password.as_deref(), Some("secret"));
        assert_eq!(transfer.mode, "Config");
        assert_eq!(transfer.timeout_secs, Some(300));
    }
}
