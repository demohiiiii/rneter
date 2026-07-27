//! Virtual-device E2E tests for the `cisco_ios` and `cisco_xe` templates.

use crate::support;
use rneter::session::SshConnectionManager;
use rneter::templates::DetectConnectPolicy;
use rneter::testkit::{DevicePersona, FakeSshDevice};

#[tokio::test]
async fn cisco_ios_full_scenario() {
    support::run_full_scenario("cisco_ios").await;
}

#[tokio::test]
async fn cisco_xe_full_scenario() {
    support::run_full_scenario("cisco_xe").await;
}

#[tokio::test]
async fn cisco_ios_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("cisco_ios").await;
}

#[tokio::test]
async fn cisco_xe_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("cisco_xe").await;
}

#[tokio::test]
async fn autodetect_and_connect_uses_calling_manager_pool() {
    let device =
        FakeSshDevice::spawn(DevicePersona::builtin("cisco_ios").expect("build cisco persona"))
            .await
            .expect("spawn cisco device");
    let manager = SshConnectionManager::new();

    let _connected = manager
        .autodetect_and_connect_with_context(
            device.detect_request(),
            device.persona().enable_password.clone(),
            device.execution_context(),
            DetectConnectPolicy::default(),
        )
        .await
        .expect("autodetect and connect with custom manager");

    let commands_after_connect = device.received_commands();
    assert_eq!(
        commands_after_connect
            .iter()
            .filter(|command| command.as_str() == "terminal pager 0")
            .count(),
        1,
        "the detected connection should run its after-connect hook once"
    );

    manager
        .get_with_context(
            device
                .connection_request()
                .expect("build connection request"),
            device.execution_context(),
        )
        .await
        .expect("reuse autodetected connection from custom manager");

    assert_eq!(
        device.received_commands(),
        commands_after_connect,
        "reusing the calling manager must not open a second connection"
    );
}
