//! Virtual-device E2E tests for the `array` template, including the
//! sys-formatted virtual-site switch (`switch {}`).

use rneter::session::SshConnectionManager;
use rneter::testkit::{DevicePersona, FakeSshDevice};

use crate::support;

#[tokio::test]
async fn array_full_scenario() {
    support::run_full_scenario("array").await;
}

#[tokio::test]
async fn array_switches_into_virtual_site_with_sys() {
    let device = FakeSshDevice::spawn(DevicePersona::builtin("array").expect("array persona"))
        .await
        .expect("spawn array device");
    let manager = SshConnectionManager::new();
    let context = device.execution_context().with_sys(Some("vs1".to_string()));

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            support::command("VSiteEnable", "run e2e-check"),
            context.clone(),
        )
        .await
        .expect("exec in virtual site enable mode");
    assert!(output.success, "output: {}", output.all);

    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            support::command("VSiteConfig", "run e2e-check"),
            context,
        )
        .await
        .expect("exec in virtual site config mode");
    assert!(output.success, "output: {}", output.all);

    let commands = device.received_commands();
    assert!(
        commands.contains(&"switch vs1".to_string()),
        "sys-formatted transition must reach the device: {commands:?}"
    );
}

#[tokio::test]
async fn array_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("array").await;
}
