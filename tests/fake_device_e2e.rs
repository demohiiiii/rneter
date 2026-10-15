//! End-to-end tests against an in-process fake SSH device built from a
//! custom (non-builtin) template via `rneter::testkit`.
//!
//! These tests drive the full `rneter` stack over a live socket: connection
//! establishment, prompt detection, state transitions, connection pooling,
//! session recording with redaction, offline replay (dry-run), and
//! transaction rollback.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rneter::device::{DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use rneter::session::{
    CmdJob, Command, ConnectionMode, ConnectionPoolConfig, HookAction, RollbackPolicy,
    SessionEvent, SessionRecordLevel, SessionRecorder, SessionReplayer, SshConnectionManager,
    TxBlock, TxStep, TxWorkflow,
};
use rneter::testkit::{
    DEFAULT_ENABLE_PASSWORD, DevicePersona, ERROR_COMMAND, FakeSshDevice, FaultInjection,
};

struct TempKeyFile {
    path: PathBuf,
}

impl TempKeyFile {
    fn new(contents: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("rneter-test-key-{}-{nonce}", std::process::id()));
        std::fs::write(&path, contents).expect("write temporary private key");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn replace(&self, contents: &str) {
        std::fs::write(&self.path, contents).expect("replace temporary private key");
    }
}

impl Drop for TempKeyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn custom_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Login", &[r"^fake>\s*$"]),
            prompt_rule("Enable", &[r"^fake#\s*$"]),
            prompt_rule("Config", &[r"^fake\(config\)#\s*$"]),
        ],
        write: vec![input_rule(
            "EnablePassword",
            true,
            "EnablePassword",
            false,
            &[r"^Password:\s*$"],
        )],
        error_regex: vec![r"^ERROR: .+$".to_string()],
        edges: vec![
            transition_rule("Login", "enable", "Enable", false, false),
            transition_rule("Enable", "configure terminal", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
            transition_rule("Enable", "exit", "Login", true, false),
        ],
        ..Default::default()
    }
}

fn custom_persona() -> DevicePersona {
    DevicePersona::for_config(
        "custom-cisco-like",
        custom_config(),
        "login",
        &[
            ("login", "fake>"),
            ("enable", "fake#"),
            ("config", "fake(config)#"),
        ],
    )
    .with_challenge("enable", "Password: ", DEFAULT_ENABLE_PASSWORD)
    .with_enable_password(DEFAULT_ENABLE_PASSWORD)
}

fn command(mode: &str, text: &str) -> Command {
    Command {
        mode: mode.to_string(),
        command: text.to_string(),
        timeout: Some(10),
        ..Command::default()
    }
}

