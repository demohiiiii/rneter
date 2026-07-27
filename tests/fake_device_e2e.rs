//! End-to-end tests against an in-process fake SSH device built from a
//! custom (non-builtin) template via `rneter::testkit`.
//!
//! These tests drive the full `rneter` stack over a live socket: connection
//! establishment, prompt detection, state transitions, connection pooling,
//! session recording with redaction, offline replay (dry-run), and
//! transaction rollback.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rneter::device::{DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use rneter::session::{
    Command, RollbackPolicy, SessionEvent, SessionRecordLevel, SessionRecorder, SessionReplayer,
    SshConnectionManager, TxBlock, TxStep,
};
use rneter::testkit::{DEFAULT_ENABLE_PASSWORD, DevicePersona, ERROR_COMMAND, FakeSshDevice};

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
    let (_sender, recorder) = manager
        .get_with_recorder_and_context(
            device.connection_request().expect("request"),
            device.execution_context(),
            recorder,
        )
        .await
        .expect("connect with recorder");

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("Config", "hostname lab-s3cret"),
            device.execution_context(),
        )
        .await
        .expect("hostname command should succeed");
    assert!(output.success);

    let live_output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("Enable", "show version"),
            device.execution_context(),
        )
        .await
        .expect("show version should succeed");
    assert!(live_output.success);

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
        .execute_tx_block_with_context(
            device.connection_request().expect("request"),
            block,
            device.execution_context(),
        )
        .await
        .expect("transaction should complete with a rollback");

    assert!(!result.committed);
    assert_eq!(result.failed_step, Some(1));
    assert!(result.rollback_attempted);
    assert!(
        result.rollback_succeeded,
        "rollback errors: {:?}",
        result.rollback_errors
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
    use rand_core::OsRng;
    use rneter::session::SshAuthMethod;
    use russh::keys::{Algorithm, PrivateKey};

    let private_key =
        PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("generate client key");
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
    use rand_core::OsRng;
    use rneter::session::SshAuthMethod;
    use russh::keys::{Algorithm, PrivateKey};

    let private_key =
        PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("generate client key");
    let public_key = private_key
        .public_key()
        .to_openssh()
        .expect("encode public key");
    let encrypted_key = private_key
        .encrypt(&mut OsRng, "correct-passphrase")
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
        error.to_string().contains("Unable to load key"),
        "unexpected error: {error:?}"
    );
    assert!(device.received_commands().is_empty());
}

#[tokio::test]
async fn private_key_file_rotation_recreates_pooled_connection() {
    use rand_core::OsRng;
    use rneter::session::SshAuthMethod;
    use russh::keys::{Algorithm, PrivateKey};

    let first_key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("first client key");
    let second_key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).expect("second client key");
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
