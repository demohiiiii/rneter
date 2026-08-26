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
//! - [`RetryPolicy`] - Opt-in bounded reconnect and backoff behavior
//! - [`SessionOperationOutput`] - Generic execution result for any session operation
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::{RwLock, oneshot};

use crate::config;
use crate::error::ConnectError;

use super::device::{DeviceHandler, IGNORE_START_LINE};

pub use fleet::{DEFAULT_FLEET_CONCURRENCY_LIMIT, FleetExecutionResult, FleetOptions, FleetTarget};
pub(crate) use hooks::HookTrigger;
pub use hooks::{HookAction, HookFailurePolicy, SessionHooks};
pub use recording::{
    NormalizeOptions, ReplayContext, SessionEvent, SessionEventRedactor, SessionRecordEntry,
    SessionRecordLevel, SessionRecorder, SessionReplayer,
};
pub use security::{ConnectionSecurityOptions, SecurityLevel};
pub use transaction::{
    RollbackPolicy, TxBlock, TxOperationStepResult, TxResult, TxStep, TxStepExecutionState,
    TxStepResult, TxStepRollbackState, TxWorkflow, TxWorkflowResult, failed_block_rollback_summary,
    workflow_rollback_order,
};

/// Global singleton SSH connection manager.
pub static MANAGER: Lazy<SshConnectionManager> = Lazy::new(SshConnectionManager::new);

/// Character encoding used to decode terminal output received over SSH.
///
/// GB2312 and GBK are subsets of GB18030, so all three Chinese encoding
/// variants use the GB18030 decoder.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TextEncoding {
    /// UTF-8 with malformed byte sequences replaced by U+FFFD.
    #[default]
    Utf8,
    /// GB2312-compatible decoding through GB18030.
    Gb2312,
    /// GBK-compatible decoding through GB18030.
    Gbk,
    /// GB18030 decoding.
    Gb18030,
}

impl TextEncoding {
    fn decoder(self) -> encoding_rs::Decoder {
        match self {
            Self::Utf8 => encoding_rs::UTF_8,
            Self::Gb2312 | Self::Gbk | Self::Gb18030 => encoding_rs::GB18030,
        }
        .new_decoder_without_bom_handling()
    }
}

/// Authentication method used to establish an SSH session.
///
/// Marked `#[non_exhaustive]` so further methods can be added without a
/// breaking change; downstream `match` arms need a wildcard branch.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SshAuthMethod {
    /// Password authentication.
    Password(String),
    /// Private key provided inline (full OpenSSH/PEM file contents).
    PrivateKey {
        key_data: String,
        passphrase: Option<String>,
    },
    /// Private key loaded from a file at connect time.
    PrivateKeyFile {
        path: std::path::PathBuf,
        passphrase: Option<String>,
    },
    /// Private key provided inline with explicit acceptance of
    /// RUSTSEC-2023-0071 when the key is RSA.
    PrivateKeyAllowVulnerableRsa {
        key_data: String,
        passphrase: Option<String>,
    },
    /// File-backed private key with explicit acceptance of
    /// RUSTSEC-2023-0071 when the key is RSA.
    PrivateKeyFileAllowVulnerableRsa {
        path: std::path::PathBuf,
        passphrase: Option<String>,
    },
    /// Authenticate through the local ssh-agent.
    #[cfg(not(target_os = "windows"))]
    Agent,
    /// Keyboard-interactive authentication. Each server prompt that
    /// *contains* a configured prompt fragment is answered with the paired
    /// response.
    KeyboardInteractive(Vec<(String, String)>),
}

impl std::fmt::Debug for SshAuthMethod {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password(_) => formatter.write_str("Password(<redacted>)"),
            Self::PrivateKey { passphrase, .. } => formatter
                .debug_struct("PrivateKey")
                .field("key_data", &"<redacted>")
                .field("has_passphrase", &passphrase.is_some())
                .finish(),
            Self::PrivateKeyFile { path, passphrase } => formatter
                .debug_struct("PrivateKeyFile")
                .field("path", path)
                .field("has_passphrase", &passphrase.is_some())
                .finish(),
            Self::PrivateKeyAllowVulnerableRsa { passphrase, .. } => formatter
                .debug_struct("PrivateKeyAllowVulnerableRsa")
                .field("key_data", &"<redacted>")
                .field("has_passphrase", &passphrase.is_some())
                .finish(),
            Self::PrivateKeyFileAllowVulnerableRsa { path, passphrase } => formatter
                .debug_struct("PrivateKeyFileAllowVulnerableRsa")
                .field("path", path)
                .field("has_passphrase", &passphrase.is_some())
                .finish(),
            #[cfg(not(target_os = "windows"))]
            Self::Agent => formatter.write_str("Agent"),
            Self::KeyboardInteractive(responses) => formatter
                .debug_struct("KeyboardInteractive")
                .field("response_count", &responses.len())
                .finish(),
        }
    }
}

impl SshAuthMethod {
    /// Password authentication.
    pub fn password(password: impl Into<String>) -> Self {
        Self::Password(password.into())
    }

