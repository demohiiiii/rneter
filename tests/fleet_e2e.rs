//! End-to-end coverage for bounded, failure-isolated fleet execution.

use rneter::session::{
    Command, FleetOptions, FleetTarget, SessionOperation, SshAuthMethod, SshConnectionManager,
};
use rneter::testkit::{DevicePersona, FakeSshDevice};

fn operation() -> SessionOperation {
    Command {
        mode: "Enable".to_string(),
        command: "show version".to_string(),
        timeout: Some(10),
        ..Command::default()
    }
    .into()
}

#[tokio::test]
async fn fleet_preserves_order_and_isolates_target_failures() {
    let first = FakeSshDevice::spawn(DevicePersona::builtin("cisco_ios").expect("persona"))
        .await
        .expect("first device");
    let failing = FakeSshDevice::spawn(DevicePersona::builtin("cisco_ios").expect("persona"))
        .await
        .expect("failing device");
    let third = FakeSshDevice::spawn(DevicePersona::builtin("cisco_ios").expect("persona"))
        .await
        .expect("third device");

    let expected_addresses = vec![
        first.connection_request().expect("request").device_addr(),
        failing.connection_request().expect("request").device_addr(),
        third.connection_request().expect("request").device_addr(),
    ];
    let targets = vec![
        FleetTarget::new(
            first.connection_request().expect("request"),
            first.execution_context(),
        ),
        FleetTarget::new(
            failing
                .connection_request_with_auth(SshAuthMethod::password("wrong-password"))
                .expect("request"),
            failing.execution_context(),
        ),
        FleetTarget::new(
            third.connection_request().expect("request"),
            third.execution_context(),
        ),
    ];

    let results = SshConnectionManager::new()
        .execute_on_fleet(targets, operation(), FleetOptions::new(2))
        .await
        .expect("fleet execution");

    assert_eq!(results.len(), 3);
    assert_eq!(
        results
            .iter()
            .map(|result| result.index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(
        results
            .iter()
            .map(|result| result.device_addr.clone())
            .collect::<Vec<_>>(),
        expected_addresses
    );
    assert!(results[0].is_ok());
    assert!(results[0].output().expect("first output").success);
    assert!(results[1].error().is_some());
    assert!(
        results[1]
            .error()
            .expect("target error")
            .partial_output()
            .steps
            .is_empty()
    );
    assert!(results[2].is_ok());
    assert!(results[2].output().expect("third output").success);
    assert!(
        first
            .received_commands()
            .contains(&"show version".to_string())
    );
    assert!(
        third
            .received_commands()
            .contains(&"show version".to_string())
    );
}

#[tokio::test]
async fn fleet_rejects_zero_concurrency_before_connecting() {
    let device = FakeSshDevice::spawn(DevicePersona::builtin("cisco_ios").expect("persona"))
        .await
        .expect("device");
    let targets = vec![FleetTarget::new(
        device.connection_request().expect("request"),
        device.execution_context(),
    )];

    let error = SshConnectionManager::new()
        .execute_on_fleet(targets, operation(), FleetOptions::new(0))
        .await
        .expect_err("zero concurrency must fail");

    assert!(matches!(
        error,
        rneter::error::ConnectError::InvalidFleetOptions(_)
    ));
    assert!(device.received_commands().is_empty());
}