async fn wait_for_command(device: &FakeSshDevice, expected: &str, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if device
            .received_commands()
            .iter()
            .any(|command| command == expected)
        {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_connection_closed(recorder: &SessionRecorder, timeout: Duration) -> bool {
    tokio::time::timeout(timeout, async {
        loop {
            if recorder
                .entries()
                .expect("session events")
                .iter()
                .any(|entry| matches!(entry.event, SessionEvent::ConnectionClosed { .. }))
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn slow_connection_is_still_evicted_after_idle_timeout() {
    let device = FakeSshDevice::spawn(
        custom_persona()
            .with_faults(FaultInjection::new().with_auth_delay(Duration::from_millis(1_200))),
    )
    .await
    .expect("spawn slow-auth fake device");
    let manager = SshConnectionManager::with_pool_config(ConnectionPoolConfig {
        max_connections: 2,
        idle_timeout: Duration::from_millis(400),
    });
    let recorder = SessionRecorder::new(SessionRecordLevel::KeyEventsOnly);

    manager
        .execute_command_with_recorder_and_context(
            device.connection_request().expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
            recorder.clone(),
        )
        .await
        .expect("command after slow connection");

    assert!(
        wait_for_connection_closed(&recorder, Duration::from_secs(2)).await,
        "idle connection was not evicted after a slow connection attempt: {:?}",
        device.received_commands()
    );
}

#[tokio::test]
async fn dropping_last_manager_closes_its_connections() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("spawn fake device");
    let manager = SshConnectionManager::with_pool_config(ConnectionPoolConfig {
        max_connections: 2,
        idle_timeout: Duration::from_secs(30),
    });
    let recorder = SessionRecorder::new(SessionRecordLevel::KeyEventsOnly);

    manager
        .execute_command_with_recorder_and_context(
            device.connection_request().expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
            recorder.clone(),
        )
        .await
        .expect("command before manager drop");
    drop(manager);

    assert!(
        wait_for_connection_closed(&recorder, Duration::from_secs(1)).await,
        "dropping the last manager left its connection open: {:?}",
        device.received_commands()
    );
}

#[tokio::test]
async fn keepalive_connection_survives_pool_idle_expiration() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("device");
    let manager = SshConnectionManager::with_pool_config(ConnectionPoolConfig {
        max_connections: 2,
        idle_timeout: Duration::from_millis(100),
    });
    let request = device.connection_request().expect("request");
    let context = device.execution_context().with_keep_alive(true);

    manager
        .execute_command_with_context(
            request.clone(),
            command("Enable", "show version"),
            context.clone(),
        )
        .await
        .expect("initial command");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(!device.received_commands().iter().any(|cmd| cmd == "exit"));
    manager
        .execute_command_with_context(request.clone(), command("Enable", "show clock"), context)
        .await
        .expect("reuse after idle expiration");
    assert_eq!(
        device
            .received_commands()
            .iter()
            .filter(|cmd| cmd.as_str() == "enable")
            .count(),
        1
    );
    assert_eq!(manager.close_connection(&request).await.expect("close"), 1);
}

#[tokio::test]
async fn changing_keepalive_replaces_the_cached_connection() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("device");
    let manager = SshConnectionManager::new();
    let request = device.connection_request().expect("request");
    for enabled in [false, true, false] {
        manager
            .execute_command_with_context(
                request.clone(),
                command("Enable", "show version"),
                device.execution_context().with_keep_alive(enabled),
            )
            .await
            .expect("command with changed keepalive setting");
    }
    assert_eq!(
        device
            .received_commands()
            .iter()
            .filter(|cmd| cmd.as_str() == "enable")
            .count(),
        3
    );
    manager.close_all().await.expect("close");
}

#[tokio::test]
async fn keepalive_interval_changes_replace_only_enabled_connections() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("device");
    let manager = SshConnectionManager::new();
    let request = device.connection_request().expect("request");
    let context = device
        .execution_context()
        .with_keep_alive(true)
        .with_keep_alive_interval(Duration::from_millis(100));
    let original = manager
        .get_with_context(request.clone(), context.clone())
        .await
        .expect("original");
    let reused = manager
        .get_with_context(request.clone(), context.clone())
        .await
        .expect("reused");
    assert!(original.same_channel(&reused));
    let replacement = manager
        .get_with_context(request.clone(), context.with_keep_alive_interval_secs(2))
        .await
        .expect("replacement");
    assert!(!original.same_channel(&replacement));
    drop((original, reused));

    let disabled = device.execution_context().with_keep_alive_interval_secs(1);
    let first = manager
        .get_with_context(request.clone(), disabled.clone())
        .await
        .expect("disabled");
    let second = manager
        .get_with_context(request, disabled.with_keep_alive_interval(Duration::ZERO))
        .await
        .expect("unused interval");
    assert!(
        first.same_channel(&second),
        "disabled keepalives ignore the interval"
    );
    drop(replacement);
    manager.close_all().await.expect("close");
}

#[tokio::test]
async fn zero_keepalive_interval_is_rejected_for_pooled_and_one_shot_connections() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("device");
    let manager = SshConnectionManager::new();
    for mode in [ConnectionMode::Pooled, ConnectionMode::OneShot] {
        let error = manager
            .get_with_context(
                device.connection_request().expect("request"),
                device
                    .execution_context()
                    .with_keep_alive(true)
                    .with_keep_alive_interval_secs(0)
                    .with_connection_mode(mode),
            )
            .await
            .expect_err("zero interval must be rejected");
        assert!(matches!(
            error,
            rneter::error::ConnectError::InvalidKeepAliveConfig(_)
        ));
        assert!(!error.is_transient());
    }
    assert_eq!(manager.close_all().await.expect("no connections opened"), 0);
}

#[tokio::test]
async fn configured_keepalive_interval_sends_ssh_traffic_without_cli_input() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("device");
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy listener");
    let proxy_addr = listener.local_addr().expect("proxy address");
    let device_addr = device.addr();
    let received_bytes = Arc::new(AtomicUsize::new(0));
    let proxy_bytes = received_bytes.clone();
    // Observe encrypted client traffic without changing the SSH server.
    let proxy = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let server = TcpStream::connect(device_addr).await?;
        let (mut client_read, mut client_write) = client.into_split();
        let (mut server_read, mut server_write) = server.into_split();
        let upload = async move {
            let mut buffer = [0u8; 4096];
            loop {
                let count = client_read.read(&mut buffer).await?;
                if count == 0 {
                    return Ok::<(), std::io::Error>(());
                }
                proxy_bytes.fetch_add(count, Ordering::SeqCst);
                server_write.write_all(&buffer[..count]).await?;
            }
        };
        tokio::select! {
            result = upload => result,
            result = tokio::io::copy(&mut server_read, &mut client_write) => result.map(|_| ()),
        }
    });
    let mut request = device.connection_request().expect("request");
    request.addr = proxy_addr.ip().to_string();
    request.port = proxy_addr.port();
    let manager = SshConnectionManager::new();
    let _sender = manager
        .get_with_context(
            request,
            device
                .execution_context()
                .with_keep_alive(true)
                .with_keep_alive_interval(Duration::from_millis(100)),
        )
        .await
        .expect("proxied connection");
    tokio::time::sleep(Duration::from_millis(250)).await;
    let baseline = received_bytes.load(Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(1), async {
        while received_bytes.load(Ordering::SeqCst) == baseline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("configured keepalive must produce SSH traffic within one second");
    assert!(
        device.received_commands().is_empty(),
        "keepalives must not write to the CLI"
    );
    manager.close_all().await.expect("close");
    tokio::time::timeout(Duration::from_secs(1), proxy)
        .await
        .expect("proxy stops after disconnect")
        .expect("proxy task")
        .expect("proxy I/O");
    assert!(
        device.received_commands().is_empty(),
        "closing SSH must not send CLI input"
    );
}

#[tokio::test]
async fn explicit_close_stops_held_senders_and_runs_cleanup_for_all_recording_variants() {
    let mut persona = custom_persona();
    persona.config.hooks.before_disconnect.push(HookAction::new(
        "cleanup",
        command("Enable", "show cleanup").into(),
    ));
    let device = FakeSshDevice::spawn(persona).await.expect("device");
    let manager = SshConnectionManager::new();
    let request = device.connection_request().expect("request");
    let context = device.execution_context().with_keep_alive(true);
    let plain = manager
        .get_with_context(request.clone(), context.clone())
        .await
        .expect("plain sender");
    let (recorded, recorder) = manager
        .get_with_recording_and_context(request.clone(), context.clone())
        .await
        .expect("recorded sender");
    let caller_recorder = SessionRecorder::new(SessionRecordLevel::Full);
    let (custom, _) = manager
        .get_with_recorder_and_context(request.clone(), context, caller_recorder.clone())
        .await
        .expect("custom recorder sender");

    assert_eq!(
        manager
            .close_connection(&request)
            .await
            .expect("close variants"),
        3
    );
    assert!(plain.is_closed());
    assert!(recorded.is_closed());
    assert!(custom.is_closed());
    assert!(!device.received_commands().iter().any(|cmd| cmd == "exit"));
    assert_eq!(
        device
            .received_commands()
            .iter()
            .filter(|cmd| cmd.as_str() == "show cleanup")
            .count(),
        3
    );
    for recorder in [recorder, caller_recorder] {
        assert_eq!(
            recorder
                .entries()
                .expect("events")
                .iter()
                .filter(|entry| matches!(entry.event, SessionEvent::ConnectionClosed { .. }))
                .count(),
            1
        );
    }
    assert_eq!(
        manager
            .close_connection(&request)
            .await
            .expect("close twice"),
        0
    );
    assert_eq!(manager.close_all().await.expect("empty pool"), 0);
    manager
        .execute_command_with_context(
            request,
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("reconnect after explicit close");
    manager.close_all().await.expect("close replacement");
}

#[tokio::test]
async fn explicit_close_is_scoped_to_the_device_and_shared_between_manager_clones() {
    let first = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("first device");
    let second = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("second device");
    let manager = SshConnectionManager::new();
    let first_sender = manager
        .get_with_context(
            first.connection_request().expect("request"),
            first.execution_context().with_keep_alive(true),
        )
        .await
        .expect("first sender");
    let second_sender = manager
        .get_with_context(
            second.connection_request().expect("request"),
            second.execution_context().with_keep_alive(true),
        )
        .await
        .expect("second sender");

    assert_eq!(
        manager
            .clone()
            .close_connection(&first.connection_request().expect("request"))
            .await
            .expect("close first"),
        1
    );
    assert!(first_sender.is_closed());
    assert!(first.received_commands().is_empty());
    assert!(!second_sender.is_closed());
    assert_eq!(
        manager.clone().close_all().await.expect("close remaining"),
        1
    );
    assert!(second_sender.is_closed());
    assert!(second.received_commands().is_empty());
    assert_eq!(manager.close_all().await.expect("close empty"), 0);
}

async fn assert_cancelled_close_finishes(close_all: bool) {
    let mut persona = custom_persona()
        .with_faults(FaultInjection::new().with_command_delay(Duration::from_millis(400)));
    persona.config.hooks.before_disconnect.push(HookAction::new(
        "cleanup",
        command("Login", "show cleanup").into(),
    ));
    let first = FakeSshDevice::spawn(persona.clone())
        .await
        .expect("first device");
    let second = FakeSshDevice::spawn(persona).await.expect("second device");
    let manager = SshConnectionManager::new();
    let request = first.connection_request().expect("request");
    let mut connections = Vec::new();
    for device in [&first, &second] {
        let (sender, recorder) = manager
            .get_with_recording_and_context(
                device.connection_request().expect("request"),
                device.execution_context().with_keep_alive(true),
            )
            .await
            .expect("recorded connection");
        let (response, result) = tokio::sync::oneshot::channel();
        sender
            .send(CmdJob {
                data: command("Login", "show version"),
                sys: None,
                responder: response,
            })
            .await
            .expect("start command");
        connections.push((device, sender, recorder, result));
    }
    for device in [&first, &second] {
        assert!(wait_for_command(device, "show version", Duration::from_secs(2)).await);
    }

    let close = async {
        if close_all {
            manager.close_all().await
        } else {
            manager.close_connection(&request).await
        }
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(50), close)
            .await
            .is_err(),
        "close should still be waiting for the active command"
    );

    for (index, (device, sender, recorder, result)) in connections.into_iter().enumerate() {
        assert!(
            result
                .await
                .expect("command response")
                .expect("active command completes")
                .success
        );
        if close_all || index == 0 {
            tokio::time::timeout(Duration::from_secs(3), sender.closed())
                .await
                .expect("shutdown must complete even after its caller is cancelled");
            assert_eq!(
                device
                    .received_commands()
                    .iter()
                    .filter(|cmd| cmd.as_str() == "show cleanup")
                    .count(),
                1
            );
            assert_eq!(
                recorder
                    .entries()
                    .expect("events")
                    .iter()
                    .filter(|entry| matches!(entry.event, SessionEvent::ConnectionClosed { .. }))
                    .count(),
                1
            );
        } else {
            assert!(
                !sender.is_closed(),
                "closing one device must preserve the other"
            );
        }
    }
    assert_eq!(
        manager
            .close_connection(&request)
            .await
            .expect("close again"),
        0
    );
    assert_eq!(
        manager.close_all().await.expect("close remaining"),
        if close_all { 0 } else { 1 }
    );
}

#[tokio::test]
async fn cancelled_close_connection_finishes_shutdown() {
    assert_cancelled_close_finishes(false).await;
}

#[tokio::test]
async fn cancelled_close_all_finishes_every_selected_connection() {
    assert_cancelled_close_finishes(true).await;
}

#[tokio::test]
async fn keepalive_does_not_extend_one_shot_lifetime() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("device");
    let manager = SshConnectionManager::new();
    let recorder = SessionRecorder::new(SessionRecordLevel::KeyEventsOnly);
    manager
        .execute_command_with_recorder_and_context(
            device.connection_request().expect("request"),
            command("Enable", "show version"),
            device
                .execution_context()
                .with_keep_alive(true)
                .with_connection_mode(ConnectionMode::OneShot),
            recorder.clone(),
        )
        .await
        .expect("one-shot command");
    assert!(wait_for_connection_closed(&recorder, Duration::from_secs(1)).await);
    assert_eq!(manager.close_all().await.expect("empty pool"), 0);
}

