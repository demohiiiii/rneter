//! Virtual-device E2E tests for the `h3c_comware` and `hp_comware` templates.

use crate::support;
use rneter::session::{
    CmdJob, ConnectionMode, RollbackPolicy, SessionEvent, SessionRecordLevel, TxBlock, TxStep,
    TxWorkflow,
};
use std::time::Duration;
use tokio::sync::oneshot;

#[tokio::test]
async fn h3c_comware_full_scenario() {
    support::run_full_scenario("h3c_comware").await;
}

#[tokio::test]
async fn hp_comware_full_scenario() {
    support::run_full_scenario("hp_comware").await;
}

#[tokio::test]
async fn h3c_comware_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("h3c_comware").await;
}

#[tokio::test]
async fn hp_comware_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("hp_comware").await;
}

#[tokio::test]
async fn h3c_comware_collects_all_paged_output() {
    support::run_pager_scenario(
        "h3c_comware",
        "Enable",
        "display paged-output",
        "---- More ----",
    )
    .await;
}

#[tokio::test]
async fn repeated_recording_gets_reuse_the_connection_owned_recorder() {
    let device = rneter::testkit::FakeSshDevice::spawn(
        rneter::testkit::DevicePersona::builtin("h3c_comware").expect("build H3C persona"),
    )
    .await
    .expect("spawn H3C device");
    let manager = rneter::session::SshConnectionManager::new();
    let request = device.connection_request().expect("request");
    let context = device.execution_context();

    let (first_sender, first_recorder) = manager
        .get_with_recording_level_and_context(
            request.clone(),
            context.clone(),
            SessionRecordLevel::KeyEventsOnly,
        )
        .await
        .expect("first recording session");
    let (second_sender, second_recorder) = manager
        .get_with_recording_level_and_context(request, context, SessionRecordLevel::KeyEventsOnly)
        .await
        .expect("second recording session lookup");

    assert_eq!(
        first_recorder.id(),
        second_recorder.id(),
        "a cache hit must return the recorder owned by the existing SSH session"
    );

    for sender in [first_sender, second_sender] {
        let (responder, result) = oneshot::channel();
        sender
            .send(CmdJob {
                data: support::command("Enable", "display version"),
                sys: None,
                responder,
            })
            .await
            .expect("send command");
        result
            .await
            .expect("command response")
            .expect("command succeeds");
    }

    assert_eq!(
        device
            .received_commands()
            .iter()
            .filter(|command| command.as_str() == "screen-length disable")
            .count(),
        1,
        "repeated recording lookups must reuse one physical SSH session"
    );
    assert_eq!(
        first_recorder
            .entries()
            .expect("entries")
            .iter()
            .filter(|entry| matches!(&entry.event, SessionEvent::CommandOutput { command, .. } if command == "display version"))
            .count(),
        2,
        "the connection-owned recorder must capture commands from both senders"
    );
}

#[tokio::test]
async fn recorder_aware_transactions_reuse_the_connection_owned_session() {
    let device = rneter::testkit::FakeSshDevice::spawn(
        rneter::testkit::DevicePersona::builtin("h3c_comware").expect("build H3C persona"),
    )
    .await
    .expect("spawn H3C device");
    let manager = rneter::session::SshConnectionManager::new();
    let request = device.connection_request().expect("request");
    let context = device.execution_context();
    let (_sender, recorder) = manager
        .get_with_recording_level_and_context(
            request.clone(),
            context.clone(),
            SessionRecordLevel::KeyEventsOnly,
        )
        .await
        .expect("recording session");
    let block = TxBlock {
        name: "recorded-block".to_string(),
        rollback_policy: RollbackPolicy::None,
        steps: vec![TxStep::new(support::command("Enable", "display version"))],
        fail_fast: true,
    };

    let workflow_result = manager
        .execute_tx_workflow_with_recorder_and_context(
            request,
            TxWorkflow {
                name: "recorded-workflow".to_string(),
                blocks: vec![block],
                fail_fast: true,
            },
            context,
            recorder.clone(),
        )
        .await
        .expect("recorded transaction workflow");
    assert!(workflow_result.committed);

    assert_eq!(
        device
            .received_commands()
            .iter()
            .filter(|command| command.as_str() == "screen-length disable")
            .count(),
        1,
        "recorder-aware operations must reuse the connection-owned session"
    );
    let entries = recorder.entries().expect("entries");
    assert!(entries.iter().any(
        |entry| matches!(&entry.event, SessionEvent::TxBlockFinished { block_name, .. } if block_name == "recorded-block")
    ));
    assert!(entries.iter().any(
        |entry| matches!(&entry.event, SessionEvent::TxWorkflowFinished { workflow_name, .. } if workflow_name == "recorded-workflow")
    ));
}

#[tokio::test]
async fn one_shot_mode_bypasses_the_connection_pool() {
    let device = rneter::testkit::FakeSshDevice::spawn(
        rneter::testkit::DevicePersona::builtin("h3c_comware").expect("build H3C persona"),
    )
    .await
    .expect("spawn H3C device");
    let manager = rneter::session::SshConnectionManager::new();
    let context = device
        .execution_context()
        .with_connection_mode(ConnectionMode::OneShot);

    for _ in 0..2 {
        manager
            .execute_command_with_context(
                device.connection_request().expect("request"),
                support::command("Enable", "display version"),
                context.clone(),
            )
            .await
            .expect("one-shot command");
    }

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let commands = device.received_commands();
            if commands
                .iter()
                .filter(|command| command.as_str() == "exit")
                .count()
                >= 2
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("one-shot connections should close after each operation");

    assert_eq!(
        device
            .received_commands()
            .iter()
            .filter(|command| command.as_str() == "screen-length disable")
            .count(),
        2,
        "each one-shot operation must establish a fresh SSH session"
    );
}

#[tokio::test]
async fn one_shot_sender_accepts_only_one_command() {
    let device = rneter::testkit::FakeSshDevice::spawn(
        rneter::testkit::DevicePersona::builtin("h3c_comware").expect("build H3C persona"),
    )
    .await
    .expect("spawn H3C device");
    let manager = rneter::session::SshConnectionManager::new();
    let sender = manager
        .get_with_context(
            device.connection_request().expect("request"),
            device
                .execution_context()
                .with_connection_mode(ConnectionMode::OneShot),
        )
        .await
        .expect("one-shot sender");

    let (responder, result) = oneshot::channel();
    sender
        .send(CmdJob {
            data: support::command("Enable", "display version"),
            sys: None,
            responder,
        })
        .await
        .expect("first command");
    result
        .await
        .expect("first response")
        .expect("first command succeeds");

    let (responder, _result) = oneshot::channel();
    assert!(
        sender
            .send(CmdJob {
                data: support::command("Enable", "display current-configuration"),
                sys: None,
                responder,
            })
            .await
            .is_err(),
        "a one-shot sender must reject a second command"
    );
}
