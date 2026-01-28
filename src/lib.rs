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
//! - **Maximum Compatibility**: Supports a wide range of SSH algorithms for compatibility with legacy devices
//! - **Async/Await**: Built on Tokio for high-performance asynchronous operations
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use rneter::session::{MANAGER, Command};
//! use rneter::device::DeviceHandler;
//! use std::collections::HashMap;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure device handler with state machine
//!     let handler = DeviceHandler::new(
//!         vec![("UserMode".to_string(), vec![r">\s*$"])],
//!         vec![],
//!         vec![],
//!         vec![r"--More--"],
//!         vec![r"% Invalid"],
//!         vec![],
//!         vec![],
//!         HashMap::new(),
//!     );
//!
//!     // Get a connection from the manager
//!     let sender = MANAGER.get(
//!         "admin".to_string(),
//!         "192.168.1.1".to_string(),
//!         22,
//!         "password".to_string(),
//!         None,
//!         handler,
//!     ).await?;
//!
//!     // Execute a command
//!     let (tx, rx) = tokio::sync::oneshot::channel();
//!     let cmd = rneter::session::CmdJob {
//!         data: Command {
//!             cmd_type: "show".to_string(),
//!             mode: "UserMode".to_string(),
//!             command: "show version".to_string(),
//!             template: String::new(),
//!             timeout: 60,
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
//! - [`config`] - SSH configuration constants for maximum compatibility

pub mod config;
pub mod device;
pub mod error;
pub mod session;
