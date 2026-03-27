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
//! - [`CommandFlow`] - Multi-step interactive command flow
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
    /// Extra prompt-response pairs for template-specific interactive flows.
    #[serde(default, flatten)]
    pub extra: HashMap<String, String>,
}

impl CommandDynamicParams {
    /// Returns true when no structured or extra prompt responses are set.
    pub fn is_empty(&self) -> bool {
        self.enable_password.is_none() && self.sudo_password.is_none() && self.extra.is_empty()
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

        values
    }
}

/// One runtime prompt-response rule attached directly to a command.
///
/// These rules are matched before template-defined static input rules so
/// protocol-specific workflows can inject new interactive prompts without
/// modifying the underlying device template.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PromptResponseRule {
    /// Regex patterns that identify the prompt requiring a response.
    pub patterns: Vec<String>,
    /// Raw response sent back to the remote device when a pattern matches.
    pub response: String,
    /// Whether the response-producing prompt should remain in captured output.
    #[serde(default)]
    pub record_input: bool,
}

impl PromptResponseRule {
    /// Build a prompt-response rule from regex patterns and a raw response payload.
    pub fn new(patterns: Vec<String>, response: String) -> Self {
        Self {
            patterns,
            response,
            record_input: false,
        }
    }

    /// Control whether the matched prompt should remain in captured output.
    pub fn with_record_input(mut self, record_input: bool) -> Self {
        self.record_input = record_input;
        self
    }
}

/// Runtime interactive behavior for a single command execution.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandInteraction {
    /// Prompt-response rules evaluated before template static input rules.
    #[serde(default)]
    pub prompts: Vec<PromptResponseRule>,
}

impl CommandInteraction {
    /// Returns true when the command has no runtime prompt-response rules.
    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty()
    }

    /// Append a runtime prompt-response rule.
    pub fn push_prompt(mut self, prompt: PromptResponseRule) -> Self {
        self.prompts.push(prompt);
        self
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

    /// Runtime prompt-response rules evaluated before template static input rules.
    ///
    /// Prefer this for protocol-specific interactive workflows such as `copy scp:`,
    /// `copy tftp:`, or future HTTP-style wizards that should not require template edits.
    #[serde(default)]
    pub interaction: CommandInteraction,
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

fn default_stop_on_error() -> bool {
    true
}

/// Multi-step command flow executed sequentially on one connection.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommandFlow {
    /// Ordered list of commands executed on the same live session.
    #[serde(default)]
    pub steps: Vec<Command>,
    /// Stop immediately after the first command that reports `success = false`.
    #[serde(default = "default_stop_on_error")]
    pub stop_on_error: bool,
}

impl Default for CommandFlow {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            stop_on_error: true,
        }
    }
}

impl CommandFlow {
    /// Build a command flow from preconstructed command steps.
    pub fn new(steps: Vec<Command>) -> Self {
        Self {
            steps,
            ..Self::default()
        }
    }

    /// Override whether execution should stop after the first unsuccessful step.
    pub fn with_stop_on_error(mut self, stop_on_error: bool) -> Self {
        self.stop_on_error = stop_on_error;
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
#[derive(Debug, Clone)]
pub struct Output {
    pub success: bool,
    /// Exit code captured from shell execution when supported by the active handler.
    pub exit_code: Option<i32>,
    pub content: String,
    pub all: String,
    /// Prompt captured by the internal state machine after command execution.
    pub prompt: Option<String>,
}

/// Aggregated result for a multi-step command flow.
#[derive(Debug, Clone)]
pub struct CommandFlowOutput {
    pub success: bool,
    pub outputs: Vec<Output>,
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
        assert!(cmd.interaction.is_empty());
    }

    #[test]
    fn command_dynamic_params_collect_unknown_keys_into_extra() {
        let cmd: Command = serde_json::from_value(serde_json::json!({
            "mode": "Enable",
            "command": "show version",
            "dyn_params": {
                "EnablePassword": "enable\n",
                "SudoPassword": "sudo\n",
                "CustomPrompt": "yes\n"
            }
        }))
        .expect("deserialize command");

        assert_eq!(cmd.dyn_params.enable_password.as_deref(), Some("enable\n"));
        assert_eq!(cmd.dyn_params.sudo_password.as_deref(), Some("sudo\n"));
        assert_eq!(
            cmd.dyn_params.extra.get("CustomPrompt"),
            Some(&"yes\n".to_string())
        );
        assert_eq!(
            cmd.dyn_params.runtime_values().get("EnablePassword"),
            Some(&"enable\n".to_string())
        );
    }

    #[test]
    fn command_flow_defaults_to_stop_on_error() {
        let flow = CommandFlow::default();

        assert!(flow.steps.is_empty());
        assert!(flow.stop_on_error);
    }

    #[test]
    fn prompt_response_rule_builder_sets_recording_flag() {
        let rule =
            PromptResponseRule::new(vec![r"^Password:\s*$".to_string()], "secret\n".to_string())
                .with_record_input(true);

        assert_eq!(rule.patterns, vec![r"^Password:\s*$".to_string()]);
        assert_eq!(rule.response, "secret\n");
        assert!(rule.record_input);
    }
}
