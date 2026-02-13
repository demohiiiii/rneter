# rneter

[![Crates.io](https://img.shields.io/crates/v/rneter.svg)](https://crates.io/crates/rneter)
[![Documentation](https://docs.rs/rneter/badge.svg)](https://docs.rs/rneter)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

[English Documentation](README.md)

`rneter` 是一个用于管理网络设备 SSH 连接的 Rust 库，具有智能状态机处理功能。它提供了高级 API 用于连接网络设备（路由器、交换机等）、执行命令以及管理设备状态，并具备自动提示符检测和模式切换功能。

## 特性

- **连接池管理**：自动缓存和重用 SSH 连接以提高性能
- **状态机管理**：智能设备状态跟踪和自动状态转换
- **提示符检测**：自动识别和处理不同设备类型的提示符
- **模式切换**：在设备模式（用户模式、特权模式、配置模式等）之间无缝转换
- **最大兼容性**：支持广泛的 SSH 算法，包括用于旧设备的传统协议
- **异步/等待**：基于 Tokio 构建，提供高性能异步操作
- **错误处理**：全面的错误类型with详细上下文信息

## 安装

在你的 `Cargo.toml` 中添加：

```toml
[dependencies]
rneter = "0.1"
```

## 快速开始

```rust
use rneter::session::{MANAGER, Command, CmdJob};
use rneter::templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 使用预定义的设备模板（例如：Cisco）
    let handler = templates::cisco();

    // 从管理器获取一个连接
    let sender = MANAGER.get(
        "admin".to_string(),
        "192.168.1.1".to_string(),
        22,
        "password".to_string(),
        None,
        handler,
    ).await?;

    // 执行命令
    let (tx, rx) = tokio::sync::oneshot::channel();
    let cmd = CmdJob {
        data: Command {
            cmd_type: "show".to_string(),
            mode: "Enable".to_string(), // Cisco 模板使用 "Enable" 模式
            command: "show version".to_string(),
            template: String::new(),
            timeout: Some(60),
        },
        sys: None,
        responder: tx,
    };
    
    sender.send(cmd).await?;
    let output = rx.await??;
    
    println!("命令执行成功: {}", output.success);
    println!("输出: {}", output.content);
    Ok(())
}
```

### 安全级别

`rneter` 现在支持安全默认值，并可在连接时自定义 SSH 安全级别：

```rust
use rneter::session::{ConnectionSecurityOptions, MANAGER};
use rneter::templates;

let handler = templates::cisco();

// 默认安全模式（known_hosts 校验 + 严格算法）
let _sender = MANAGER.get(
    "admin".to_string(),
    "192.168.1.1".to_string(),
    22,
    "password".to_string(),
    None,
    handler,
).await?;

// 显式指定安全配置
let _sender = MANAGER.get_with_security(
    "admin".to_string(),
    "192.168.1.1".to_string(),
    22,
    "password".to_string(),
    None,
    templates::cisco(),
    ConnectionSecurityOptions::legacy_compatible(),
).await?;
```

## 架构

### 连接管理

`SshConnectionManager` 提供了通过 `MANAGER` 常量访问的单例连接池。它可以自动：
- 缓存连接 5 分钟的不活动时间
- 在连接失败时重新连接
- 管理最多 100 个并发连接

### 状态机

`DeviceHandler` 实现了一个有限状态机：
- 使用正则表达式模式跟踪当前设备状态
- 使用 BFS 算法查找状态之间的最优路径
- 处理自动状态转换
- 支持特定系统状态（例如不同的 VRF 或上下文）

### 命令执行

命令通过基于异步通道的架构执行：
1. 向连接发送器提交一个 `CmdJob`
2. 库会在需要时自动转换到目标状态
3. 执行命令并等待提示符
4. 返回带有成功状态的输出

## 支持的设备类型

该库旨在与任何支持 SSH 的网络设备配合使用。特别适合：

- Cisco IOS/IOS-XE/IOS-XR 设备
- Juniper JunOS 设备
- Arista EOS 设备
- 华为 VRP 设备
- 通过 SSH 访问的通用 Linux/Unix 系统

## 配置

### SSH 算法支持

`rneter` 在 `config` 模块中包含全面的 SSH 算法支持：
- 密钥交换：Curve25519、DH 组、ECDH
- 加密：AES（CTR/CBC/GCM）、ChaCha20-Poly1305
- MAC：HMAC-SHA1/256/512 及 ETM 变体
- 主机密钥：Ed25519、ECDSA、RSA、DSA（用于旧设备）

这确保了与现代和传统网络设备的最大兼容性。

## 错误处理

该库通过 `ConnectError` 提供详细的错误类型：

- `UnreachableState`：无法从当前状态到达目标状态
- `TargetStateNotExistError`：请求的状态在配置中不存在
- `ChannelDisconnectError`：SSH 通道意外断开
- `ExecTimeout`：命令执行超时
- 等等...

## 文档

详细的 API 文档请访问 [docs.rs/rneter](https://docs.rs/rneter)。

## 许可证

本项目采用 MIT 许可证 - 详情请参阅 [LICENSE](LICENSE) 文件。

## 贡献

欢迎贡献！请随时提交 Pull Request。

## 作者

demohiiiii