    /// Private key authentication from inline key contents.
    pub fn private_key(key_data: impl Into<String>, passphrase: Option<String>) -> Self {
        Self::PrivateKey {
            key_data: key_data.into(),
            passphrase,
        }
    }

    /// Private key authentication from a key file path.
    pub fn private_key_file(
        path: impl Into<std::path::PathBuf>,
        passphrase: Option<String>,
    ) -> Self {
        Self::PrivateKeyFile {
            path: path.into(),
            passphrase,
        }
    }

    /// Private key authentication from inline key contents with explicit
    /// acceptance of RUSTSEC-2023-0071 for RSA keys.
    ///
    /// Prefer [`Self::private_key`] with Ed25519/ECDSA keys or
    /// `SshAuthMethod::agent()` where available.
    pub fn private_key_allow_vulnerable_rsa(
        key_data: impl Into<String>,
        passphrase: Option<String>,
    ) -> Self {
        Self::PrivateKeyAllowVulnerableRsa {
            key_data: key_data.into(),
            passphrase,
        }
    }

    /// File-backed private key authentication with explicit acceptance of
    /// RUSTSEC-2023-0071 for RSA keys.
    ///
    /// Prefer [`Self::private_key_file`] with Ed25519/ECDSA keys or
    /// `SshAuthMethod::agent()` where available.
    pub fn private_key_file_allow_vulnerable_rsa(
        path: impl Into<std::path::PathBuf>,
        passphrase: Option<String>,
    ) -> Self {
        Self::PrivateKeyFileAllowVulnerableRsa {
            path: path.into(),
            passphrase,
        }
    }

    /// Authentication through the local ssh-agent.
    #[cfg(not(target_os = "windows"))]
    pub fn agent() -> Self {
        Self::Agent
    }

    /// Keyboard-interactive authentication with `(prompt fragment, response)`
    /// pairs.
    pub fn keyboard_interactive(responses: Vec<(String, String)>) -> Self {
        Self::KeyboardInteractive(responses)
    }

