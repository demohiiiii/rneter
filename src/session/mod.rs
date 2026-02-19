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
