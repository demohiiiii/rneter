//! End-to-end coverage for bounded reconnect and retry policies.

use std::time::Duration;

use rneter::device::{DeviceHandlerConfig, prompt_rule};
use rneter::error::ConnectError;
use rneter::session::{Command, CommandFlow, RetryPolicy, SessionOperation, SshConnectionManager};
use rneter::testkit::{DevicePersona, FakeSshDevice, FaultInjection};

fn persona(faults: FaultInjection) -> DevicePersona {
    let config = DeviceHandlerConfig {
        prompt: vec![prompt_rule("Exec", &[r"^retry-test#\s*$"])],
        error_regex: vec![r"^ERROR: .+$".to_string()],
        ..Default::default()
    };
    DevicePersona::for_config("retry-test", config, "exec", &[("exec", "retry-test#")])
        .with_faults(faults)
}

fn command(text: &str) -> Command {
    Command {
        mode: "Exec".to_string(),
        command: text.to_string(),
        timeout: Some(10),
        ..Command::default()
    }
}

fn immediate_retry(max_retries: usize) -> RetryPolicy {
    RetryPolicy::new(max_retries).with_backoff(Duration::ZERO, Duration::ZERO)
}

#[tokio::test]
async fn retries_are_disabled_by_default() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_disconnect_command("show version", 1),
    ))
    .await
    .expect("spawn disconnecting device");

    SshConnectionManager::new()
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("show version"),
            device.execution_context(),
        )
        .await
        .expect_err("default context must not retry");
    assert_eq!(device.received_commands(), ["show version"]);
}

#[tokio::test]
async fn transient_disconnect_reconnects_and_retries_command() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_disconnect_command("show version", 1),
    ))
    .await
    .expect("spawn disconnecting device");
    let context = device
        .execution_context()
        .with_retry_policy(immediate_retry(1));

    let output = SshConnectionManager::new()
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("show version"),
            context,
        )
        .await
        .expect("one retry should recover after disconnect");
    assert!(output.success);
    assert_eq!(device.received_commands(), ["show version", "show version"]);
}

#[tokio::test]
async fn flow_retry_resumes_at_first_unfinished_step() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_disconnect_command("show version", 1),
    ))
    .await
    .expect("spawn disconnecting device");
    let context = device
        .execution_context()
        .with_retry_policy(immediate_retry(1));
    let flow = CommandFlow::new(vec![command("show clock"), command("show version")]);

    let output = SshConnectionManager::new()
        .execute_operation_with_context(
            device.connection_request().expect("request"),
            SessionOperation::from(flow),
            context,
        )
        .await
        .expect("flow should resume after reconnect");

    assert!(output.success);
    assert_eq!(output.steps.len(), 2);
    assert!(output.steps.iter().all(|output| output.success));
    assert_eq!(output.steps[0].step_index, 0);
    assert_eq!(output.steps[1].step_index, 1);
    assert_eq!(
        device.received_commands(),
        ["show clock", "show version", "show version"]
    );
}

#[tokio::test]
async fn exhausted_flow_retries_preserve_completed_step_output() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_disconnect_command("show version", 2),
    ))
    .await
    .expect("spawn disconnecting device");
    let context = device
        .execution_context()
        .with_retry_policy(immediate_retry(1));
    let flow = CommandFlow::new(vec![command("show clock"), command("show version")]);

    let error = SshConnectionManager::new()
        .execute_operation_with_context(
            device.connection_request().expect("request"),
            SessionOperation::from(flow),
            context,
        )
        .await
        .expect_err("both attempts at the second step should disconnect");

    assert_eq!(error.partial_output().steps.len(), 1);
    assert_eq!(error.partial_output().steps[0].step_index, 0);
    assert_eq!(
        error.partial_output().steps[0].operation_summary,
        "show clock"
    );
    assert_eq!(
        device.received_commands(),
        ["show clock", "show version", "show version"]
    );
}

#[tokio::test]
async fn authentication_rejections_are_not_retried_by_default() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_rejected_auth_attempts(1),
    ))
    .await
    .expect("spawn flaky-auth device");

    SshConnectionManager::new()
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("show version"),
            device
                .execution_context()
                .with_retry_policy(immediate_retry(1)),
        )
        .await
        .expect_err("authentication retry requires explicit opt-in");
    assert!(device.received_commands().is_empty());
}

#[tokio::test]
async fn authentication_rejections_can_be_retried_explicitly() {
    let device = FakeSshDevice::spawn(persona(
        FaultInjection::new().with_rejected_auth_attempts(1),
    ))
    .await
    .expect("spawn flaky-auth device");
    let retry = immediate_retry(1).with_authentication_retries(true);

    let output = SshConnectionManager::new()
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("show version"),
            device.execution_context().with_retry_policy(retry),
        )
        .await
        .expect("explicit authentication retry should recover");
    assert!(output.success);
    assert_eq!(device.received_commands(), ["show version"]);
}

#[tokio::test]
async fn invalid_retry_policy_is_rejected_before_connecting() {
    let device = FakeSshDevice::spawn(persona(FaultInjection::new()))
        .await
        .expect("spawn device");
    let retry =
        RetryPolicy::new(1).with_backoff(Duration::from_millis(2), Duration::from_millis(1));

    let error = SshConnectionManager::new()
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command("show version"),
            device.execution_context().with_retry_policy(retry),
        )
        .await
        .expect_err("invalid policy must fail before connecting");
    assert!(matches!(error, ConnectError::InvalidRetryPolicy(_)));
    assert!(device.received_commands().is_empty());
}
