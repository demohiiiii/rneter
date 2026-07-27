//! Error types for SSH connection and device state management.
//!
//! This module defines all errors that can occur during SSH operations,
//! device state transitions, and command execution.

use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

/// Errors that can occur during SSH connection and device state management.
///
/// Marked `#[non_exhaustive]` so new error variants can be added without a
/// breaking change; downstream `match` arms need a wildcard branch.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ConnectError {
    /// The target state cannot be reached from the current state.
    #[error("unreachable state {0}")]
    UnreachableState(String),

    /// The target state does not exist in the configuration.
    #[error("target state does not exist")]
    TargetStateNotExistError,

    /// The SSH channel was disconnected while waiting for a prompt.
    #[error("channel disconnected while waiting for prompt")]
    ChannelDisconnectError,

    /// The SSH channel was disconnected during a specific session stage.
    #[error("channel disconnected during {stage} for {target}")]
    ChannelDisconnectStageError { stage: &'static str, target: String },

    /// The SSH connection has been closed.
    #[error("connection closed")]
    ConnectClosedError,

    /// No exit command is defined for the specified state.
    #[error("{0} no exit command")]
    NoExitCommandError(String),

    /// Command execution timed out.
    #[error("exec command timeout: {0}")]
    ExecTimeout(String),

    /// SSH connection initialization timed out while waiting for initial prompt.
    #[error("connection initialization timeout: {0}")]
    InitTimeout(String),

    /// SSH connection establishment timed out.
    #[error("SSH connection timeout: {0}")]
    ConnectTimeout(String),

    /// Device handler configuration is invalid.
    #[error("invalid device handler config: {0}")]
    InvalidDeviceHandlerConfig(String),

    /// Interactive command prompt-response rules are invalid.
    #[error("invalid command interaction: {0}")]
    InvalidCommandInteraction(String),

    /// Command flow definition is invalid at runtime.
    #[error("invalid command flow: {0}")]
    InvalidCommandFlow(String),

    /// SSH authentication material could not be loaded or fingerprinted.
    #[error("invalid SSH authentication configuration: {0}")]
    InvalidSshAuth(String),

    /// Fleet execution options are invalid.
    #[error("invalid fleet options: {0}")]
    InvalidFleetOptions(String),

    /// An error occurred in the async-ssh2-tokio library.
    #[error("async ssh2 error: {0}")]
    Ssh2Error(#[from] async_ssh2_tokio::Error),

    /// An async-ssh2-tokio error occurred during a specific SSH session stage.
    #[error("async ssh2 error during {stage} for {target}: {source}")]
    Ssh2StageError {
        stage: &'static str,
        target: String,
        #[source]
        source: async_ssh2_tokio::Error,
    },

    /// An error occurred in the russh library.
    #[error("russh error: {0}")]
    RusshError(#[from] russh::Error),

    /// A russh error occurred during a specific SSH session stage.
    #[error("russh error during {stage} for {target}: {source}")]
    RusshStageError {
        stage: &'static str,
        target: String,
        #[source]
        source: russh::Error,
    },

    /// Failed to send data through the channel.
    #[error("Failed to send data: {0}")]
    SendDataError(#[from] SendError<String>),

    /// Requested template is not found.
    #[error("template not found: {0}")]
    TemplateNotFound(String),

    /// Autodetect did not produce any ranked candidate.
    #[error("autodetect found no matching template: {0}")]
    AutodetectNoMatch(String),

    /// Autodetect found a candidate, but its confidence was below policy.
    #[error("autodetect confidence too low: {0}")]
    AutodetectConfidenceTooLow(String),

    /// Replay data does not match expected command/mode flow.
    #[error("replay mismatch: {0}")]
    ReplayMismatchError(String),

    /// Transaction block is invalid.
    #[error("invalid transaction block: {0}")]
    InvalidTransaction(String),

    /// An internal server error occurred.
    #[error("Internal server error: {0}")]
    InternalServerError(String),
}

impl ConnectError {
    pub(crate) fn ssh2_stage(
        stage: &'static str,
        target: impl Into<String>,
        source: async_ssh2_tokio::Error,
    ) -> Self {
        Self::Ssh2StageError {
            stage,
            target: target.into(),
            source,
        }
    }

    pub(crate) fn russh_stage(
        stage: &'static str,
        target: impl Into<String>,
        source: russh::Error,
    ) -> Self {
        Self::RusshStageError {
            stage,
            target: target.into(),
            source,
        }
    }

    pub(crate) fn channel_disconnect_stage(stage: &'static str, target: impl Into<String>) -> Self {
        Self::ChannelDisconnectStageError {
            stage,
            target: target.into(),
        }
    }
}