#[tokio::test]
#[ignore = "waits 65 seconds for real SSH keepalive and inactivity timers"]
async fn keepalive_preserves_idle_ssh_transport() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("device");
    let manager = SshConnectionManager::new();
    let request = device.connection_request().expect("request");
    let active = manager
        .get_with_context(
            request.clone(),
            device.execution_context().with_keep_alive(true),
        )
        .await
        .expect("keepalive sender");
    let (idle, _) = manager
        .get_with_recording_and_context(request, device.execution_context())
        .await
        .expect("default sender");

    tokio::time::sleep(Duration::from_secs(65)).await;
    let (response, result) = tokio::sync::oneshot::channel();
    active
        .send(CmdJob {
            data: command("Enable", "show version"),
            sys: None,
            responder: response,
        })
        .await
        .expect("send after idle period");
    assert!(
        result
            .await
            .expect("response")
            .expect("keepalive transport still connected")
            .success
    );
    let (response, result) = tokio::sync::oneshot::channel();
    idle.send(CmdJob {
        data: command("Enable", "show version"),
        sys: None,
        responder: response,
    })
    .await
    .expect("send on expired transport");
    assert!(matches!(
        result.await.expect("response"),
        Err(rneter::error::ConnectError::ConnectClosedError)
    ));
    manager.close_all().await.expect("close transports");
}

