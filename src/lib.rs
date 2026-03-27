//! # rneter - Network Device SSH Connection Manager
//!
//! `rneter` is a Rust library for managing SSH connections to network devices with
//! intelligent state machine handling. It provides a high-level API for connecting to
//! network devices (routers, switches, etc.), executing commands, and managing device
//! states with automatic prompt detection and mode switching.
//!
//! ## Features
//!
//! - **Connection Pooling**: Automatically caches and reuses SSH connections
//! - **State Machine Management**: Intelligent device state tracking and transitions
//! - **Prompt Detection**: Automatic prompt recognition and handling
//! - **Mode Switching**: Seamless transitions between device modes (user mode, enable mode, config mode, etc.)
//! - **SFTP File Uploads**: Upload local files to remote hosts that expose the SSH `sftp` subsystem
//! - **Built-in Copy Flow Templates**: Reuse structured templates for Cisco-like interactive `copy` workflows
//! - **Maximum Compatibility**: Supports a wide range of SSH algorithms for compatibility with legacy devices
//! - **Async/Await**: Built on Tokio for high-performance asynchronous operations
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use rneter::session::{ConnectionRequest, ExecutionContext, MANAGER, Command, CmdJob};
//! use rneter::templates;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Use a predefined device template (e.g., Cisco)
//!     let handler = templates::cisco()?;
//!
//!     // Get a connection from the manager
//!     let sender = MANAGER
//!         .get_with_context(
//!             ConnectionRequest::new(
//!                 "admin".to_string(),
//!                 "192.168.1.1".to_string(),
//!                 22,
//!                 "password".to_string(),
//!                 None,
//!                 handler,
//!             ),
//!             ExecutionContext::default(),
//!         )
//!         .await?;
//!
//!     // Execute a command
//!     let (tx, rx) = tokio::sync::oneshot::channel();
//!     let cmd = CmdJob {
//!         data: Command {
//!             mode: "Enable".to_string(), // Cisco template uses "Enable" mode
//!             command: "show version".to_string(),
//!             timeout: Some(60),
//!             ..Command::default()
//!         },
//!         sys: None,
//!         responder: tx,
//!     };
//!     
//!     sender.send(cmd).await?;
//!     let output = rx.await??;
//!     
//!     println!("Command output: {}", output.content);
//!     Ok(())
//! }
//! ```
//!
//! ## Main Components
//!
//! - [`session::SshConnectionManager`] - Manages SSH connection pool and lifecycle
//! - [`device::DeviceHandler`] - Handles device state machine and transitions
//! - [`error::ConnectError`] - Error types for connection and state operations
//! - [`session::SessionOperationExecutionError`] - Operation-level execution error with partial outputs
//! - [`config`] - SSH configuration constants
//! - [`templates`] - Predefined device configurations for common vendors for maximum compatibility

pub mod config;
pub mod device;
pub mod error;
pub mod session;
pub mod templates;
