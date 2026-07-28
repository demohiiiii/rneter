//! End-to-end coverage for deterministic testkit fault injection.

use std::time::Duration;

use rneter::device::{DeviceHandlerConfig, prompt_rule};
use rneter::error::ConnectError;
use rneter::session::{Command, SshAuthMethod, SshConnectionManager};
use rneter::testkit::{DevicePersona, FakeSshDevice, FaultInjection};

fn persona(faults: FaultInjection) -> DevicePersona {
    let config = DeviceHandlerConfig {
        prompt: vec![prompt_rule("Exec", &[r"^fault-test#\s*$"])],
        error_regex: vec![r"^ERROR: .+$".to_string()],
        ..Default::default()
    };
    DevicePersona::for_config("fault-test", config, "exec", &[("exec", "fault-test#")])
        .with_faults(faults)
}

fn command(text: &str, timeout: u64) -> Command {
    Command {
        mode: "Exec".to_string(),
        command: text.to_string(),
        timeout: Some(timeout),
        ..Command::default()
    }
}

#[tokio::test]
async fn rejected_auth_budget_is_shared_across_connections() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_rejected_auth_attempts(1),
    ))
    .await
    .expect("spawn fault-injecting device");
    let manager = SshConnectionManager::new();

    manager
        .execute_command_with_context(
            device.connection_request().expect("first request"),
            command("show version", 10),
            device.execution_context(),
        )
        .await
        .expect_err("first authentication attempt should be rejected");
    assert!(device.received_commands().is_empty());

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("second request"),
            command("show version", 10),
            device.execution_context(),
        )
        .await
        .expect("second connection should consume the exhausted shared budget");
    assert!(output.success);
    assert_eq!(device.received_commands(), ["show version"]);
}

#[tokio::test]
async fn public_key_auth_consumes_one_rejection_per_connection() {
    use russh::keys::{Algorithm, PrivateKey};

    let private_key =
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("generate client key");
    let public_key = private_key
        .public_key()
        .to_openssh()
        .expect("encode public key");
    let private_key = private_key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("encode private key")
        .to_string();
    let device = FakeSshDevice::spawn(
        persona(FaultInjection::new().with_rejected_auth_attempts(1))
            .with_authorized_public_key(public_key),
    )
    .await
    .expect("spawn key-auth device");
    let manager = SshConnectionManager::new();

    for should_succeed in [false, true] {
        let result = manager
            .execute_command_with_context(
                device
                    .connection_request_with_auth(SshAuthMethod::private_key(
                        private_key.clone(),
                        None,
                    ))
                    .expect("request"),
                command("show version", 10),
                device.execution_context(),
            )
            .await;
        assert_eq!(
            result.is_ok(),
            should_succeed,
            "public-key rejection budget should be consumed once per connection"
        );
    }
}

#[tokio::test]
async fn keyboard_interactive_auth_consumes_one_rejection_per_connection() {
    let device = FakeSshDevice::spawn(
        persona(FaultInjection::new().with_rejected_auth_attempts(1))
            .with_keyboard_interactive("OTP: ", "token"),
    )
    .await
    .expect("spawn keyboard-interactive device");
    let manager = SshConnectionManager::new();

    for should_succeed in [false, true] {
        let result = manager
            .execute_command_with_context(
                device
                    .connection_request_with_auth(SshAuthMethod::keyboard_interactive(vec![(
                        "OTP".to_string(),
                        "token".to_string(),
                    )]))
                    .expect("request"),
                command("show version", 10),
                device.execution_context(),
            )
            .await;
        assert_eq!(
            result.is_ok(),
            should_succeed,
            "keyboard-interactive rejection budget should be consumed once per connection"
        );
    }
}

#[tokio::test]
async fn auth_delay_can_trigger_connect_timeout() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_auth_delay(Duration::from_millis(200)),
    ))
    .await
    .expect("spawn delayed-auth device");
    let manager = SshConnectionManager::new();
    let context = device
        .execution_context()
        .with_connect_timeout(Duration::from_millis(50));

    let error = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("show version", 10),
            context,
        )
        .await
        .expect_err("authentication delay should exceed connect timeout");
    assert!(
        error.to_string().contains("SSH connection timeout"),
        "unexpected delayed-auth error: {error:?}"
    );
    assert!(device.received_commands().is_empty());
}

#[tokio::test]
async fn command_disconnect_budget_is_shared_and_only_matches_exact_command() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_disconnect_command("show version", 1),
    ))
    .await
    .expect("spawn disconnecting device");
    let manager = SshConnectionManager::new();

    let unaffected = manager
        .execute_command_with_context(
            device.connection_request().expect("unaffected request"),
            command("show clock", 10),
            device.execution_context(),
        )
        .await
        .expect("non-matching command should not disconnect");
    assert!(unaffected.success);

    manager
        .execute_command_with_context(
            device.connection_request().expect("disconnect request"),
            command("show version", 10),
            device.execution_context(),
        )
        .await
        .expect_err("first matching command should close the shell channel");

    let recovered = manager
        .execute_command_with_context(
            device.connection_request().expect("reconnect request"),
            command("show version", 10),
            device.execution_context(),
        )
        .await
        .expect("new connection should see the exhausted shared disconnect budget");
    assert!(recovered.success);
    assert_eq!(
        device.received_commands(),
        ["show clock", "show version", "show version"]
    );
}

#[tokio::test]
async fn command_delay_can_trigger_execution_timeout() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_command_delay(Duration::from_millis(1_200)),
    ))
    .await
    .expect("spawn delayed-command device");

    let error = SshConnectionManager::new()
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("show slow", 1),
            device.execution_context(),
        )
        .await
        .expect_err("response delay should exceed command timeout");
    assert!(matches!(error, ConnectError::ExecTimeout(_)));
    assert_eq!(device.received_commands(), ["show slow"]);
}