#[tokio::test]
async fn executes_commands_and_reuses_pooled_connection() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("spawn fake device");
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("first command should succeed");
    assert!(output.success);
    assert!(
        output
            .content
            .contains(&device.persona().benign_reply.clone())
    );
    assert!(
        output.all.contains("show version"),
        "full output keeps the echo"
    );

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("second command should succeed");
    assert!(output.success);

    let commands = device.received_commands();
    assert_eq!(
        commands.iter().filter(|c| c.as_str() == "enable").count(),
        1,
        "the pooled connection must only authenticate enable mode once; got {commands:?}"
    );
    assert!(commands.contains(&DEFAULT_ENABLE_PASSWORD.to_string()));
    assert_eq!(
        commands
            .iter()
            .filter(|c| c.as_str() == "show version")
            .count(),
        2
    );
}

#[tokio::test]
async fn command_marked_failed_when_device_reports_error() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("spawn fake device");
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("Enable", ERROR_COMMAND),
            device.execution_context(),
        )
        .await
        .expect("error output is a successful execution with success=false");
    assert!(!output.success);
    assert!(output.content.contains("ERROR: forced failure"));
}

#[tokio::test]
async fn records_redacts_and_replays_session_as_dry_run() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("spawn fake device");
    let manager = SshConnectionManager::new();

    let recorder =
        SessionRecorder::new(SessionRecordLevel::KeyEventsOnly).with_redactor(
            |event| match event {
                SessionEvent::CommandOutput {
                    command,
                    mode,
                    prompt_before,
                    prompt_after,
                    fsm_prompt_before,
                    fsm_prompt_after,
                    success,
                    exit_code,
                    content,
                    all,
                } => SessionEvent::CommandOutput {
                    command: command.replace("s3cret", "***"),
                    mode,
                    prompt_before,
                    prompt_after,
                    fsm_prompt_before,
                    fsm_prompt_after,
                    success,
                    exit_code,
                    content: content.replace("s3cret", "***"),
                    all: all.replace("s3cret", "***"),
                },
                other => other,
            },
        );
    let output = manager
        .execute_command_with_recorder_and_context(
            device.connection_request().expect("request"),
            command("Config", "hostname lab-s3cret"),
            device.execution_context(),
            recorder.clone(),
        )
        .await
        .expect("hostname command should succeed");
    assert!(output.success);

    let live_output = manager
        .execute_command_with_recorder_and_context(
            device.connection_request().expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
            recorder.clone(),
        )
        .await
        .expect("recorded show command should succeed");
    assert!(live_output.success);

    let isolated_output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("Enable", "show isolated"),
            device.execution_context(),
        )
        .await
        .expect("ordinary pooled command should succeed");
    assert!(isolated_output.success);

    // The device received the real secret, but the recording never stores it.
    assert!(
        device
            .received_commands()
            .contains(&"hostname lab-s3cret".to_string()),
        "device must receive the unredacted command"
    );
    let jsonl = recorder.to_jsonl().expect("encode recording");
    assert!(
        !jsonl.contains("s3cret"),
        "recording leaked the secret: {jsonl}"
    );
    assert!(jsonl.contains("hostname lab-***"));
    assert!(
        !jsonl.contains("show isolated"),
        "ordinary pooled command leaked into a caller's recorder: {jsonl}"
    );

    // Dry-run: replay the recorded session offline, without any device.
    let mut replayer = SessionReplayer::from_recorder(&recorder);
    let context = replayer
        .initial_context()
        .expect("recording contains connection context");
    assert_eq!(
        context.device_addr,
        format!("admin@127.0.0.1:{}", device.port())
    );

    let replayed = replayer
        .replay_next("hostname lab-***")
        .expect("replay hostname command");
    assert!(replayed.success);

    let replayed = replayer
        .replay_next("show version")
        .expect("replay show version");
    assert!(replayed.success);
    assert_eq!(replayed.content, live_output.content);
}

