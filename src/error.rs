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
    ///
    /// This occurs when trying to transition to a state that is not reachable
    /// through any path in the device's state graph.
    #[error("unreachable state {0}")]
    UnreachableState(String),

    /// The target state does not exist in the device's state configuration.
    #[error("target state not exist")]
    TargetStateNotExistError,

    /// The SSH channel was disconnected while waiting for a prompt.
    ///
    /// This typically happens when the remote device closes the connection
    /// unexpectedly during initialization or command execution.
    #[error("channel disconnect on wait prompt")]
    ChannelDisconnectError,

    /// The SSH connection has been closed.
    ///
    /// This error is returned when attempting to use a connection that has
    /// already been closed or terminated.
    #[error("connect closed")]
    ConnectClosedError,

    /// No exit command is defined for the specified state.
    ///
    /// This occurs when trying to exit from a state that doesn't have an
    /// exit path configured in the device handler.
    #[error("{0} no exit command")]
    NoExitCommandError(String),

    /// Command execution timed out.
    ///
    /// The command did not complete within the configured timeout period.
    /// The error contains the partial output received before the timeout.
    #[error("exec command timeout: {0}")]
    ExecTimeout(String),

    /// An error occurred in the async-ssh2-tokio library.
    #[error("async ssh2 error: {0}")]
    Ssh2Error(#[from] async_ssh2_tokio::Error),

    /// An error occurred in the russh library.
    #[error("russh error: {0}")]
    RusshError(#[from] russh::Error),

    /// Failed to send data through the channel.
    #[error("Failed to send data: {0}")]
    SendDataError(#[from] SendError<String>),
}