    fn validate_private_key_data(
        key_data: &str,
        passphrase: Option<&str>,
    ) -> Result<(), ConnectError> {
        let key = russh::keys::decode_secret_key(key_data, passphrase).map_err(|error| {
            ConnectError::InvalidSshAuth(format!("failed to decode private key: {error}"))
        })?;
        if key.algorithm().is_rsa() {
            return Err(ConnectError::InvalidSshAuth(
                "in-process RSA private-key authentication is disabled because of RUSTSEC-2023-0071; use Ed25519/ECDSA, ssh-agent, or explicitly opt in through SshAuthMethod"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Digest of the effective authentication material used for cached-
    /// connection parameter comparison.
    ///
    /// File-backed keys are read and hashed by content, while agent
    /// authentication hashes the agent's current public identities. Only the
    /// resulting digest is retained by pooled connections.
    pub(crate) async fn fingerprint(&self) -> Result<[u8; 32], ConnectError> {
        fn update_bytes(hasher: &mut Sha256, value: &[u8]) {
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value);
        }

        fn update_optional_str(hasher: &mut Sha256, value: Option<&str>) {
            match value {
                Some(value) => {
                    hasher.update([1u8]);
                    update_bytes(hasher, value.as_bytes());
                }
                None => hasher.update([0u8]),
            }
        }

        let mut hasher = Sha256::new();
        match self {
            Self::Password(password) => {
                hasher.update([0u8]);
                update_bytes(&mut hasher, password.as_bytes());
            }
            Self::PrivateKey {
                key_data,
                passphrase,
            } => {
                hasher.update([1u8]);
                update_bytes(&mut hasher, key_data.as_bytes());
                update_optional_str(&mut hasher, passphrase.as_deref());
            }
            Self::PrivateKeyFile { path, passphrase } => {
                let key_data = tokio::fs::read(path).await.map_err(|error| {
                    ConnectError::InvalidSshAuth(format!(
                        "read private key file '{}': {error}",
                        path.display()
                    ))
                })?;
                hasher.update([2u8]);
                update_bytes(&mut hasher, &key_data);
                update_optional_str(&mut hasher, passphrase.as_deref());
            }
            Self::PrivateKeyAllowVulnerableRsa {
                key_data,
                passphrase,
            } => {
                hasher.update([5u8]);
                update_bytes(&mut hasher, key_data.as_bytes());
                update_optional_str(&mut hasher, passphrase.as_deref());
            }
            Self::PrivateKeyFileAllowVulnerableRsa { path, passphrase } => {
                let key_data = tokio::fs::read(path).await.map_err(|error| {
                    ConnectError::InvalidSshAuth(format!(
                        "read private key file '{}': {error}",
                        path.display()
                    ))
                })?;
                hasher.update([6u8]);
                update_bytes(&mut hasher, &key_data);
                update_optional_str(&mut hasher, passphrase.as_deref());
            }
            #[cfg(not(target_os = "windows"))]
            Self::Agent => {
                hasher.update([3u8]);
                let mut agent = russh::keys::agent::client::AgentClient::connect_env()
                    .await
                    .map_err(|error| {
                        ConnectError::InvalidSshAuth(format!("connect to ssh-agent: {error}"))
                    })?;
                let identities = agent.request_identities().await.map_err(|error| {
                    ConnectError::InvalidSshAuth(format!("read identities from ssh-agent: {error}"))
                })?;
                let mut encoded_identities = identities
                    .into_iter()
                    .map(|identity| {
                        match identity {
                            russh::keys::agent::AgentIdentity::PublicKey { key, .. } => {
                                key.to_bytes()
                            }
                            russh::keys::agent::AgentIdentity::Certificate {
                                certificate, ..
                            } => certificate.to_bytes(),
                        }
                        .map_err(|error| {
                            ConnectError::InvalidSshAuth(format!(
                                "encode ssh-agent identity: {error}"
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                encoded_identities.sort();
                for identity in encoded_identities {
                    update_bytes(&mut hasher, &identity);
                }
            }
            Self::KeyboardInteractive(responses) => {
                hasher.update([4u8]);
                for (prompt, response) in responses {
                    update_bytes(&mut hasher, prompt.as_bytes());
                    update_bytes(&mut hasher, response.as_bytes());
                }
            }
        }
        Ok(hasher.finalize().into())
    }

    /// Maps to transport authentication after applying private-key policy.
    pub(crate) async fn to_transport(&self) -> Result<AuthMethod, ConnectError> {
        Ok(match self {
            Self::Password(password) => AuthMethod::with_password(password),
            Self::PrivateKey {
                key_data,
                passphrase,
            } => {
                Self::validate_private_key_data(key_data, passphrase.as_deref())?;
                AuthMethod::with_key(key_data, passphrase.as_deref())
            }
            Self::PrivateKeyFile { path, passphrase } => {
                let key_data = tokio::fs::read_to_string(path).await.map_err(|error| {
                    ConnectError::InvalidSshAuth(format!(
                        "failed to read private key file '{}': {error}",
                        path.display()
                    ))
                })?;
                Self::validate_private_key_data(&key_data, passphrase.as_deref())?;
                AuthMethod::with_key(&key_data, passphrase.as_deref())
            }
            Self::PrivateKeyAllowVulnerableRsa {
                key_data,
                passphrase,
            } => AuthMethod::with_key(key_data, passphrase.as_deref()),
            Self::PrivateKeyFileAllowVulnerableRsa { path, passphrase } => {
                AuthMethod::with_key_file(path, passphrase.as_deref())
            }
            #[cfg(not(target_os = "windows"))]
            Self::Agent => AuthMethod::with_agent(),
            Self::KeyboardInteractive(responses) => {
                let interactive = responses.iter().fold(
                    async_ssh2_tokio::client::AuthKeyboardInteractive::new(),
                    |interactive, (prompt, response)| interactive.with_response(prompt, response),
                );
                AuthMethod::with_keyboard_interactive(interactive)
            }
        })
    }
}

/// Lightweight request used by template autodetection before a concrete handler is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectRequest {
    pub user: String,
    pub addr: String,
    pub port: u16,
    pub auth: SshAuthMethod,
    pub output_encoding: TextEncoding,
}

impl DetectRequest {
    /// Build a new password-authenticated autodetect request.
    pub fn new(user: String, addr: String, port: u16, password: String) -> Self {
        Self::new_with_auth(user, addr, port, SshAuthMethod::Password(password))
    }

    /// Build a new autodetect request with an explicit authentication method.
    pub fn new_with_auth(user: String, addr: String, port: u16, auth: SshAuthMethod) -> Self {
        Self {
            user,
            addr,
            port,
            auth,
            output_encoding: TextEncoding::default(),
        }
    }

    /// Override the character encoding used to decode SSH terminal output.
    pub fn with_output_encoding(mut self, output_encoding: TextEncoding) -> Self {
        self.output_encoding = output_encoding;
        self
    }

    /// Stable textual device address used for diagnostics.
    pub fn device_addr(&self) -> String {
        format!("{}@{}:{}", self.user, self.addr, self.port)
    }
}

/// Connection request describing how to reach a device and which handler to use.
#[derive(Clone)]
pub struct ConnectionRequest {
    pub user: String,
    pub addr: String,
    pub port: u16,
    pub auth: SshAuthMethod,
    pub enable_password: Option<String>,
    pub handler: DeviceHandler,
    pub output_encoding: TextEncoding,
}

impl ConnectionRequest {
    /// Build a new password-authenticated connection request.
    pub fn new(
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Self {
        Self::new_with_auth(
            user,
            addr,
            port,
            SshAuthMethod::Password(password),
            enable_password,
            handler,
        )
    }

    /// Build a new connection request with an explicit authentication method.
    pub fn new_with_auth(
        user: String,
        addr: String,
        port: u16,
        auth: SshAuthMethod,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Self {
        Self {
            user,
            addr,
            port,
            auth,
            enable_password,
            handler,
            output_encoding: TextEncoding::default(),
        }
    }

    /// Override the character encoding used to decode SSH terminal output.
    pub fn with_output_encoding(mut self, output_encoding: TextEncoding) -> Self {
        self.output_encoding = output_encoding;
        self
    }

    /// Stable cache key used by the connection manager.
    pub fn device_addr(&self) -> String {
        format!("{}@{}:{}", self.user, self.addr, self.port)
    }
}

/// Bounded retry behavior for ordinary session operations.
///
/// Retries have at-least-once semantics: a device may apply a command and
/// disconnect before returning its prompt. Only enable retries for operations
/// whose commands are safe to repeat. Transaction, workflow, and upload APIs
/// do not consume this policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Maximum number of retries after the initial attempt.
    pub max_retries: usize,
    /// Delay before the first retry. Later retries double this duration.
    pub initial_backoff: Duration,
    /// Upper bound for exponential backoff delays.
    pub max_backoff: Duration,
    /// Whether server authentication rejections may be retried.
    pub retry_authentication_errors: bool,
}

impl RetryPolicy {
    /// Creates a retry policy with the requested number of retries.
    pub const fn new(max_retries: usize) -> Self {
        Self {
            max_retries,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            retry_authentication_errors: false,
        }
    }

    /// Sets the initial and maximum exponential backoff durations.
    pub const fn with_backoff(mut self, initial: Duration, maximum: Duration) -> Self {
        self.initial_backoff = initial;
        self.max_backoff = maximum;
        self
    }

    /// Controls whether authentication rejections are eligible for retries.
    pub const fn with_authentication_retries(mut self, enabled: bool) -> Self {
        self.retry_authentication_errors = enabled;
        self
    }

    fn validate(&self) -> Result<(), ConnectError> {
        if self.max_retries > 0 && self.initial_backoff > self.max_backoff {
            return Err(ConnectError::InvalidRetryPolicy(
                "initial_backoff must not exceed max_backoff".to_string(),
            ));
        }
        Ok(())
    }

    fn backoff_before_retry(&self, retry_index: usize) -> Duration {
        let mut backoff = self.initial_backoff.min(self.max_backoff);
        for _ in 1..retry_index.min(128) {
            backoff = backoff.saturating_mul(2).min(self.max_backoff);
            if backoff == self.max_backoff {
                break;
            }
        }
        backoff
    }

    fn retries_error(&self, error: &ConnectError) -> bool {
        error.is_transient()
            || (self.retry_authentication_errors && error.is_authentication_failure())
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Execution context shared by manager entrypoints.
#[derive(Clone)]
pub struct ExecutionContext {
    /// SSH security behavior for connection establishment.
    pub security_options: ConnectionSecurityOptions,
    /// Optional system name used by templates with dynamic transitions.
    pub sys: Option<String>,
    /// Maximum time allowed for the underlying SSH connection to establish.
    pub connect_timeout: Duration,
    /// Bounded retry behavior for ordinary command/session operations.
    pub retry_policy: RetryPolicy,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            security_options: ConnectionSecurityOptions::default(),
            sys: None,
            connect_timeout: Duration::from_secs(60),
            retry_policy: RetryPolicy::default(),
        }
    }
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

    /// Override the maximum time allowed for SSH connection establishment.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Override the SSH connection establishment timeout in seconds.
    pub fn with_connect_timeout_secs(self, timeout_secs: u64) -> Self {
        self.with_connect_timeout(Duration::from_secs(timeout_secs))
    }

    /// Applies a bounded retry policy to ordinary command/session operations.
    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }
}

/// A shared SSH client instance with state machine tracking.
pub struct SharedSshClient {
    client: Client,
    sender: Sender<String>,
    recv: Receiver<String>,
    handler: DeviceHandler,
    device_addr: String,
    prompt: String,
    hooks: SessionHooks,
    in_hook: bool,

    /// SHA-256 digest of the authentication method, used for connection
    /// parameter comparison (secrets themselves are never retained)
    auth_digest: [u8; 32],

    /// SHA-256 hash of the enable password (if present)
    enable_password_hash: Option<[u8; 32]>,

    /// Effective security options used when the connection was established.
    security_options: ConnectionSecurityOptions,

    /// Character encoding used to decode SSH terminal output.
    output_encoding: TextEncoding,

    /// Optional session recorder bound to this connection.
    recorder: Option<SessionRecorder>,

    /// Set once `close()` has run, so the connection is never reused or
    /// closed twice.
    closed: bool,
}

/// Structured prompt-response overrides for a single command execution.
///
/// Values are sent to the remote device as-is, so include any required trailing
/// newline when the prompt expects the response to be submitted immediately.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandDynamicParams {
    #[serde(default, alias = "EnablePassword")]
    pub enable_password: Option<String>,
    /// Extra prompt-response pairs for template-specific interactive flows.
    #[serde(default, flatten)]
    pub extra: HashMap<String, String>,
}

impl CommandDynamicParams {
    /// Returns true when no structured or extra prompt responses are set.
    pub fn is_empty(&self) -> bool {
        self.enable_password.is_none() && self.extra.is_empty()
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

/// Controls how a command containing newline-separated text is expanded.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MultilineMode {
    /// Execute each non-empty trimmed line as an independent command.
    #[default]
    SplitLines,
    /// Preserve the original text and execute it as one command.
    Whole,
}

pub(crate) fn mode_candidates(mode: &str) -> impl Iterator<Item = &str> {
    mode.split([',', '|'])
        .map(str::trim)
        .filter(|state| !state.is_empty())
}

/// Configuration for a command to execute on a device.
#[derive(Default, Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Command {
    /// Execution mode - Specifies the device mode in which the command should run.
    /// Multiple acceptable modes can be separated with `,` or `|`, for example
    /// `"Root,User"` or `"Login|Config"`. If the current mode is acceptable it
    /// is kept; otherwise the outermost acceptable mode is preferred.
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

    /// Controls whether newline-separated text is split into concrete commands.
    #[serde(default)]
    pub multiline_mode: MultilineMode,

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

impl Command {
    /// Override how newline-separated command text is expanded.
    pub fn with_multiline_mode(mut self, multiline_mode: MultilineMode) -> Self {
        self.multiline_mode = multiline_mode;
        self
    }

    /// Expand this command into the concrete flow described by its multiline strategy.
    pub fn into_flow(self) -> Result<CommandFlow, ConnectError> {
        match self.multiline_mode {
            MultilineMode::Whole => {
                if self.command.trim().is_empty() {
                    return Err(ConnectError::InvalidCommandFlow(
                        "multiline command cannot be empty".to_string(),
                    ));
                }
                Ok(CommandFlow::new(vec![self]))
            }
            MultilineMode::SplitLines => {
                let lines = self
                    .command
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if lines.is_empty() {
                    return Err(ConnectError::InvalidCommandFlow(
                        "multiline command has no executable lines".to_string(),
                    ));
                }

                let steps = lines
                    .into_iter()
                    .map(|command| Command {
                        command,
                        ..self.clone()
                    })
                    .collect();
                Ok(CommandFlow::new(steps))
            }
        }
    }
}

/// Higher-level executable operation supported by the session layer.
///
/// Transactions and workflows run this abstraction instead of assuming every
/// step is a plain text command. This keeps the current executor compatible
/// with direct commands and multi-step command flows.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionOperation {
    Command(Command),
    Flow(CommandFlow),
}

/// Stable summary metadata for any executable session operation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SessionOperationSummary {
    /// Operation kind identifier used for logging and dry-run inspection.
    pub kind: String,
    /// Primary mode used by the operation, typically the first command mode.
    pub mode: String,
    /// Human-readable description of what will run.
    pub description: String,
    /// Number of concrete command steps the operation expands to.
    pub step_count: usize,
}

impl SessionOperation {
    /// Wrap a single command as a session operation.
    pub fn command(command: Command) -> Self {
        Self::Command(command)
    }

    /// Wrap a multi-step flow as a session operation.
    pub fn flow(flow: CommandFlow) -> Self {
        Self::Flow(flow)
    }

    /// Inspect this operation without executing it.
    pub fn summary(&self) -> Result<SessionOperationSummary, ConnectError> {
        self.summary_impl()
    }
}

impl From<Command> for SessionOperation {
    fn from(value: Command) -> Self {
        Self::Command(value)
    }
}

impl From<CommandFlow> for SessionOperation {
    fn from(value: CommandFlow) -> Self {
        Self::Flow(value)
    }
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CommandFlow {
    /// Ordered list of commands executed on the same live session.
    #[serde(default)]
    pub steps: Vec<Command>,
    /// Stop immediately after the first command that reports `success = false`.
    #[serde(default = "default_stop_on_error")]
    pub stop_on_error: bool,
    /// Maximum number of executed child steps before aborting as invalid flow.
    ///
    /// This acts as a safety guard for unusually long or accidentally recursive
    /// command flows assembled by callers.
    #[serde(default)]
    pub max_steps: Option<usize>,
}

impl Default for CommandFlow {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            stop_on_error: true,
            max_steps: None,
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

    /// Expand multiline commands while preserving flow-level execution options.
    pub fn expand_multiline(self) -> Result<Self, ConnectError> {
        let Self {
            steps,
            stop_on_error,
            max_steps,
        } = self;
        let mut expanded_steps = Vec::new();
        for command in steps {
            expanded_steps.extend(command.into_flow()?.steps);
        }
        Ok(Self {
            steps: expanded_steps,
            stop_on_error,
            max_steps,
        })
    }

    /// Override whether execution should stop after the first unsuccessful step.
    pub fn with_stop_on_error(mut self, stop_on_error: bool) -> Self {
        self.stop_on_error = stop_on_error;
        self
    }

    /// Override the flow-level maximum executed-step limit.
    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = Some(max_steps);
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

/// Detailed execution result for one concrete child step inside a session operation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SessionOperationStepOutput {
    /// Child step index inside the executed operation.
    pub step_index: usize,
    /// Mode used for this child step.
    pub mode: String,
    /// Human-readable child step summary.
    pub operation_summary: String,
    /// Whether this child step succeeded.
    pub success: bool,
    /// Optional exit code captured from shell execution.
    pub exit_code: Option<i32>,
    /// Primary captured content for this child step.
    pub content: String,
    /// Full captured transcript for this child step.
    pub all: String,
    /// Prompt observed after the child step finished.
    pub prompt: Option<String>,
}

impl SessionOperationStepOutput {
    /// Drop operation-specific metadata and keep only the legacy command output shape.
    pub fn into_output(self) -> Output {
        Output {
            success: self.success,
            exit_code: self.exit_code,
            content: self.content,
            all: self.all,
            prompt: self.prompt,
        }
    }

    fn to_output(&self) -> Output {
        Output {
            success: self.success,
            exit_code: self.exit_code,
            content: self.content.clone(),
            all: self.all.clone(),
            prompt: self.prompt.clone(),
        }
    }
}

/// Generic execution result for any session operation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SessionOperationOutput {
    /// Whether the overall operation succeeded.
    pub success: bool,
    /// Concrete child step outputs produced by the operation.
    #[serde(default)]
    pub steps: Vec<SessionOperationStepOutput>,
}

impl SessionOperationOutput {
    /// Convert this generic result into the legacy command-flow result shape.
    pub fn into_command_flow_output(self) -> CommandFlowOutput {
        CommandFlowOutput {
            success: self.success,
            outputs: self
                .steps
                .into_iter()
                .map(SessionOperationStepOutput::into_output)
                .collect(),
        }
    }

    /// Borrow this generic result as the legacy command-flow result shape.
    pub fn to_command_flow_output(&self) -> CommandFlowOutput {
        CommandFlowOutput {
            success: self.success,
            outputs: self
                .steps
                .iter()
                .map(SessionOperationStepOutput::to_output)
                .collect(),
        }
    }
}

/// Public error returned by operation-level APIs when execution fails.
///
/// Unlike plain `ConnectError`, this error preserves already completed child
/// step outputs so callers can inspect partial progress for multi-step
/// operations outside transaction/workflow execution.
#[derive(Debug)]
pub struct SessionOperationExecutionError {
    error: ConnectError,
    partial_output: SessionOperationOutput,
}

impl SessionOperationExecutionError {
    /// Build a new operation execution error from the root cause and partial output.
    pub fn new(error: ConnectError, partial_output: SessionOperationOutput) -> Self {
        Self {
            error,
            partial_output,
        }
    }

    /// Borrow the underlying connection/session error.
    pub fn error(&self) -> &ConnectError {
        &self.error
    }

    /// Borrow partial child step outputs captured before the failure.
    pub fn partial_output(&self) -> &SessionOperationOutput {
        &self.partial_output
    }

    /// Consume the wrapper and return both the root cause and partial output.
    pub fn into_parts(self) -> (ConnectError, SessionOperationOutput) {
        (self.error, self.partial_output)
    }
}

impl std::fmt::Display for SessionOperationExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for SessionOperationExecutionError {}

/// Compatibility result for command-flow-specific APIs.
#[derive(Debug, Clone)]
pub struct CommandFlowOutput {
    pub success: bool,
    pub outputs: Vec<Output>,
}

/// Tuning options for the SSH connection pool.
#[derive(Debug, Clone)]
pub struct ConnectionPoolConfig {
    /// Maximum number of connections kept in the pool.
    pub max_connections: u64,
    /// How long an idle connection stays pooled before it is closed.
    ///
    /// Keep this below the exec/idle timeout configured on the devices
    /// themselves so the pool never hands out a connection the device
    /// has already dropped.
    pub idle_timeout: Duration,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 100,
            idle_timeout: Duration::from_secs(5 * 60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConnectionCacheKey {
    Shared(String),
    Recorder {
        device_addr: String,
        recorder_id: u64,
    },
}

struct ConnectionPoolInner {
    cache: Cache<ConnectionCacheKey, (mpsc::Sender<CmdJob>, Arc<RwLock<SharedSshClient>>)>,
    /// Whether the pool maintenance task has been started.
    maintenance_started: AtomicBool,
    /// Interval between pending-task maintenance runs.
    maintenance_period: Duration,
}

/// SSH connection pool manager.
///
/// Manages a cache of SSH connections with automatic reconnection and
/// connection pooling. A connection is gracefully closed (running
/// `before_disconnect` hooks) once its last command sender is gone — i.e.
/// the pool evicted its handle and no caller still holds one.
#[derive(Clone)]
pub struct SshConnectionManager {
    inner: Arc<ConnectionPoolInner>,
}

mod client;
mod fleet;
mod hooks;
mod manager;
mod recording;
mod security;
mod transaction;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    #[tokio::test]
    async fn in_process_rsa_keys_require_explicit_opt_in() {
        use russh::keys::ssh_key::{Algorithm, LineEnding, PrivateKey};

        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Rsa { hash: None })
            .expect("generate RSA key")
            .to_openssh(LineEnding::LF)
            .expect("encode RSA key");
        let auth = SshAuthMethod::private_key(key.to_string(), None);

        let error = auth
            .to_transport()
            .await
            .expect_err("RSA must be rejected by default");
        assert!(matches!(error, ConnectError::InvalidSshAuth(_)));

        SshAuthMethod::private_key_allow_vulnerable_rsa(key.to_string(), None)
            .to_transport()
            .await
            .expect("explicit opt-in allows RSA");
    }

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
        assert!(matches!(
            request.auth,
            SshAuthMethod::Password(ref password) if password == "password"
        ));
        assert_eq!(request.output_encoding, TextEncoding::Utf8);
    }

    #[test]
    fn connection_and_detect_requests_allow_output_encoding_overrides() {
        let connection = ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco().expect("template"),
        )
        .with_output_encoding(TextEncoding::Gbk);
        let detect = DetectRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
        )
        .with_output_encoding(TextEncoding::Gb18030);

        assert_eq!(connection.output_encoding, TextEncoding::Gbk);
        assert_eq!(detect.output_encoding, TextEncoding::Gb18030);
    }

    #[tokio::test]
    async fn ssh_auth_method_fingerprint_distinguishes_methods() {
        let password = SshAuthMethod::password("secret");
        let other_password = SshAuthMethod::password("other");
        let key = SshAuthMethod::private_key("-----BEGIN OPENSSH PRIVATE KEY-----\n", None);
        let interactive = SshAuthMethod::keyboard_interactive(vec![(
            "Password".to_string(),
            "secret".to_string(),
        )]);

        assert_ne!(
            password.fingerprint().await.expect("fingerprint"),
            other_password.fingerprint().await.expect("fingerprint")
        );
        assert_ne!(
            password.fingerprint().await.expect("fingerprint"),
            key.fingerprint().await.expect("fingerprint")
        );
        assert_ne!(
            password.fingerprint().await.expect("fingerprint"),
            interactive.fingerprint().await.expect("fingerprint")
        );
        assert_eq!(
            password.fingerprint().await.expect("fingerprint"),
            SshAuthMethod::password("secret")
                .fingerprint()
                .await
                .expect("fingerprint")
        );
    }

    #[tokio::test]
    async fn ssh_auth_method_fingerprint_distinguishes_absent_and_empty_passphrases() {
        let without_passphrase = SshAuthMethod::private_key("key-data", None);
        let empty_passphrase = SshAuthMethod::private_key("key-data", Some(String::new()));

        assert_ne!(
            without_passphrase.fingerprint().await.expect("fingerprint"),
            empty_passphrase.fingerprint().await.expect("fingerprint")
        );
    }

    #[tokio::test]
    async fn missing_private_key_file_returns_auth_configuration_error() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let missing_path =
            std::env::temp_dir().join(format!("rneter-missing-key-{}-{nonce}", std::process::id()));
        let error = SshAuthMethod::private_key_file(missing_path, None)
            .fingerprint()
            .await
            .expect_err("missing key file");

        assert!(matches!(error, ConnectError::InvalidSshAuth(_)));
    }

    #[test]
    fn ssh_auth_method_debug_redacts_secrets() {
        let auth_methods = [
            SshAuthMethod::password("password-secret-value"),
            SshAuthMethod::private_key(
                "private-key-secret-value",
                Some("passphrase-secret-value".to_string()),
            ),
            SshAuthMethod::keyboard_interactive(vec![(
                "One-Time Password".to_string(),
                "interactive-secret-value".to_string(),
            )]),
        ];

        let debug_output = format!("{auth_methods:?}");
        for secret in [
            "password-secret-value",
            "private-key-secret-value",
            "passphrase-secret-value",
            "interactive-secret-value",
        ] {
            assert!(!debug_output.contains(secret));
        }
        assert!(debug_output.contains("<redacted>"));
        assert!(debug_output.contains("response_count: 1"));
    }

    #[test]
    fn execution_context_builder_overrides_defaults() {
        let retry_policy = RetryPolicy::new(3)
            .with_backoff(Duration::from_millis(25), Duration::from_millis(100))
            .with_authentication_retries(true);
        let context = ExecutionContext::new()
            .with_security_options(ConnectionSecurityOptions::legacy_compatible())
            .with_sys(Some("vsys1".to_string()))
            .with_connect_timeout(Duration::from_secs(12))
            .with_retry_policy(retry_policy);
        assert_eq!(
            context.security_options,
            ConnectionSecurityOptions::legacy_compatible()
        );
        assert_eq!(context.sys.as_deref(), Some("vsys1"));
        assert_eq!(context.connect_timeout, Duration::from_secs(12));
        assert_eq!(context.retry_policy, retry_policy);
    }

    #[test]
    fn execution_context_connect_timeout_defaults_to_60_seconds() {
        assert_eq!(
            ExecutionContext::new().connect_timeout,
            Duration::from_secs(60)
        );
        assert_eq!(
            ExecutionContext::new()
                .with_connect_timeout_secs(7)
                .connect_timeout,
            Duration::from_secs(7)
        );
        assert_eq!(ExecutionContext::new().retry_policy, RetryPolicy::default());
    }

    #[test]
    fn retry_policy_uses_capped_exponential_backoff() {
        let policy = RetryPolicy::new(4)
            .with_backoff(Duration::from_millis(100), Duration::from_millis(250));

        assert_eq!(policy.backoff_before_retry(1), Duration::from_millis(100));
        assert_eq!(policy.backoff_before_retry(2), Duration::from_millis(200));
        assert_eq!(policy.backoff_before_retry(3), Duration::from_millis(250));
        assert_eq!(policy.backoff_before_retry(4), Duration::from_millis(250));
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
    fn operation_execution_error_preserves_partial_output() {
        let err = SessionOperationExecutionError::new(
            ConnectError::ExecTimeout("show version".to_string()),
            SessionOperationOutput {
                success: false,
                steps: vec![SessionOperationStepOutput {
                    step_index: 0,
                    mode: "Enable".to_string(),
                    operation_summary: "terminal length 0".to_string(),
                    success: true,
                    exit_code: None,
                    content: "ok".to_string(),
                    all: "ok".to_string(),
                    prompt: Some("router#".to_string()),
                }],
            },
        );

        assert!(matches!(err.error(), ConnectError::ExecTimeout(_)));
        assert_eq!(err.partial_output().steps.len(), 1);
        assert_eq!(
            err.partial_output().steps[0].operation_summary,
            "terminal length 0"
        );
    }

    #[test]
    fn command_default_has_empty_dyn_params() {
        let cmd = Command::default();
        assert_eq!(cmd.timeout, None);
        assert!(cmd.mode.is_empty());
        assert!(cmd.command.is_empty());
        assert_eq!(cmd.multiline_mode, MultilineMode::SplitLines);
        assert!(cmd.dyn_params.is_empty());
        assert!(cmd.interaction.is_empty());
    }

    #[test]
    fn multiline_mode_defaults_to_split_lines() {
        assert_eq!(MultilineMode::default(), MultilineMode::SplitLines);
    }

    #[test]
    fn command_into_flow_splits_lines_and_inherits_command_options() {
        let command = Command {
            mode: "Config".to_string(),
            command: "\n interface Gi0/1 \n\n description uplink \n no shutdown\n".to_string(),
            multiline_mode: MultilineMode::SplitLines,
            timeout: Some(30),
            dyn_params: CommandDynamicParams {
                enable_password: None,
                extra: HashMap::from([("confirm".to_string(), "yes\n".to_string())]),
            },
            interaction: CommandInteraction::default().push_prompt(PromptResponseRule::new(
                vec!["confirm".to_string()],
                "yes\n".to_string(),
            )),
        };

        let flow = command.into_flow().expect("split multiline command");

        assert_eq!(flow.steps.len(), 3);
        assert_eq!(flow.steps[0].command, "interface Gi0/1");
        assert_eq!(flow.steps[1].command, "description uplink");
        assert_eq!(flow.steps[2].command, "no shutdown");
        assert!(flow.steps.iter().all(|step| step.mode == "Config"));
        assert!(flow.steps.iter().all(|step| step.timeout == Some(30)));
        assert!(
            flow.steps
                .iter()
                .all(|step| step.dyn_params == flow.steps[0].dyn_params)
        );
        assert!(
            flow.steps
                .iter()
                .all(|step| step.interaction == flow.steps[0].interaction)
        );
    }

    #[test]
    fn command_into_flow_whole_preserves_original_text() {
        let original = "echo first\necho second\n";
        let flow = Command {
            mode: "Root".to_string(),
            command: original.to_string(),
            ..Command::default()
        }
        .with_multiline_mode(MultilineMode::Whole)
        .into_flow()
        .expect("preserve multiline command");

        assert_eq!(flow.steps.len(), 1);
        assert_eq!(flow.steps[0].command, original);
    }

    #[test]
    fn command_into_flow_rejects_empty_multiline_text() {
        for mode in [MultilineMode::SplitLines, MultilineMode::Whole] {
            let err = Command {
                mode: "Enable".to_string(),
                command: " \n\t\n".to_string(),
                ..Command::default()
            }
            .with_multiline_mode(mode)
            .into_flow()
            .expect_err("empty multiline command should fail");

            assert!(matches!(err, ConnectError::InvalidCommandFlow(_)));
        }
    }

    #[test]
    fn command_flow_expands_nested_multiline_commands() {
        let flow = CommandFlow::new(vec![
            Command {
                mode: "Enable".to_string(),
                command: "show version\nshow inventory".to_string(),
                ..Command::default()
            },
            Command {
                mode: "Root".to_string(),
                command: "printf 'a\\nb'".to_string(),
                ..Command::default()
            }
            .with_multiline_mode(MultilineMode::Whole),
        ])
        .with_stop_on_error(false)
        .with_max_steps(4)
        .expand_multiline()
        .expect("expand multiline flow");

        assert_eq!(flow.steps.len(), 3);
        assert_eq!(flow.steps[0].command, "show version");
        assert_eq!(flow.steps[1].command, "show inventory");
        assert_eq!(flow.steps[2].command, "printf 'a\\nb'");
        assert!(!flow.stop_on_error);
        assert_eq!(flow.max_steps, Some(4));
    }

    #[test]
    fn command_dynamic_params_collect_unknown_keys_into_extra() {
        let cmd: Command = serde_json::from_value(serde_json::json!({
            "mode": "Enable",
            "command": "show version",
            "dyn_params": {
                "EnablePassword": "enable\n",
                "CustomPrompt": "yes\n"
            }
        }))
        .expect("deserialize command");

        assert_eq!(cmd.dyn_params.enable_password.as_deref(), Some("enable\n"));
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
        assert_eq!(flow.max_steps, None);
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