#[tokio::test]
async fn rolls_back_transaction_when_forward_step_fails() {
    let device = FakeSshDevice::spawn(custom_persona())
        .await
        .expect("spawn fake device");
    let manager = SshConnectionManager::new();

    let block = TxBlock {
        name: "hostname-change".to_string(),
        rollback_policy: RollbackPolicy::PerStep,
        fail_fast: true,
        steps: vec![
            TxStep {
                run: command("Config", "hostname tx-step").into(),
                rollback: Some(command("Config", "hostname fake").into()),
                rollback_on_failure: false,
            },
            TxStep {
                run: command("Enable", ERROR_COMMAND).into(),
                rollback: None,
                rollback_on_failure: false,
            },
        ],
    };

    let result = manager
        .execute_tx_workflow_with_context(
            device.connection_request().expect("request"),
            TxWorkflow {
                name: "hostname-change-workflow".to_string(),
                blocks: vec![block],
                fail_fast: true,
            },
            device.execution_context(),
        )
        .await
        .expect("transaction should complete with a rollback");

    assert!(!result.committed);
    let block_result = result
        .block_results
        .first()
        .expect("failed workflow must include block result");
    assert_eq!(block_result.failed_step, Some(1));
    assert!(result.rollback_attempted);
    assert!(
        block_result.rollback_succeeded,
        "rollback errors: {:?}",
        block_result.rollback_errors
    );

    let commands = device.received_commands();
    let forward_pos = commands
        .iter()
        .position(|c| c == "hostname tx-step")
        .expect("forward step must reach the device");
    let failing_pos = commands
        .iter()
        .position(|c| c == ERROR_COMMAND)
        .expect("failing step must reach the device");
    let rollback_pos = commands
        .iter()
        .position(|c| c == "hostname fake")
        .expect("rollback command must reach the device");
    assert!(forward_pos < failing_pos, "device saw: {commands:?}");
    assert!(
        failing_pos < rollback_pos,
        "rollback must run after the failing step; device saw: {commands:?}"
    );
}

