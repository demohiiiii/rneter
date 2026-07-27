# rneter

[![Crates.io](https://img.shields.io/crates/v/rneter.svg)](https://crates.io/crates/rneter)
[![Documentation](https://docs.rs/rneter/badge.svg)](https://docs.rs/rneter)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

[English Documentation](README.md)

`rneter` 是一个用于管理网络设备和 Linux 主机 SSH 连接的 Rust 库，采用显式的 Prompt 状态机执行模型。它的设计思路参考了 [Netmiko](https://github.com/ktbyers/netmiko)、[Scrapli](https://github.com/carlmontanari/scrapli) 和 [OpenSecFlow/netdriver](https://github.com/OpenSecFlow/netdriver)，解决的问题域与它们类似，但更强调正式的状态切换、可复用交互流程、事务回滚以及可回放的自动化工作流。

## 目录

- [特性](#特性)
- [安装](#安装)
- [快速开始](#快速开始)
- [Linux 主机管理](#linux-主机管理)
- [连接安全](#连接安全)
- [SSH 认证方式](#ssh-认证方式)
- [Fleet 批量执行](#fleet-批量执行)
- [文件传输](#文件传输)
- [命令流与交互](#命令流与交互)
- [会话录制与回放](#会话录制与回放)
- [事务工作流](#事务工作流)
- [虚拟设备测试能力（testkit）](#虚拟设备测试能力testkit)
- [模板与状态机生态](#模板与状态机生态)
- [架构](#架构)
- [生命周期 Hook](#生命周期-hook)
- [模板自动识别](#模板自动识别)
- [与 Netmiko 和 Scrapli 的对比](#与-netmiko-和-scrapli-的对比)
- [支持的设备类型](#支持的设备类型)
- [配置](#配置)
- [错误处理](#错误处理)
- [文档](#文档)
- [许可证](#许可证)
- [贡献](#贡献)
- [作者](#作者)

## 特性

- **连接池管理**：自动缓存和重用 SSH 连接以提高性能
- **Fleet 批量执行**：以受控并发在多台独立配置的设备上执行同一命令或流程，并隔离单设备错误
- **灵活的 SSH 认证**：通过 `SshAuthMethod` 支持密码、私钥（内联或文件）、ssh-agent 与键盘交互认证
- **状态机管理**：智能设备状态跟踪和自动状态转换
- **提示符检测**：自动识别和处理不同设备类型的提示符
- **模式切换**：在设备模式（用户模式、特权模式、配置模式等）之间无缝转换
- **生命周期 Hook**：支持在连接后、断开前以及状态切换前后声明式执行准备/清理操作
- **模板自动识别**：在创建完整状态机会话前，先对内置模板做探测打分和候选排序
- **SFTP 文件上传**：可向开启 SSH `sftp` 子系统的远端主机上传本地文件
- **多行命令执行**：可将换行命令拆分为独立设备操作，也可保留为一条命令执行
- **完整回显诊断**：通过 `Output.all` 查看命令回显和设备语法错误上下文
- **最大兼容性**：支持广泛的 SSH 算法，包括用于旧设备的传统协议
- **异步/等待**：基于 Tokio 构建，提供高性能异步操作
- **错误处理**：全面的错误类型和详细的上下文信息
- **虚拟设备测试套件**：可选的 `testkit` feature 提供进程内虚拟 SSH 设备，模仿全部内置模板，无需真实硬件即可测试基于 rneter 的自动化

## 安装

在你的 `Cargo.toml` 中添加：

```toml
[dependencies]
rneter = "0.4.7"
```

## 快速开始

使用内置模板连接设备并执行一条命令：

```rust
use rneter::session::{Command, ConnectionRequest, ExecutionContext, MANAGER};
use rneter::templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = MANAGER
        .execute_command_with_context(
            ConnectionRequest::new(
                "admin".to_string(),
                "192.168.1.1".to_string(),
                22,
                "password".to_string(),
                None,
                templates::cisco()?,
            ),
            Command {
                mode: "Enable".to_string(), // Cisco 模板使用 "Enable" 模式
                command: "show version".to_string(),
                timeout: Some(60),
                ..Command::default()
            },
            ExecutionContext::default(),
        )
        .await?;

    println!("命令执行成功: {}", output.success);
    println!("输出: {}", output.content);
    Ok(())
}
```

后续章节分别介绍 Linux 主机、文件传输、交互命令流、连接安全、会话录制和事务工作流。

## Linux 主机管理

`rneter` 支持 Linux 主机管理，并可按需配置提权方式：

```rust
use rneter::session::{ConnectionRequest, ExecutionContext, MANAGER, Command, CmdJob};
use rneter::templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let handler = templates::linux()?;

    let sender = MANAGER
        .get_with_context(
            ConnectionRequest::new(
                "user".to_string(),
                "192.168.1.100".to_string(),
                22,
                "ssh_password".to_string(),
                Some("your_privilege_password".to_string()),
                handler,
            ),
            ExecutionContext::default(),
        )
        .await?;

    let (tx, rx) = tokio::sync::oneshot::channel();
    sender
        .send(CmdJob {
            data: Command {
                mode: "User".to_string(),
                command: "ls -la /home".to_string(),
                timeout: Some(30),
                ..Command::default()
            },
            sys: None,
            responder: tx,
        })
        .await?;
    let output = rx.await??;
    println!("输出: {}", output.content);

    let (tx, rx) = tokio::sync::oneshot::channel();
    sender
        .send(CmdJob {
            data: Command {
                mode: "Root".to_string(),
                command: "systemctl restart nginx".to_string(),
                timeout: Some(30),
                ..Command::default()
            },
            sys: None,
            responder: tx,
        })
        .await?;
    let output = rx.await??;
    println!("重启结果: {}", output.content);

    Ok(())
}
```

Linux 模板默认使用 `DeviceShellFlavor::Posix`。如果远端登录 shell 是 `fish`，
可按下面的示例修改 `DeviceHandlerConfig.command_execution`。

**自定义配置：**

```rust
use rneter::device::{
    DeviceCommandExecutionConfig, DeviceShellFlavor, prompt_rule, transition_rule,
};
use rneter::templates::linux_handler_config;

// 将默认的 User -> Root `sudo -i` 切换命令替换为 `sudo -s`
let mut config = linux_handler_config();
config.edges = vec![
    transition_rule("User", "sudo -s", "Root", false, false),
    transition_rule("Root", "exit", "User", true, false),
];
let handler = config.build()?;

let mut config = linux_handler_config();
config.prompt = vec![
    prompt_rule("User", &[r"^myuser@myhost\$\s*$"]),
    prompt_rule("Root", &[r"^root@myhost#\s*$"]),
];
let handler = config.build()?;

let mut config = linux_handler_config();
config.command_execution = DeviceCommandExecutionConfig::ShellExitStatus {
    marker: "__RNETER_EXIT_CODE__:".to_string(),
    shell_flavor: DeviceShellFlavor::Fish,
};
let handler = config.build()?;
```

默认 Linux 模板使用 `sudo -i` 作为 `User -> Root` edge。需要使用其他提权方式时，
直接替换 `DeviceHandlerConfig.edges`；prompt 和命令执行策略也在同一个配置上修改。
直接以 root 登录时，prompt 会识别为 `Root`，不会执行该 edge。

## 连接安全

`rneter` 现在支持安全默认值，并可在连接时自定义 SSH 安全级别：

```rust
use rneter::session::{
    ConnectionRequest, ConnectionSecurityOptions, ExecutionContext, MANAGER,
};
use rneter::templates;

// 默认安全模式（known_hosts 校验 + 严格算法）
let _sender = MANAGER
    .get_with_context(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco()?,
        ),
        ExecutionContext::default(),
    )
    .await?;

// 显式指定安全配置
let _sender = MANAGER
    .get_with_context(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco()?,
        ),
        ExecutionContext::new()
            .with_security_options(ConnectionSecurityOptions::legacy_compatible()),
    )
    .await?;
```

## SSH 认证方式

密码认证仍是默认路径：`ConnectionRequest::new(...)`。
其他方式通过 `ConnectionRequest::new_with_auth(...)` 和 `SshAuthMethod` 构造：

```rust
use rneter::session::{
    ConnectionRequest, ExecutionContext, MANAGER, SshAuthMethod,
};
use rneter::templates;

// 私钥（内联 OpenSSH/PEM 内容）
let auth = SshAuthMethod::private_key(
    std::fs::read_to_string("/home/ops/.ssh/id_ed25519")?,
    None, // 可选口令
);
let _sender = MANAGER
    .get_with_context(
        ConnectionRequest::new_with_auth(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            auth,
            None,
            templates::cisco()?,
        ),
        ExecutionContext::default(),
    )
    .await?;

// 私钥文件路径（连接时加载）
let auth = SshAuthMethod::private_key_file("/home/ops/.ssh/id_ed25519", None);

// 本地 ssh-agent（仅 Unix）
#[cfg(not(target_os = "windows"))]
let auth = SshAuthMethod::agent();

// 键盘交互：匹配任意包含该片段的服务端提示
let auth = SshAuthMethod::keyboard_interactive(vec![
    ("Password".to_string(), "secret".to_string()),
    ("OTP".to_string(), "123456".to_string()),
]);
```

自动识别同样支持 `DetectRequest::new_with_auth(...)`。
连接池会把认证方式纳入参数指纹，因此更换凭据一定会重建连接。

## Fleet 批量执行

`execute_on_fleet(...)` 会在一组独立配置的目标上执行同一个
`SessionOperation`。并发上限控制同时执行的目标数量，单台设备失败不会
取消其他设备，返回结果始终保持输入顺序：

```rust
use rneter::session::{
    Command, ConnectionRequest, ExecutionContext, FleetOptions, FleetTarget,
    MANAGER, SessionOperation,
};
use rneter::templates;

let targets = vec![
    FleetTarget::new(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.10".to_string(),
            22,
            "password-a".to_string(),
            None,
            templates::cisco()?,
        ),
        ExecutionContext::default(),
    ),
    FleetTarget::new(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.11".to_string(),
            22,
            "password-b".to_string(),
            None,
            templates::cisco()?,
        ),
        ExecutionContext::default(),
    ),
];
let operation = SessionOperation::from(Command {
    mode: "Enable".to_string(),
    command: "show version".to_string(),
    timeout: Some(60),
    ..Command::default()
});

let results = MANAGER
    .execute_on_fleet(targets, operation, FleetOptions::new(16))
    .await?;
for target in results {
    match target.result {
        Ok(output) => println!("{}: success={}", target.device_addr, output.success),
        Err(error) => eprintln!("{}: {error}", target.device_addr),
    }
}
```

外层 `Result` 只报告 Fleet/操作配置错误或内部任务异常。连接和执行错误会
保留在各自的 `FleetExecutionResult` 中，命令流已经产生的部分输出也不会丢失。

## 文件传输

### SFTP 文件上传

如果远端主机启用了 SSH `sftp` 子系统，`rneter` 可以在同一条认证过的 SSH 连接上上传本地文件：

```rust
use rneter::session::{ConnectionRequest, ExecutionContext, FileUploadRequest, MANAGER};
use rneter::templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let handler = templates::linux()?;

    MANAGER
        .upload_file_with_context(
            ConnectionRequest::new(
                "user".to_string(),
                "192.168.1.100".to_string(),
                22,
                "ssh_password".to_string(),
                None,
                handler,
            ),
            FileUploadRequest::new(
                "./artifacts/config.backup".to_string(),
                "/tmp/config.backup".to_string(),
            )
            .with_timeout_secs(30)
            .with_buffer_size(16 * 1024)
            .with_progress_reporting(true),
            ExecutionContext::default(),
        )
        .await?;

    Ok(())
}
```

这条路径要求远端支持 SFTP。对于只支持 `copy scp:`、`copy tftp:` 这类 CLI 传输命令的
网络设备，调用方可以直接构建带命令级 `CommandInteraction` 规则的 `CommandFlow`，再交给
通用的 command-flow 执行 API。

## 命令流与交互

交互行为分布在两个不同的执行边界：

```text
CommandFlow
  -> Command
       -> CommandInteraction
            -> PromptResponseRule
```

- `CommandInteraction` 用于一条命令仍在执行、设备尚未返回正常 prompt 时，回答中间询问。
- `CommandFlow` 用于按声明顺序执行多条完整命令。每条命令返回正常设备 prompt 后，才开始下一条命令。
- Flow 内的每条命令都可以定义自己的 `CommandInteraction`，两者是组合关系，不是替代关系。

| 机制 | 定义 prompt 匹配规则 | 定义回复值 | 生命周期 | 适用场景 |
| --- | --- | --- | --- | --- |
| 模板 `write` / `input_rule` | 是 | 静态值或动态 key | 整个 handler/session | 设备系列通用提示，例如 enable 或 sudo 密码 |
| `Command.dyn_params` | 否 | 是 | 仅当前命令 | 临时覆盖模板 `input_rule` 使用的动态值 |
| `Command.interaction` | 是 | 是 | 仅当前命令 | 当前命令特有提示，例如文件名和覆盖确认 |
| `CommandFlow` | 否 | 否 | 多条完整命令 | 顺序执行、逐命令 mode/timeout 和遇错停止 |

当前命令执行期间的提示处理顺序如下：

```text
正常设备 prompt
  -> 命令级 interaction 规则
  -> 模板级 write/input 规则
  -> 继续等待输出
```

匹配正常设备 prompt 会结束当前命令，因此 interaction 规则用于处理中间询问，不用于开始另一条命令。运行时 interaction 正则会在命令执行前编译；无效表达式会返回 `ConnectError::InvalidCommandInteraction`。

模板已经知道如何匹配提示，但某条命令需要临时回复值时，使用 `dyn_params`：

```rust
use rneter::session::{Command, CommandDynamicParams};

let command = Command {
    mode: "Enable".to_string(),
    command: "copy protected-config startup-config".to_string(),
    dyn_params: CommandDynamicParams {
        enable_password: Some("temporary-secret\n".to_string()),
        ..CommandDynamicParams::default()
    },
    ..Command::default()
};
```

命令级动态值和 interaction 回复都会按原样发送。远端提示需要立即提交回复时，应自行包含结尾换行符。命令结束后会恢复原有动态值，因此不会永久覆盖连接级参数。

提示本身只属于当前命令时，使用 `CommandInteraction`：

```rust
use rneter::session::{Command, CommandInteraction, PromptResponseRule};

let command = Command {
    mode: "Enable".to_string(),
    command: "copy running-config startup-config".to_string(),
    interaction: CommandInteraction::default()
        .push_prompt(PromptResponseRule::new(
            vec![r"(?i)^Destination filename.*\?\s*$".to_string()],
            "\n".to_string(),
        ))
        .push_prompt(PromptResponseRule::new(
            vec![r"(?i)^Overwrite.*\?\s*$".to_string()],
            "yes\n".to_string(),
        )),
    ..Command::default()
};
```

`record_input` 决定匹配到的提示是否保留在捕获输出中。密码类提示应保持为 `false`；需要保留非敏感交互上下文时可以启用。

### 多行命令

多行策略由 `Command` 自身携带。`SplitLines` 是默认策略：每个去除首尾空白后的非空行
都会成为一条独立命令，分别等待 prompt 并产生输出。结果可能包含多个步骤时，使用
`execute_multiline_command_with_context(...)` 下发。

```rust
use rneter::session::{
    Command, ConnectionRequest, ExecutionContext, MANAGER, MultilineMode,
};
use rneter::templates;

let result = MANAGER
    .execute_multiline_command_with_context(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco()?,
        ),
        Command {
            mode: "Enable".to_string(),
            command: "show version\nshow inventory\nshow interfaces".to_string(),
            timeout: Some(60),
            ..Command::default()
        },
        ExecutionContext::default(),
    )
    .await?;

for step in &result.steps {
    println!(
        "step={} command={} success={} output={}",
        step.step_index, step.operation_summary, step.success, step.content
    );
}
```

heredoc、脚本块或其他必须作为一条命令发送的输入，应在 command 上设置
`.with_multiline_mode(MultilineMode::Whole)`；此时 `result.steps` 只包含一个输出。发生超时或断连时会返回
`SessionOperationExecutionError`，已经完成的行仍可通过 `partial_output()` 读取。

### 自定义交互命令流程

设备流程需要多条完整命令时，可以直接构建 `CommandFlow`，并为包含中间询问的步骤挂载运行时 `PromptResponseRule`。Flow 在同一个活动连接上执行，每条命令可以使用独立 mode 和 timeout；默认遇到首个失败步骤就停止，也可以通过 `with_max_steps(...)` 限制最大步骤数：

```rust
use rneter::session::{
    Command, CommandFlow, CommandInteraction, ConnectionRequest, ExecutionContext, MANAGER,
    PromptResponseRule,
};
use rneter::templates;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let flow = CommandFlow::new(vec![Command {
        mode: "Enable".to_string(),
        command: "copy http: flash:/image.bin".to_string(),
        timeout: Some(600),
        interaction: CommandInteraction::default()
            .push_prompt(PromptResponseRule::new(
                vec![r"(?i)^Address or name of remote host.*\?\s*$".to_string()],
                "203.0.113.10\n".to_string(),
            ))
            .push_prompt(PromptResponseRule::new(
                vec![r"(?i)^Source (?:file ?name|filename).*\?\s*$".to_string()],
                "/pub/image.bin\n".to_string(),
            ))
            .push_prompt(
                PromptResponseRule::new(
                    vec![r"(?i)^Destination (?:file ?name|filename).*\?\s*$".to_string()],
                    "\n".to_string(),
                )
                .with_record_input(true),
            ),
        ..Command::default()
    },
    Command {
        mode: "Enable".to_string(),
        command: "verify /md5 flash:/image.bin".to_string(),
        timeout: Some(300),
        ..Command::default()
    }])
    .with_stop_on_error(true)
    .with_max_steps(10);

    let result = MANAGER
        .execute_command_flow_with_context(
            ConnectionRequest::new(
                "admin".to_string(),
                "192.168.1.1".to_string(),
                22,
                "password".to_string(),
                None,
                templates::cisco()?,
            ),
            flow,
            ExecutionContext::default(),
        )
        .await?;

    if let Some(last) = result.outputs.last() {
        println!("最后一步输出: {}", last.content);
    }
    Ok(())
}
```

运行时 prompt-response 规则会优先于模板里的静态输入规则生效，所以后续新增 `scp`、`tftp`、`http` 这类向导式 CLI 交互时，通常不需要再改底层模板定义。
每条命令到达正常 prompt 后，Flow 才会按声明顺序进入下一步。每一步都会在 `CommandFlowOutput.outputs` 中产生独立输出。

## 会话录制与回放

```rust
use rneter::session::{
    ConnectionRequest, ExecutionContext, MANAGER, SessionRecordLevel, SessionReplayer,
};
use rneter::templates;

let (sender, recorder) = MANAGER
    .get_with_recording_level_and_context(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco()?,
        ),
        ExecutionContext::default(),
        SessionRecordLevel::Full,
    )
    .await?;

// 实时订阅后续录制事件
let mut rx = recorder.subscribe();
tokio::spawn(async move {
    while let Ok(entry) = rx.recv().await {
        println!("实时事件: {:?}", entry.event);
    }
});

// 或者仅记录关键事件（不记录原始 shell 分块）
let (_sender2, _recorder2) = MANAGER
    .get_with_recording_level_and_context(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco()?,
        ),
        ExecutionContext::default(),
        SessionRecordLevel::KeyEventsOnly,
    )
    .await?;

// ...通过 `sender` 发送 CmdJob...

// 导出为 JSONL
let jsonl = recorder.to_jsonl()?;

// 恢复并离线回放
let restored = rneter::session::SessionRecorder::from_jsonl(&jsonl)?;
let mut replayer = SessionReplayer::from_recorder(&restored);
let replayed_output = replayer.replay_next("show version")?;
println!("回放输出: {}", replayed_output.content);

// 无需真实 SSH 的离线命令流程测试
let script = vec![
    rneter::session::Command {
        mode: "Enable".to_string(),
        command: "terminal length 0".to_string(),
        timeout: None,
        ..rneter::session::Command::default()
    },
    rneter::session::Command {
        mode: "Enable".to_string(),
        command: "show version".to_string(),
        timeout: None,
        ..rneter::session::Command::default()
    },
];
let outputs = replayer.replay_script(&script)?;
assert_eq!(outputs.len(), 2);
```

对于 CI 离线测试，可以把 JSONL 录制文件放入 `tests/fixtures/`，并在集成测试中回放（参考 `tests/replay_fixtures.rs`）。可以用下面的命令将线上噪声录制归一化为稳定 fixture：

```bash
cargo run --example normalize_fixture -- raw_session.jsonl tests/fixtures/session_new.jsonl
```

## 事务工作流

事务能力由四层模型组成：

```text
SessionOperation -> TxStep -> TxBlock -> TxWorkflow
```

- `SessionOperation` 是一个已经确定的可执行单元，可以是 `Command` 或 `CommandFlow`。
- `TxStep` 将正向操作与可选的补偿操作关联起来。
- `TxBlock` 按顺序执行一组相关步骤，并应用一个显式回滚策略。
- `TxWorkflow` 按顺序执行多个 block；后续 block 失败时，已提交 block 会按相反顺序执行补偿。

这里的事务是应用层补偿事务，不是数据库事务。设备已经接受的命令不会被原子撤销，回滚操作本身也可能失败。事务行为不会根据命令文本自动推断：调用方必须明确选择策略，并提供该策略所需的补偿操作。

### 回滚策略

| 策略 | 行为 | 典型场景 |
| --- | --- | --- |
| `RollbackPolicy::None` | 不尝试回滚 | 只读操作，或由 rneter 之外的系统管理变更 |
| `RollbackPolicy::WholeResource` | 执行一个 block 级补偿操作 | 可以通过单个操作撤销的创建或更新流程 |
| `RollbackPolicy::PerStep` | 按相反顺序执行可用的补偿操作 | 各步骤具有独立逆操作的多步变更 |

对于 `WholeResource`，`trigger_step_index` 表示必须成功执行哪个正向步骤后，整体回滚才有效。对于 `PerStep`，`rollback_on_failure` 决定是否尝试补偿失败步骤本身；此前已成功的步骤仍按执行顺序的反向顺序处理。

```rust
let block = TxBlock {
    name: "interface-update".to_string(),
    rollback_policy: RollbackPolicy::PerStep,
    steps: vec![
        TxStep::new(Command {
            mode: "Config".to_string(),
            command: "interface ethernet 1/1".to_string(),
            ..Command::default()
        }),
        TxStep::new(Command {
            mode: "Config".to_string(),
            command: "description uplink".to_string(),
            ..Command::default()
        })
        .with_rollback(Command {
            mode: "Config".to_string(),
            command: "no description".to_string(),
            ..Command::default()
        })
        .with_rollback_on_failure(true),
    ],
    fail_fast: true,
};
```

`PerStep` 允许某些步骤不提供 rollback；无法生成补偿计划时，结果中会记录为跳过。

### 失败处理顺序

使用推荐配置 `fail_fast: true` 时，失败处理顺序如下：

```text
正向步骤失败
  -> 停止当前 block
  -> 执行当前 block 的回滚策略
  -> 将 workflow 标记为失败
  -> 按相反顺序补偿此前已提交的 block
  -> 分别返回正向执行与回滚结果
```

在 block 层，`fail_fast` 会在首次失败后停止剩余步骤；在 workflow 层，它会在首个 block 失败后停止启动后续 block。需要统一成败语义时应保持开启。

### 构建并执行单个事务块

下面的 block 用于创建地址对象。步骤 `0` 成功后，如果后续步骤失败，整体回滚操作会删除该对象：

```rust
use rneter::session::{
    Command, CommandFlow, ConnectionRequest, ExecutionContext, MANAGER,
    RollbackPolicy, TxBlock, TxStep,
};
use rneter::templates;

let block = TxBlock {
    name: "addr-create".to_string(),
    rollback_policy: RollbackPolicy::WholeResource {
        rollback: Box::new(
            Command {
                mode: "Config".to_string(),
                command: "no object network WEB01".to_string(),
                timeout: Some(30),
                ..Command::default()
            }
            .into(),
        ),
        trigger_step_index: 0,
    },
    steps: vec![
        TxStep::new(Command {
            mode: "Config".to_string(),
            command: "object network WEB01".to_string(),
            timeout: Some(30),
            ..Command::default()
        }),
        TxStep::new(CommandFlow::new(vec![
            Command {
                mode: "Config".to_string(),
                command: "host 10.0.0.10".to_string(),
                timeout: Some(30),
                ..Command::default()
            },
            Command {
                mode: "Config".to_string(),
                command: "description WEB01".to_string(),
                timeout: Some(30),
                ..Command::default()
            },
        ])),
    ],
    fail_fast: true,
};

let result = MANAGER
    .execute_tx_block_with_context(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco()?,
        ),
        block,
        ExecutionContext::default(),
    )
    .await?;
println!(
    "committed={}, rollback_succeeded={}",
    result.committed, result.rollback_succeeded
);
```

### 步骤操作

`TxStep::new(...)` 接受单条命令或一个已经确定的 `CommandFlow`：

包含多行文本的 `Command` 会根据自身的 `multiline_mode` 自动展开。默认 `SplitLines` 会把
每个非空行作为同一个事务步骤里的子操作；必须保持整体发送时设置为 `Whole`。

```rust
let verify_step = TxStep::new(Command {
    mode: "Enable".to_string(),
    command: "show running-config\nshow startup-config".to_string(),
    ..Command::default()
});

let summary = verify_step.run.summary()?;
println!(
    "kind={} mode={} steps={} desc={}",
    summary.kind, summary.mode, summary.step_count, summary.description
);
```

### 多块工作流

“地址对象 -> 服务对象 -> 策略”这类有顺序依赖的场景可以使用 `TxWorkflow`。某个 block 失败时，会先执行该 block 自身的回滚策略，然后依据各 block 的策略，按相反顺序补偿此前已提交的 block。

```rust
use rneter::session::{TxWorkflow, TxWorkflowResult};

let workflow = TxWorkflow {
    name: "fw-policy-publish".to_string(),
    blocks: vec![addr_block, svc_block, policy_block],
    fail_fast: true,
};

let workflow_result: TxWorkflowResult = MANAGER
    .execute_tx_workflow_with_context(
        ConnectionRequest::new(
            "admin".to_string(),
            "192.168.1.1".to_string(),
            22,
            "password".to_string(),
            None,
            templates::cisco()?,
        ),
        workflow,
        ExecutionContext::default(),
    )
    .await?;

for block in &workflow_result.block_results {
    for step in &block.step_results {
        println!(
            "step[{}] op={} execution={:?} rollback={:?}",
            step.step_index,
            step.operation_summary,
            step.execution_state,
            step.rollback_state
        );
        for child in &step.forward_operation_steps {
            println!(
                "  forward_step[{}] op={} success={}",
                child.step_index, child.operation_summary, child.success
            );
        }
        for child in &step.rollback_operation_steps {
            println!(
                "  rollback_step[{}] op={} success={}",
                child.step_index, child.operation_summary, child.success
            );
        }
    }
    if let Some(block_rollback) = &block.block_rollback_operation_summary {
        println!("block_rollback={block_rollback}");
        for child in &block.block_rollback_steps {
            println!(
                "  block_rollback_step[{}] op={} success={}",
                child.step_index, child.operation_summary, child.success
            );
        }
    }
}
```

### 检查执行结果

`TxResult` 和 `TxWorkflowResult` 同时保留正向执行与回滚细节：

- Block 结果：`committed`、`failed_step`、`failure_reason`
- 回滚结果：`rollback_attempted`、`rollback_succeeded`、`rollback_errors`
- Step 结果：`execution_state`、`failure_reason`、`rollback_state`、`rollback_reason`
- 嵌套操作输出：`forward_operation_steps`、`rollback_operation_steps`、`block_rollback_steps`

调用方可以据此区分正向操作失败、回滚被跳过，以及已尝试但执行失败的回滚。

### 便捷构建函数

`templates::build_tx_block` 仅负责将命令字符串列表转换为 `TxStep`。事务回滚策略必须由调用方通过 `RollbackPolicy` 显式指定：

```rust
use rneter::session::{Command, RollbackPolicy};

let cmds = vec![
    "object network WEB01".to_string(),
    "host 10.0.0.10".to_string(),
];
let block = templates::build_tx_block(
    "addr-create",
    "Config",
    &cmds,
    Some(30),
    RollbackPolicy::WholeResource {
        rollback: Box::new(Command {
            mode: "Config".to_string(),
            command: "no object network WEB01".to_string(),
            timeout: Some(30),
            ..Command::default()
        }.into()),
        trigger_step_index: 0,
    },
)?;
```

### 使用建议

- 补偿操作应尽量保持幂等，避免重试造成额外影响。
- 只有在回滚目标资源确定已经创建后，才应设置对应的 `trigger_step_index`。
- 仅当失败步骤可能部分生效且能够安全补偿时，才启用 `rollback_on_failure`。
- 回滚失败应作为独立的运维事件处理；事务失败不等于状态一定已经恢复。
- 手动构造 block 或 workflow 时，建议在执行前调用校验方法。
- 使用会话录制保留审计信息，并离线回放正向执行和回滚行为。

### 录制与审计

启用会话录制后，事务执行会产生 block、step、rollback 和 workflow 生命周期事件。主要事件类型包括 `tx_block_started`、`tx_step_succeeded`、`tx_step_failed`、`tx_rollback_started`、`tx_rollback_step_succeeded`、`tx_rollback_step_failed`、`tx_block_finished`、`tx_workflow_started` 和 `tx_workflow_finished`。

```json
{
  "kind": "tx_block_finished",
  "block_name": "addr-create",
  "committed": false,
  "rollback_attempted": true,
  "rollback_succeeded": true
}
```

如果只需要审计生命周期结果，可以使用 `SessionRecordLevel::KeyEventsOnly`；需要排查原始数据块和详细命令输出时，使用 `Full`。

## 虚拟设备测试能力（testkit）

可选的 `testkit` feature 提供进程内虚拟 SSH 设备，让基于 `rneter` 的上层应用无需真实硬件即可测试自动化逻辑。虚拟设备是一个真正的 SSH 服务器（基于 `russh`，启动时生成一次性主机密钥），提供脚本化的 CLI，因此完整链路都会被真实执行：SSH 握手、提示符探测、状态机转换、生命周期 Hook、会话录制与事务。

在 `dev-dependencies` 中启用：

```toml
[dev-dependencies]
rneter = { version = "0.4.7", features = ["testkit"] }
```

每个内置模板都有现成的 persona。模拟设备的状态机与客户端模板派生自同一份 `DeviceHandlerConfig`，模板变更永远不会与模拟实现悄悄脱节：

```rust
use rneter::session::{Command, SshConnectionManager};
use rneter::testkit::{DevicePersona, FakeSshDevice};

#[tokio::test]
async fn my_automation_works_on_cisco() -> Result<(), Box<dyn std::error::Error>> {
    let device = FakeSshDevice::spawn(DevicePersona::builtin("cisco_ios")?).await?;
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device.connection_request()?,
            Command {
                mode: "Enable".to_string(),
                command: "show version".to_string(),
                ..Command::default()
            },
            device.execution_context(),
        )
        .await?;
    assert!(output.success);

    // 从设备视角断言：哪些命令真正到达了设备
    assert!(device.received_commands().contains(&"show version".to_string()));
    Ok(())
}
```

常用入口：

- `DevicePersona::builtin(name)`——全部内置模板的现成 persona，模仿真实设备：主机名风格的提示符（`Router#`、`<HUAWEI>`、`FGT60F #` 等）、厂商版本命令的真实回显（`show version`、`display version`、`get system status` 等）、enable/sudo 密码质询，以及厂商风格的错误输出（发送 `testkit::ERROR_COMMAND` 触发）。
- `DevicePersona::with_canned_reply(command, output)`——为任意 persona 追加更多真实命令回显。
- `DevicePersona::for_config(...)`——模拟自定义 `DeviceHandlerConfig`，可通过 builder 方法追加质询和错误文案。
- `FakeSshDevice::received_commands()`——设备侧命令日志，适合断言状态转换顺序与事务回滚顺序。
- `device.connection_request()` / `device.execution_context()`——为已启动的虚拟设备预接线的连接参数。
- `FakeSshDevice::spawn_on(persona, addr)`——绑定到固定端口，让外部进程（或原生 `ssh` 客户端）直接连接。
- `builtin_personas()`——一次性获取全部内置 persona，用于批量起舰队或矩阵测试。

### 默认凭据

| 项目 | 常量 | 值 |
| --- | --- | --- |
| 登录用户名 | `DEFAULT_USERNAME` | `admin` |
| 登录密码 | `DEFAULT_PASSWORD` | `testkit-login-pw` |
| enable/sudo 密码 | `DEFAULT_ENABLE_PASSWORD` | `testkit-enable-pw` |

以上均为 persona 的公开字段，可按需覆盖。

### 虚拟设备的行为规则

- **状态机转换**：收到模板转换命令（`enable`、`system-view`、`configure terminal` 等）时按状态机切换提示符；需要密码的转换会先发出质询（`Password:`、`[sudo] password for admin:` 等），并校验应答。
- **仿真命令**：命中 persona 内置（或 `with_canned_reply` 追加）的命令时，返回厂商真实格式的多行回显。
- **未知命令**：返回 `benign_reply`（默认 `testkit-ok sample output`），判定为执行成功——上层测试可以放心发送任意配置命令。
- **`make-error`**（`testkit::ERROR_COMMAND`）：返回该厂商风格的错误文案（linux 为退出码 1），用于测试错误检测与事务回滚路径。
- **行终止符**：同时兼容自动化客户端的 `\n` 和交互式 SSH 终端的 `\r`，因此可以直接用 `ssh` 人工登录调试。
- 注意：与命令文本相同或互为前缀的输出行会被 rneter 的回显过滤器从 `Output.content` 中滤除（如 NX-OS 的 `!Command: ...` 首行），需要原始数据时请读取 `Output.all`。

### 各内置 persona 的提示符与仿真命令

| 模板 | 提示符风格 | 仿真命令 |
| --- | --- | --- |
| `cisco_ios` / `cisco_xe` | `Router>` `Router#` `Router(config)#` | `show version` · `show running-config` · `show ip interface brief` |
| `cisco_asa` | `ciscoasa>` `ciscoasa#` | `show version` · `show running-config` · `show interface ip brief` |
| `cisco_nxos` | `switch>` `switch#` | `show version` · `show running-config` · `show interface brief` |
| `arista_eos` | `switch>` `switch#` | `show version` · `show running-config` · `show interfaces status` |
| `aruba_aoscx` | `switch>` `switch#` | `show version` · `show running-config` · `show interface brief` |
| `dell_os10` | `OS10>` `OS10#` | `show version` · `show running-configuration` · `show interface status` |
| `juniper_junos` | `admin@SRX>` `admin@SRX#` | `show version` · `show configuration` · `show interfaces terse` |
| `fortinet` | `FGT60F #` | `get system status` · `show system interface` · `get system performance status` |
| `paloalto_panos` | `admin@PA-3220>` `admin@PA-3220#` | `show system info` · `show config running` · `show interface all` |
| `checkpoint_gaia` | `gw-13800b>` | `show version all` · `show configuration` · `show interfaces all` |
| `huawei` | `<HUAWEI>` `[HUAWEI]` | `display version` · `display current-configuration` · `display interface brief` |
| `h3c_comware` / `hp_comware` | `<H3C>` `[H3C]` | `display version` · `display current-configuration` · `display interface brief` |
| `hillstone_stoneos` | `SG-6000#` `SG-6000(config)#` | `show version` · `show configuration` · `show interface` |
| `topsec` | `TopsecOS#` | `system version show` · `system config show` · `network interface show` |
| `dptech` | `<DPTECH>` `[DPTECH]` | `show version` · `show running-config` · `show interface brief` |
| `qianxin` | `QiAnXin>` `QiAnXin-config]` | `show version` · `show running-config` · `show interface` |
| `venustech` | `USG>` `USG#` | `show version` · `show running-config` · `show interface` |
| `chaitin` | `safeline>` `safeline#` | `show version` · `show running-config` · `show interface` |
| `zte_zxros` | `ZXR10>` `ZXR10#` | `show version` · `show running-config` · `show ip interface brief` |
| `maipu` | `MyPower>` `MyPower#` | `show version` · `show running-config` · `show ip interface brief` |
| `ruijie_os` | `Ruijie>` `Ruijie#` | `show version` · `show running-config` · `show ip interface brief` |
| `array` | `AN>` `AN#` + 虚拟站点 `vs1$` | `show version` · `show running-config` · `show interface` |
| `linux` | `admin@debian:~$` `root@debian:~#` | `uname -a` · `ip -brief address` · `cat /etc/os-release` |

虚拟设备也可以通过自带的 example 作为独立服务运行——每个内置模板一台，或自定义一台：

```bash
# 列出全部内置设备 persona
cargo run --example virtual_device --features testkit -- --list

# 在固定端口运行一台虚拟设备
cargo run --example virtual_device --features testkit -- cisco_ios 2201

# 运行舰队：每个内置模板一台（端口 2200..2224）
cargo run --example virtual_device --features testkit -- --all 2200

# 运行自定义设备类型（自定义提示符/转换/错误风格）
cargo run --example virtual_device --features testkit -- --custom 2300

# 然后在任意终端：
ssh -p 2201 admin@127.0.0.1   # 密码: testkit-login-pw
```

## 模板与状态机生态

你可以把内置模板当作注册表管理，并直接对状态图做诊断：

```rust
use rneter::templates;

let names = templates::available_templates();
assert!(names.contains(&"cisco_ios"));

let _handler = templates::by_name("juniper_junos")?; // 大小写不敏感，旧别名也仍可用

let report = templates::diagnose_template("cisco_ios")?;
println!("是否存在问题: {}", report.has_issues());
println!("死路状态: {:?}", report.dead_end_states);

let catalog = templates::template_catalog();
println!("模板数量: {}", catalog.len());

let all_json = templates::diagnose_all_templates_json()?;
println!("全部诊断 JSON 字节数: {}", all_json.len());
```

也可以先导出内置模板配置，再按需扩展后重新构建：

```rust
use rneter::device::prompt_rule;
use rneter::templates;

let mut config = templates::by_name_config("cisco_ios")?;
config
    .prompt
    .push(prompt_rule("CustomMode", &[r"^custom>\s*$"]));

let handler = config.build()?;
assert!(handler.states().iter().any(|state| state == "custommode"));
```

新增的录制/回放能力：

- Prompt 前后态：每条 `command_output` 都记录 `prompt_before`/`prompt_after`
- 状态机 prompt 前后态：事件可记录 `fsm_prompt_before`/`fsm_prompt_after`
- 返回值带 prompt：命令执行与离线回放的 `Output` 现在包含 `prompt`
- 兼容旧 schema：历史 `connection_established` 的 `prompt`/`state` 字段仍可读取
- fixture 测试工作流：`tests/fixtures/` 提供成功流/失败流/状态切换样本，`tests/replay_fixtures.rs` 提供快照与质量校验

`command_output` 事件结构示例：

```json
{
  "kind": "command_output",
  "command": "show version",
  "mode": "Enable",
  "prompt_before": "router#",
  "prompt_after": "router#",
  "fsm_prompt_before": "enable",
  "fsm_prompt_after": "enable",
  "success": true,
  "content": "Version 1.0",
  "all": "show version\nVersion 1.0\nrouter#"
}
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

#### 设计思路

这个状态机的设计基于网络设备自动化里的两个稳定事实：

1. 相比命令文本，Prompt 更适合判断当前模式。
2. 不同厂商/型号的模式切换路径不同，路径搜索必须数据驱动。

核心设计选择：

- 状态统一小写，并将 prompt 正则匹配结果映射到状态索引，保证快速定位。
- 将 prompt 检测（`read_prompt`）与状态更新（`read`）拆开，保证命令循环行为可预测。
- 将状态转换建模为有向图（`edges`），通过 BFS 找到最短可行切换路径。
- 将动态输入处理（`read_need_write`）与命令逻辑解耦，复用密码/确认类交互处理。
- 同时记录 CLI prompt 文本与 FSM prompt（状态名），便于在线诊断和离线回放断言。

这样设计的好处：

- 可移植性更好：设备差异主要通过配置表达，而不是硬编码分支。
- 稳定性更好：执行依赖 prompt/状态收敛，而不是脆弱的输出格式假设。
- 可测试性更好：可通过 record/replay 离线验证状态切换与 prompt 演化，不依赖真实 SSH。

#### 状态转换模型

```mermaid
flowchart LR
    O["Output"] --> L["Login Prompt"]
    L -->|enable| E["Enable Prompt"]
    E -->|configure terminal| C["Config Prompt"]
    C -->|exit| E
    E -->|exit| L
    E -->|show ...| E
    C -->|show ... / set ...| C
```

#### 命令执行流程（带状态感知）

```mermaid
flowchart TD
    A["接收命令(mode, command, timeout)"] --> B["读取当前 FSM prompt/state"]
    B --> C["BFS 规划切换路径: trans_state_write(target_mode)"]
    C --> D["按顺序执行切换命令"]
    D --> E["执行目标命令"]
    E --> F["读取流式输出 -> handler.read(line) 更新状态"]
    F --> G{"匹配到 prompt?"}
    G -->|否| F
    G -->|是| H["构建 Output(success, content, all, prompt)"]
    H --> I["记录事件: prompt_before/after + fsm_prompt_before/after"]
```

### 命令执行

命令通过基于异步通道的架构执行：

1. 向连接发送器提交一个 `CmdJob`
2. 库会在需要时自动转换到目标状态
3. 执行命令并等待提示符
4. 返回带有成功状态的输出

调用方传入的 mode 名称会在内部统一转成小写匹配，因此 `"Enable"`、`"enable"`、`"ENABLE"` 都会指向同一个 FSM 状态。

## 生命周期 Hook

`rneter` 现在可以通过 `DeviceHandlerConfig.hooks` 声明生命周期 Hook：

- `after_connect`
- `before_disconnect`
- `after_enter_state`
- `before_exit_state`

Hook 复用了 `SessionOperation`，因此既可以执行单条命令，也可以执行命令流。在 `0.4.4` 中，连接级 Hook 先限定为模板级能力，这样就不会和连接缓存复用产生行为歧义；状态级 Hook 则会自动按内部小写 FSM 状态名做归一化匹配。

内置模板也可以提供默认行为，例如：

- Cisco/ASA 会在连接后执行 `terminal pager 0`
- Juniper 会在连接后执行 `set cli screen-length 0`

Hook 的输出不会并入父命令返回结果，但 Hook 的生命周期事件会进入 session recorder。

## 模板自动识别

`rneter` 现在可以在真正创建 `DeviceHandler` 之前，先对内置模板做自动识别和排序。

自动识别返回的是一份候选报告，而不是一个不可解释的单值结果，核心字段包括：

- `best_match`
- `candidates`
- `raw_facts`

这样在现场环境里更容易理解“为什么它更像 Cisco IOS/IOS-XE / Juniper Junos / Huawei / H3C/HP Comware / Linux / Arista EOS / Aruba AOS-CX / Cisco ASA/NX-OS / Dell OS10 / Ruijie OS / ZTE ZXROS / Fortinet / Palo Alto PAN-OS / Check Point Gaia”，也更方便排查误判。

当前范围：

- 仅支持 SSH
- 当前已覆盖的内置模板：`cisco_ios`、`cisco_xe`、`juniper_junos`、`huawei`、`h3c_comware`、`hp_comware`、`linux`、`hillstone_stoneos`、`arista_eos`、`aruba_aoscx`、`cisco_asa`、`cisco_nxos`、`dell_os10`、`fortinet`、`paloalto_panos`、`ruijie_os`、`zte_zxros`、`checkpoint_gaia`
- 旧的 rneter 名称如 `cisco`、`juniper`、`h3c`、`hillstone`、`arista`、`paloalto`、`ruijie`、`checkpoint` 仍然可作为别名使用
- `cisco_asa` 作为独立模板名和自动识别目标暴露，但当前复用已经验证过的 `cisco_ios` handler 行为
- 基于初始 prompt/输出和只读 probe 命令做缓存式打分

如何理解诊断结果：

- `raw_facts` 现在同时包含“正向命中”和“probe 错误命中”两类事实。
- 正向事实表示某条 prompt 或 probe 输出命中了加分正则，因此会贡献分数。
- 错误事实表示这条 probe 输出命中了 `Invalid input`、`Unrecognized command`、`command not found` 之类的错误模式；此时该 probe 会像 Netmiko 的 autodetect 一样，被视为无效而不参与加分。
- 这样更容易区分“这台设备不像 Cisco”和“Cisco 的探测命令在这里根本不成立”这两种情况。

示例：

```rust
use rneter::session::{DetectRequest, ExecutionContext};
use rneter::templates::autodetect_with_context;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let report = autodetect_with_context(
    DetectRequest::new(
        "admin".to_string(),
        "192.168.1.1".to_string(),
        22,
        "password".to_string(),
    ),
    ExecutionContext::default(),
)
.await?;

if let Some(best) = &report.best_match {
    println!("最佳模板: {} ({:?}, score={})", best.template_name, best.confidence, best.score);
}

for candidate in &report.candidates {
    println!("候选模板: {} score={}", candidate.template_name, candidate.score);
}
# Ok(())
# }
```

如果最佳候选满足最小置信度阈值，也可以直接继续建立正式连接：

```rust
use rneter::session::{ExecutionContext, DetectRequest};
use rneter::templates::{
    autodetect_and_connect_with_context, DetectConnectPolicy,
};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let connected = autodetect_and_connect_with_context(
    DetectRequest::new(
        "admin".to_string(),
        "192.168.1.1".to_string(),
        22,
        "password".to_string(),
    ),
    None,
    ExecutionContext::default(),
    DetectConnectPolicy::default(), // 默认最小置信度 = Medium
)
.await?;

println!("连接使用模板: {}", connected.template_name);
# Ok(())
# }
```

如果你希望调用方自己定义 autodetect 目标，也可以直接传入自己的
`handler_config + detect_profile`：

```rust
use rneter::device::prompt_rule;
use rneter::device::DeviceHandlerConfig;
use rneter::session::{DetectRequest, ExecutionContext};
use rneter::templates::{
    autodetect_with_templates_and_context, DetectTemplateDefinition,
    TemplateDetectProfile, TemplateProbe, TemplateProbeRule,
};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let custom = DetectTemplateDefinition::new(
    "custom_linux",
    DeviceHandlerConfig {
        prompt: vec![prompt_rule("Root", &[r"^custom#\\s*$"])],
        ..DeviceHandlerConfig::default()
    },
    TemplateDetectProfile {
        initial_rules: vec![TemplateProbeRule {
            pattern: r"^custom#\\s*$".to_string(),
            weight: 20,
        }],
        probes: vec![TemplateProbe {
            command: "show version".to_string(),
            rules: vec![TemplateProbeRule {
                pattern: r"Custom Linux".to_string(),
                weight: 90,
            }],
            error_patterns: Vec::new(),
        }],
    },
);

let report = autodetect_with_templates_and_context(
    DetectRequest::new(
        "admin".to_string(),
        "192.168.1.1".to_string(),
        22,
        "password".to_string(),
    ),
    ExecutionContext::default(),
    vec![custom],
)
.await?;
# Ok(())
# }
```

如果你希望“内置 autodetect 能力 + 自定义模板”一起跑，可以直接用新的合并入口：

```rust
use rneter::device::prompt_rule;
use rneter::device::DeviceHandlerConfig;
use rneter::session::{DetectRequest, ExecutionContext};
use rneter::templates::{
    autodetect_with_builtin_and_templates_and_context, DetectTemplateDefinition,
    TemplateDetectProfile, TemplateProbe, TemplateProbeRule,
};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let custom = DetectTemplateDefinition::new(
    "custom_linux",
    DeviceHandlerConfig {
        prompt: vec![prompt_rule("Root", &[r"^custom#\\s*$"])],
        ..DeviceHandlerConfig::default()
    },
    TemplateDetectProfile {
        initial_rules: vec![TemplateProbeRule {
            pattern: r"^custom#\\s*$".to_string(),
            weight: 20,
        }],
        probes: vec![TemplateProbe {
            command: "show version".to_string(),
            rules: vec![TemplateProbeRule {
                pattern: r"Custom Linux".to_string(),
                weight: 90,
            }],
            error_patterns: Vec::new(),
        }],
    },
);

let report = autodetect_with_builtin_and_templates_and_context(
    DetectRequest::new(
        "admin".to_string(),
        "192.168.1.1".to_string(),
        22,
        "password".to_string(),
    ),
    ExecutionContext::default(),
    vec![custom],
)
.await?;
# Ok(())
# }
```

## 与 Netmiko 和 Scrapli 的对比

如果你之前主要使用 [Netmiko](https://github.com/ktbyers/netmiko) 或
[Scrapli](https://github.com/carlmontanari/scrapli)，最需要先建立的认知是：
`rneter` 的抽象边界和它们不完全一样。

- `Netmiko` 更像一个围绕 prompt 驱动命令执行的设备会话工具库。
- `Scrapli` 更像一个围绕 transport/channel/driver 和 privilege level 的设备连接工具库。
- `rneter` 更像一个围绕显式状态、状态边和可复用操作构建的 Prompt 状态机执行引擎。

从底层机制上说：

- 在 `Netmiko` 里，prompt 主要用于判断一条命令什么时候执行结束。
- 在 `Scrapli` 里，prompt 和 privilege level 主要用于维持 channel 与预期模式对齐。
- 在 `rneter` 里，prompt 不仅用于判断命令结束，还会驱动正式状态机更新当前状态。

### 机制对照

| 维度                  | `rneter`                                                                 | `Netmiko`                                                                | `Scrapli`                                                  | 说明                                                  |
| --------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ---------------------------------------------------------- | ----------------------------------------------------- |
| 核心抽象              | `DeviceHandler` 是正式有限状态机，包含 prompt 规则、输入规则和状态迁移边 | `BaseConnection` 是 prompt 驱动的设备会话对象                            | `Driver + Channel + Transport`，并配合平台 privilege level | `rneter` 对设备行为建模更显式，另外两者先强调会话交互 |
| Prompt 的角色         | Prompt 既是状态事件，也是命令结束信号                                    | Prompt 主要是命令结束信号                                                | Prompt 主要用于 channel 对齐和结束判定                     | `rneter` 把 prompt 当作控制面数据，而不仅是输出分隔符 |
| 模式切换              | 基于显式 `edges` 做 BFS 自动寻路                                         | 常见是 `enable()`、`config_mode()`、`exit_config_mode()` 这类专用 helper | 常见是切换到目标 privilege level                           | `rneter` 更容易泛化复杂模式图                         |
| 交互输入              | 输入提示也是状态机规则的一部分，还能按 command flow 扩展                 | 常通过 `send_command_timing()`、`send_multiline()` 等方式处理            | 常通过交互式 channel 操作和显式 prompt 期望处理            | `rneter` 更适合复用设备向导式交互                     |
| 多行 / 脏 Prompt 处理 | 统一做流式清洗、prompt prefix 缓冲、片段合并再匹配                       | 常见是 ANSI/backspace 清洗后直接读 prompt                                | 常见是 channel prompt pattern 搜索和显式读取               | `rneter` 在复杂 prompt 场景下投入了更多底层机制       |
| 错误处理              | 错误行可映射为状态机 `error` 状态，也可通过 `ignore_errors` 忽略         | 主要是方法级或输出模式级判断                                             | 主要是 response 失败条件或上层逻辑判断                     | `rneter` 更容易把错误语义收敛到统一执行流程中         |
| 输出模型              | `Output.success`、`content`、`all`、`prompt`、可选 `exit_code`、录制事件 | 以处理后的字符串输出为主，外加辅助解析手段                               | 以 response 对象为主，包含原始/处理后输出和 channel 元信息 | `rneter` 更偏编排和回放，而不仅是交互式使用           |
| Linux 支持            | Linux 复用同一套状态执行引擎，并支持 shell exit-status 捕获              | 不是主要设计中心                                                         | 支持，但仍偏 channel/prompt 视角                           | `rneter` 更容易统一网络设备和 Linux 主机的执行语义    |
| 事务 / 回滚           | 内置 `TxBlock`、`TxWorkflow`、回滚策略和子步骤结果                       | 需要调用方自行组织                                                       | 需要调用方自行组织                                         | 这是 `rneter` 与另外两者最明显的架构差异之一          |
| 回放 / 固件测试       | 内置 session recording / replay                                          | 不是核心架构能力                                                         | 不是核心架构能力                                           | `rneter` 更适合作为 CLI 自动化平台底层内核            |

### 同一任务下的不同心智模型

| 任务                         | `Netmiko` 的常见思路                                | `Scrapli` 的常见思路                            | `rneter` 的常见思路                                            |
| ---------------------------- | --------------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------- |
| 执行 `show version`          | 发命令并一直读到 prompt                             | 通过 channel 发命令并一直读到 prompt pattern    | 先收敛到目标 mode，再执行命令，并用返回 prompt 更新 FSM        |
| 下发配置命令                 | 进入 config mode，发命令，必要时退出                | 切换到 config privilege，发送配置，再视情况切回 | 把 config 视为一个状态节点，并通过状态边自动路由过去           |
| 处理 `copy scp:` 交互        | 用 timing / multiline helper 加预期 prompt 逐步处理 | 用交互式 send/read 操作配合显式 prompt 期望处理 | 构建带命令级 `CommandInteraction` 规则的 `CommandFlow`         |
| 处理 `[edit]` + `user@host#` | 调整平台 prompt 逻辑                                | 调整 prompt pattern / channel 行为              | 将 `[edit]` 建模为 prompt prefix，并在匹配前与后续 prompt 合并 |

### 为什么这很重要

对 `Netmiko` 用户来说，`rneter` 更不像“另一个更强的 `send_command`”，而更像
“一个知道设备当前状态、并能围绕状态执行自动化编排的执行引擎”。

对 `Scrapli` 用户来说，`rneter` 更不像“另一个 driver/channel 栈”，而更像
“在 prompt 解析之上再往上一层，正式构建状态图和执行模型的系统”。

这也是为什么 `rneter` 在下面这些场景里会特别有优势：

- 多步骤命令工作流
- 厂商特定的交互式向导
- 事务化下发与回滚
- 基于 prompt 的可回放测试
- 同时覆盖网络设备和 Linux 主机的统一编排层

对应的代价是：相比 `Netmiko` 和 `Scrapli`，`rneter` 会更频繁地要求调用方从
“状态、迁移和执行模型”的角度思考问题。

## 支持的设备类型

该库旨在与任何支持 SSH 的网络设备和 Linux 主机配合使用。特别适合：

**网络设备：**

| 模板名        | 厂商 / 平台               | 主要模式                                | 备注                                                |
| ------------- | ------------------------- | --------------------------------------- | --------------------------------------------------- |
| `cisco`       | Cisco IOS / IOS-XE        | `Login`、`Enable`、`Config`             | 也作为 `cisco_asa` 当前已验证的 handler 行为        |
| `cisco_asa`   | Cisco ASA                 | `Login`、`Enable`、`Config`             | 独立模板名和自动识别目标；复用 `cisco` handler 行为 |
| `cisco_nxos`  | Cisco NX-OS               | `Login`、`Enable`、`Config`             | Cisco-like 模式切换，包含 NX-OS 分页默认设置        |
| `juniper`     | Juniper JunOS             | `Enable`、`Config`                      | 支持 JunOS edit prompt prefix 处理                  |
| `arista`      | Arista EOS                | `Login`、`Enable`、`Config`             | 面向 EOS 的 Cisco-like 模板                         |
| `aruba_aoscx` | Aruba AOS-CX              | `Login`、`Enable`、`Config`             | 使用 AOS-CX 分页默认设置                            |
| `dell_os10`   | Dell OS10                 | `Login`、`Enable`、`Config`             | 面向 Dell OS10 的 Cisco-like 模板                   |
| `ruijie`      | 锐捷 Ruijie RGOS          | `Login`、`Enable`、`Config`             | 包含拒绝修改密码提示的交互规则                      |
| `zte_zxros`   | 中兴 ZTE ZXROS            | `Login`、`Enable`、`Config`             | 面向 ZTE ZXROS 的 Cisco-like 模板                   |
| `huawei`      | 华为 Huawei VRP           | `Enable`、`Config`                      | 使用 `system-view` / `return` 模式切换              |
| `h3c`         | H3C Comware               | `Enable`、`Config`                      | Comware 风格尖括号/方括号 prompt                    |
| `hillstone`   | Hillstone SG / StoneOS    | `Enable`、`Config`                      | 包含保存确认提示                                    |
| `array`       | Array Networks APV        | `Login`、`Enable`、`Config`、vsite 模式 | 支持系统/上下文模式变体                             |
| `fortinet`    | Fortinet FortiGate        | `Enable`、vdom 模式                     | 基础 FortiGate / VDOM 状态模型                      |
| `paloalto`    | Palo Alto Networks PAN-OS | `Enable`、`Config`                      | Operational 和 config prompt                        |
| `checkpoint`  | Check Point Gaia          | `Enable`                                | 只读/操作类模板                                     |
| `topsec`      | Topsec NGFW               | `Enable`                                | 基础操作类模板                                      |
| `venustech`   | 启明星辰 Venustech USG    | `Login`、`Enable`、`Config`             | Cisco-like 防火墙模板                               |
| `dptech`      | 迪普 DPTech 防火墙        | `Enable`、`Config`                      | H3C-like prompt 风格                                |
| `chaitin`     | 长亭 Chaitin SafeLine     | `Login`、`Enable`、`Config`             | Cisco-like 网关模板                                 |
| `qianxin`     | 奇安信 QiAnXin NSG        | `Enable`、`Config`                      | 安全网关模板                                        |
| `maipu`       | 迈普通信 Maipu 网络设备   | `Login`、`Enable`、`Config`             | 面向 Maipu 设备的 Cisco-like 模板                   |

**Linux 主机：**

| 模板名  | 范围              | 备注                                                          |
| ------- | ----------------- | ------------------------------------------------------------- |
| `linux` | 通用 Linux 发行版 | Ubuntu、Debian、CentOS、RHEL 以及其他基于 shell 的 Linux 主机 |
| `linux` | 提权方式          | 支持 `sudo -i`、`sudo -s`、`su` 和直接 root 会话              |
| `linux` | Prompt 处理       | 支持带自定义 pattern 的智能 prompt 检测                       |
| `linux` | 事务能力          | 支持带回滚策略的事务式配置管理                                |

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

对于 `execute_operation_with_context(...)` 这类 operation 级 API，失败时现在会返回
`SessionOperationExecutionError`，可通过 `partial_output()` 读取失败前已完成的子步骤结果。

## 文档

详细的 API 文档请访问 [docs.rs/rneter](https://docs.rs/rneter)。

## 许可证

本项目采用 MIT 许可证 - 详情请参阅 [LICENSE](LICENSE) 文件。

## 贡献

欢迎贡献！请随时提交 Pull Request。

## 作者

demohiiiii
