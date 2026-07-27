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

    /// Retry policy options are invalid.
    #[error("invalid retry policy: {0}")]
    InvalidRetryPolicy(String),

    /// A shared connection attempt failed with a transient transport error.
    #[error("SSH connection establishment failed: {0}")]
    ConnectionEstablishmentFailed(String),

    /// A shared connection attempt failed authentication.
    #[error("SSH authentication failed: {0}")]
    AuthenticationFailed(String),

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

    /// Whether this error represents a transient connection or channel failure.
    ///
    /// Command execution timeouts and authentication failures are deliberately
    /// excluded because retrying them can duplicate a remote side effect or
    /// repeatedly submit invalid credentials.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::ChannelDisconnectError
            | Self::ChannelDisconnectStageError { .. }
            | Self::ConnectClosedError
            | Self::InitTimeout(_)
            | Self::ConnectTimeout(_)
            | Self::ConnectionEstablishmentFailed(_)
            | Self::SendDataError(_) => true,
            Self::RusshError(source) | Self::RusshStageError { source, .. } => {
                russh_error_is_transient(source)
            }
            Self::Ssh2Error(source) | Self::Ssh2StageError { source, .. } => {
                ssh2_error_is_transient(source)
            }
            _ => false,
        }
    }

    /// Whether this error is an SSH authentication rejection.
    pub fn is_authentication_failure(&self) -> bool {
        match self {
            Self::AuthenticationFailed(_) => true,
            Self::Ssh2Error(source) | Self::Ssh2StageError { source, .. } => {
                ssh2_error_is_authentication_failure(source)
            }
            _ => false,
        }
    }

    pub(crate) fn from_shared_connection_error(error: &Self) -> Self {
        match error {
            Self::ConnectTimeout(target) => Self::ConnectTimeout(target.clone()),
            Self::InitTimeout(message) => Self::InitTimeout(message.clone()),
            _ if error.is_authentication_failure() => Self::AuthenticationFailed(error.to_string()),
            _ if error.is_transient() => Self::ConnectionEstablishmentFailed(error.to_string()),
            _ => Self::InternalServerError(error.to_string()),
        }
    }
}

fn ssh2_error_is_transient(error: &async_ssh2_tokio::Error) -> bool {
    match error {
        async_ssh2_tokio::Error::SshError(source) => russh_error_is_transient(source),
        async_ssh2_tokio::Error::SendError(_)
        | async_ssh2_tokio::Error::IoError(_)
        | async_ssh2_tokio::Error::ChannelSendError(_) => true,
        _ => false,
    }
}

fn russh_error_is_transient(error: &russh::Error) -> bool {
    matches!(
        error,
        russh::Error::Disconnect
            | russh::Error::HUP
            | russh::Error::ConnectionTimeout
            | russh::Error::KeepaliveTimeout
            | russh::Error::InactivityTimeout
            | russh::Error::SendError
            | russh::Error::IO(_)
            | russh::Error::Elapsed(_)
            | russh::Error::RecvError
    )
}

fn ssh2_error_is_authentication_failure(error: &async_ssh2_tokio::Error) -> bool {
    matches!(
        error,
        async_ssh2_tokio::Error::KeyboardInteractiveAuthFailed
            | async_ssh2_tokio::Error::KeyAuthFailed
            | async_ssh2_tokio::Error::PasswordWrong
            | async_ssh2_tokio::Error::AgentAuthenticationFailed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_classification_is_conservative() {
        assert!(ConnectError::ConnectTimeout("device".to_string()).is_transient());
        assert!(ConnectError::ChannelDisconnectError.is_transient());
        assert!(!ConnectError::ExecTimeout("show version".to_string()).is_transient());
        assert!(!ConnectError::InvalidSshAuth("bad key".to_string()).is_transient());
        assert!(ConnectError::RusshError(russh::Error::HUP).is_transient());
        assert!(
            ConnectError::Ssh2Error(async_ssh2_tokio::Error::SshError(russh::Error::Disconnect))
                .is_transient()
        );
    }

    #[test]
    fn permanent_russh_errors_are_not_transient() {
        let errors = [
            russh::Error::NoCommonAlgo {
                kind: russh::AlgorithmKind::Kex,
                ours: vec!["curve25519-sha256".to_string()],
                theirs: vec!["diffie-hellman-group1-sha1".to_string()],
            },
            russh::Error::KeyChanged { line: 1 },
            russh::Error::WrongServerSig,
            russh::Error::UnsupportedAuthMethod,
            russh::Error::InvalidConfig("bad config".to_string()),
        ];

        for error in errors {
            assert!(
                !ConnectError::RusshError(error).is_transient(),
                "permanent russh error must not be retried"
            );
        }

        let wrapped = ConnectError::Ssh2Error(async_ssh2_tokio::Error::SshError(
            russh::Error::UnsupportedAuthMethod,
        ));
        assert!(!wrapped.is_transient());
    }

    #[test]
    fn authentication_rejections_are_classified_separately() {
        let error = ConnectError::Ssh2StageError {
            stage: "connect",
            target: "device".to_string(),
            source: async_ssh2_tokio::Error::PasswordWrong,
        };

        assert!(error.is_authentication_failure());
        assert!(!error.is_transient());
        let shared = ConnectError::from_shared_connection_error(&error);
        assert!(matches!(shared, ConnectError::AuthenticationFailed(_)));
    }
}
