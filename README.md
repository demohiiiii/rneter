# rneter

[![Crates.io](https://img.shields.io/crates/v/rneter.svg)](https://crates.io/crates/rneter)
[![Documentation](https://docs.rs/rneter/badge.svg)](https://docs.rs/rneter)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

[中文文档](README_zh.md)

`rneter` is a Rust library for managing SSH connections to network devices with intelligent state machine handling. It provides a high-level API for connecting to network devices (routers, switches, etc.), executing commands, and managing device states with automatic prompt detection and mode switching.

## Features

- **Connection Pooling**: Automatically caches and reuses SSH connections for better performance
- **State Machine Management**: Intelligent device state tracking and automatic transitions
- **Prompt Detection**: Automatic prompt recognition and handling across different device types
- **Mode Switching**: Seamless transitions between device modes (user mode, enable mode, config mode, etc.)
- **Maximum Compatibility**: Supports a wide range of SSH algorithms including legacy protocols for older devices
- **Async/Await**: Built on Tokio for high-performance asynchronous operations
- **Error Handling**: Comprehensive error types with detailed context

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
rneter = "0.1"
```

## Quick Start

```rust
use rneter::session::{MANAGER, Command, CmdJob};
use rneter::templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use a predefined device template (e.g., Cisco)
    let handler = templates::cisco();

    // Get a connection from the manager
    let sender = MANAGER.get(
        "admin".to_string(),
        "192.168.1.1".to_string(),
        22,
        "password".to_string(),
        None,
        handler,
    ).await?;

    // Execute a command
    let (tx, rx) = tokio::sync::oneshot::channel();
    let cmd = CmdJob {
        data: Command {
            cmd_type: "show".to_string(),
            mode: "Enable".to_string(), // Cisco template uses "Enable" mode
            command: "show version".to_string(),
            template: String::new(),
            timeout: 60,
        },
        sys: None,
        responder: tx,
    };
    
    sender.send(cmd).await?;
    let output = rx.await??;
    
    println!("Command successful: {}", output.success);
    println!("Output: {}", output.content);
    Ok(())
}
```

## Architecture

### Connection Management

The `SshConnectionManager` provides a singleton connection pool accessible via the `MANAGER` constant. It automatically:
- Caches connections for 5 minutes of inactivity
- Reconnects on connection failure
- Manages up to 100 concurrent connections

### State Machine

The `DeviceHandler` implements a finite state machine that:
- Tracks the current device state using regex patterns
- Finds optimal paths between states using BFS
- Handles automatic state transitions
- Supports system-specific states (e.g., different VRFs or contexts)

### Command Execution

Commands are executed through an async channel-based architecture:
1. Submit a `CmdJob` to the connection sender
2. The library automatically transitions to the target state if needed
3. Executes the command and waits for the prompt
4. Returns the output with success status

## Supported Device Types

The library is designed to work with any SSH-enabled network device. It's particularly well-suited for:

- Cisco IOS/IOS-XE/IOS-XR devices
- Juniper JunOS devices
- Arista EOS devices
- Huawei VRP devices
- Generic Linux/Unix systems accessible via SSH

## Configuration

### SSH Algorithm Support

`rneter` includes comprehensive SSH algorithm support in the `config` module:
- Key exchange: Curve25519, DH groups, ECDH
- Ciphers: AES (CTR/CBC/GCM), ChaCha20-Poly1305
- MAC: HMAC-SHA1/256/512 with ETM variants
- Host keys: Ed25519, ECDSA, RSA, DSA (for legacy devices)

This ensures maximum compatibility with both modern and legacy network equipment.

## Error Handling

The library provides detailed error types through `ConnectError`:

- `UnreachableState`: Target state cannot be reached from current state
- `TargetStateNotExistError`: Requested state doesn't exist in configuration
- `ChannelDisconnectError`: SSH channel disconnected unexpectedly
- `ExecTimeout`: Command execution exceeded timeout
- And more...

## Documentation

For detailed API documentation, visit [docs.rs/rneter](https://docs.rs/rneter).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Author

demohiiiii