#[tokio::test]
async fn authenticates_with_private_key() {
    use rneter::session::SshAuthMethod;
    use russh::keys::{Algorithm, PrivateKey};

    let private_key =
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("generate client key");
    let public_key = private_key
        .public_key()
        .to_openssh()
        .expect("encode public key");
    let key_data = private_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("encode private key")
        .to_string();

    let persona = custom_persona().with_authorized_public_key(public_key);
    let device = FakeSshDevice::spawn(persona)
        .await
        .expect("spawn key-auth device");
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device
                .connection_request_with_auth(SshAuthMethod::private_key(key_data, None))
                .expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("private-key login should succeed");
    assert!(output.success);
    assert!(output.content.contains("testkit-ok sample output"));
}

#[tokio::test]
async fn rejects_an_incorrect_private_key_passphrase() {
    use rneter::session::SshAuthMethod;
    use russh::keys::{Algorithm, PrivateKey};

    let private_key =
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("generate client key");
    let public_key = private_key
        .public_key()
        .to_openssh()
        .expect("encode public key");
    let encrypted_key = private_key
        .encrypt(&mut rand::rng(), "correct-passphrase")
        .expect("encrypt private key")
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("encode private key")
        .to_string();

    let device = FakeSshDevice::spawn(custom_persona().with_authorized_public_key(public_key))
        .await
        .expect("spawn key-auth device");
    let error = SshConnectionManager::new()
        .execute_command_with_context(
            device
                .connection_request_with_auth(SshAuthMethod::private_key(
                    encrypted_key,
                    Some("wrong-passphrase".to_string()),
                ))
                .expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect_err("wrong passphrase must fail");

    assert!(
        matches!(error, rneter::error::ConnectError::InvalidSshAuth(_)),
        "unexpected error: {error:?}"
    );
    assert!(device.received_commands().is_empty());
}

#[tokio::test]
async fn private_key_file_rotation_recreates_pooled_connection() {
    use rneter::session::SshAuthMethod;
    use russh::keys::{Algorithm, PrivateKey};

    let first_key =
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("first client key");
    let second_key =
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("second client key");
    let first_public_key = first_key
        .public_key()
        .to_openssh()
        .expect("first public key");
    let second_public_key = second_key
        .public_key()
        .to_openssh()
        .expect("second public key");
    let first_key_data = first_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("first private key")
        .to_string();
    let second_key_data = second_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("second private key")
        .to_string();

    let key_file = TempKeyFile::new(&first_key_data);
    let persona = custom_persona()
        .with_authorized_public_key(first_public_key)
        .with_authorized_public_key(second_public_key);
    let device = FakeSshDevice::spawn(persona)
        .await
        .expect("spawn key-auth device");
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device
                .connection_request_with_auth(SshAuthMethod::private_key_file(
                    key_file.path(),
                    None,
                ))
                .expect("first request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("first key-file login");
    assert!(output.success);

    key_file.replace(&second_key_data);
    let output = manager
        .execute_command_with_context(
            device
                .connection_request_with_auth(SshAuthMethod::private_key_file(
                    key_file.path(),
                    None,
                ))
                .expect("second request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("second key-file login");
    assert!(output.success);

    let enables = device
        .received_commands()
        .iter()
        .filter(|command| command.as_str() == "enable")
        .count();
    assert_eq!(enables, 2, "rotated key file should force reconnection");
}

#[tokio::test]
async fn authenticates_with_keyboard_interactive() {
    use rneter::session::SshAuthMethod;

    let persona = custom_persona().with_keyboard_interactive("One-Time Password: ", "otp-token");
    let device = FakeSshDevice::spawn(persona)
        .await
        .expect("spawn ki-auth device");
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device
                .connection_request_with_auth(SshAuthMethod::keyboard_interactive(vec![(
                    "One-Time Password".to_string(),
                    "otp-token".to_string(),
                )]))
                .expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("keyboard-interactive login should succeed");
    assert!(output.success);
}

#[tokio::test]
async fn auth_method_change_recreates_pooled_connection() {
    use rneter::session::SshAuthMethod;

    let persona = custom_persona().with_keyboard_interactive("OTP: ", "token-a");
    let device = FakeSshDevice::spawn(persona)
        .await
        .expect("spawn multi-auth device");
    let manager = SshConnectionManager::new();

    // First connection via password.
    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("password request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("password login");
    assert!(output.success);

    // Different auth method must not reuse the pooled password connection.
    let output = manager
        .execute_command_with_context(
            device
                .connection_request_with_auth(SshAuthMethod::keyboard_interactive(vec![(
                    "OTP".to_string(),
                    "token-a".to_string(),
                )]))
                .expect("ki request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("keyboard-interactive login after password");
    assert!(output.success);

    // Enable should have been authenticated twice (once per connection).
    let enables = device
        .received_commands()
        .iter()
        .filter(|c| c.as_str() == "enable")
        .count();
    assert_eq!(
        enables,
        2,
        "auth change should force a fresh connection; commands={:?}",
        device.received_commands()
    );
}
