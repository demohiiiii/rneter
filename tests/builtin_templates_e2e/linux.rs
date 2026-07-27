//! Virtual-device E2E tests for the `linux` template, including shell
//! exit-code capture.

use rneter::session::SshConnectionManager;
use rneter::testkit::{DevicePersona, ERROR_COMMAND, FakeSshDevice};

use crate::support;

#[tokio::test]
async fn linux_full_scenario() {
    support::run_full_scenario("linux").await;
}

#[tokio::test]
async fn linux_reports_shell_exit_codes() {
    let device = FakeSshDevice::spawn(DevicePersona::builtin("linux").expect("linux persona"))
        .await
        .expect("spawn linux device");
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            support::command("Root", "run e2e-check"),
            device.execution_context(),
        )
        .await
        .expect("exec as root");
    assert!(output.success, "output: {}", output.all);
    assert_eq!(output.exit_code, Some(0));

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            support::command("Root", ERROR_COMMAND),
            device.execution_context(),
        )
        .await
        .expect("exec failing command as root");
    assert!(!output.success);
    assert_eq!(output.exit_code, Some(1));

    // The sudo password challenge was answered on the wire.
    assert!(
        device.received_commands().contains(&"sudo -i".to_string()),
        "device must see the sudo transition: {:?}",
        device.received_commands()
    );
}

#[tokio::test]
async fn linux_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("linux").await;
}
