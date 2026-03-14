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

pub use recording::{
    NormalizeOptions, ReplayContext, SessionEvent, SessionRecordEntry, SessionRecordLevel,
    SessionRecorder, SessionReplayer,
};
pub use security::{ConnectionSecurityOptions, SecurityLevel};
pub use transaction::{
    CommandBlockKind, RollbackPolicy, TxBlock, TxResult, TxStep, TxWorkflow, TxWorkflowResult,
    failed_block_rollback_summary, workflow_rollback_order,
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
}
