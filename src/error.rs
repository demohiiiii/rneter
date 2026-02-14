//! Error types for SSH connection and device state management.
//!
//! This module defines all errors that can occur during SSH operations,
//! device state transitions, and command execution.

use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

/// Errors that can occur during SSH connection and device state management.
#[derive(Error, Debug)]
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

    /// Device handler configuration is invalid.
    #[error("invalid device handler config: {0}")]
    InvalidDeviceHandlerConfig(String),

    /// An error occurred in the async-ssh2-tokio library.
    #[error("async ssh2 error: {0}")]
    Ssh2Error(#[from] async_ssh2_tokio::Error),

    /// An error occurred in the russh library.
    #[error("russh error: {0}")]
    RusshError(#[from] russh::Error),

    /// Failed to send data through the channel.
    #[error("Failed to send data: {0}")]
    SendDataError(#[from] SendError<String>),

    /// Replay data does not match expected command/mode flow.
    #[error("replay mismatch: {0}")]
    ReplayMismatchError(String),

    /// An internal server error occurred.
    #[error("Internal server error: {0}")]
    InternalServerError(String),
}
