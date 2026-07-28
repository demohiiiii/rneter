//! Shared full-scenario driver used by every per-vendor virtual-device test.

use rneter::session::{
    Command, RollbackPolicy, SessionEvent, SessionRecordLevel, SessionRecorder,
    SshConnectionManager, TxBlock, TxStep,
};
use rneter::templates;
use rneter::testkit::{DevicePersona, ERROR_COMMAND, FakeSshDevice};
use rneter::{DetectConfidence, autodetect_with_context};

pub fn command(mode: &str, text: &str) -> Command {
    Command {
        mode: mode.to_string(),
        command: text.to_string(),
        timeout: Some(10),
        ..Command::default()
    }
}

/// Finds `needle` in `commands` at or after `from`, panicking with context.
fn position_from(commands: &[String], from: usize, needle: &str, name: &str) -> usize {
    commands
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, c)| c.as_str() == needle)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| {
            panic!("[{name}] device never received '{needle}' after index {from}: {commands:?}")
        })
}

/// Runs SSH autodetect against the virtual device of one built-in template.
///
/// Templates with a detect profile must be identified as themselves with
/// high confidence — the personas' realistic prompts and version-command
/// replies are exactly what the built-in probes score against, so this
/// doubles as a fidelity audit. Templates without a profile cannot be
/// identified, but their devices must never be mistaken for another
/// template with high confidence.
pub async fn run_autodetect_scenario(template: &str) {
    let persona = DevicePersona::builtin(template)
        .unwrap_or_else(|error| panic!("[{template}] build persona: {error}"));
    let name = persona.name.clone();
    let has_profile = templates::template_metadata(&name)
        .unwrap_or_else(|error| panic!("[{name}] template metadata: {error}"))
        .detect_profile
        .is_some();

    let device = FakeSshDevice::spawn(persona)
        .await
        .unwrap_or_else(|error| panic!("[{name}] spawn virtual device: {error}"));

    let report = autodetect_with_context(device.detect_request(), device.execution_context())
        .await
        .unwrap_or_else(|error| panic!("[{name}] autodetect: {error}"));

    if has_profile {
        let best = report
            .best_match
            .unwrap_or_else(|| panic!("[{name}] autodetect produced no candidate"));
        assert_eq!(
            best.template_name,
            name,
            "[{name}] autodetect picked the wrong template (score {}, candidates: {:?})",
            best.score,
            report
                .candidates
                .iter()
                .map(|c| format!("{}:{}", c.template_name, c.score))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            best.confidence,
            DetectConfidence::High,
            "[{name}] autodetect should identify its own virtual device with high confidence; score {}",
            best.score
        );
    } else if let Some(best) = report.best_match {
        assert_ne!(
            best.confidence,
            DetectConfidence::High,
            "[{name}] has no detect profile, but its device was identified as '{}' with high confidence",
            best.template_name
        );
    }
}

/// Verifies that a built-in template answers a device-side confirmation
/// challenge with its configured static response.
pub async fn run_confirmation_scenario(
    template: &str,
    mode: &str,
    command_text: &str,
    expected_response: &str,
) {
    let persona = DevicePersona::builtin(template)
        .unwrap_or_else(|error| panic!("[{template}] build persona: {error}"));
    let name = persona.name.clone();
    let device = FakeSshDevice::spawn(persona)
        .await
        .unwrap_or_else(|error| panic!("[{name}] spawn virtual device: {error}"));
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device
                .connection_request()
                .unwrap_or_else(|error| panic!("[{name}] build request: {error}")),
            command(mode, command_text),
            device.execution_context(),
        )
        .await
        .unwrap_or_else(|error| panic!("[{name}] execute confirmation command: {error}"));
    assert!(output.success, "[{name}] confirmation command failed");

    let commands = device.received_commands();
    let command_index = position_from(&commands, 0, command_text, &name);
    let response_index = position_from(&commands, command_index + 1, expected_response, &name);
    assert_eq!(
        response_index,
        command_index + 1,
        "[{name}] confirmation response must immediately follow its command: {commands:?}"
    );
}

/// Verifies that a template advances through every page and returns one
/// cleaned output without leaking pager prompts.
pub async fn run_pager_scenario(
    template: &str,
    mode: &str,
    command_text: &str,
    pager_prompt: &str,
) {
    let pages = [
        "page-one unique output",
        "page-two unique output",
        "page-three unique output",
    ];
    let persona = DevicePersona::builtin(template)
        .unwrap_or_else(|error| panic!("[{template}] build persona: {error}"))
        .with_paged_reply(command_text, pager_prompt, pages);
    let name = persona.name.clone();
    let device = FakeSshDevice::spawn(persona)
        .await
        .unwrap_or_else(|error| panic!("[{name}] spawn virtual device: {error}"));
    let manager = SshConnectionManager::new();

    let output = manager
        .execute_command_with_context(
            device
                .connection_request()
                .unwrap_or_else(|error| panic!("[{name}] build request: {error}")),
            command(mode, command_text),
            device.execution_context(),
        )
        .await
        .unwrap_or_else(|error| panic!("[{name}] execute paged command: {error}"));
    assert!(output.success, "[{name}] paged command failed");
    assert!(
        !output.content.contains(pager_prompt),
        "[{name}] cleaned output leaked pager prompt: {}",
        output.content
    );

    let mut previous_position = 0;
    for page in pages {
        let position = output.content[previous_position..]
            .find(page)
            .map(|offset| previous_position + offset)
            .unwrap_or_else(|| panic!("[{name}] output is missing {page:?}: {}", output.content));
        previous_position = position + page.len();
    }

    let commands = device.received_commands();
    let command_index = position_from(&commands, 0, command_text, &name);
    assert_eq!(
        commands[command_index + 1..]
            .iter()
            .filter(|command| command.as_str() == " ")
            .count(),
        pages.len() - 1,
        "[{name}] must send one space per page boundary: {commands:?}"
    );
}

/// Runs the full end-to-end scenario against the virtual device of one
/// built-in template.
///
/// Covers: SSH connect (including after-connect hooks), prompt detection, a
/// benign command in every prompt state (walking all transition edges
/// forward and back), vendor-styled error detection, transaction rollback
/// ordering verified from the device's own command log, and session
/// recording.
pub async fn run_full_scenario(template: &str) {
    let persona = DevicePersona::builtin(template)
        .unwrap_or_else(|error| panic!("[{template}] build persona: {error}"));
    let name = persona.name.clone();
    let benign_reply = persona.benign_reply.clone();
    let canned_replies = persona.canned_replies.clone();
    let prompt_states: Vec<String> = persona
        .config
        .prompt
        .iter()
        .map(|rule| rule.state.to_ascii_lowercase())
        .collect();

    let device = FakeSshDevice::spawn(persona)
        .await
        .unwrap_or_else(|error| panic!("[{name}] spawn virtual device: {error}"));
    let manager = SshConnectionManager::new();

    let recorder = SessionRecorder::new(SessionRecordLevel::KeyEventsOnly);
    manager
        .get_with_recorder_and_context(
            device
                .connection_request()
                .unwrap_or_else(|error| panic!("[{name}] build request: {error}")),
            device.execution_context(),
            recorder.clone(),
        )
        .await
        .unwrap_or_else(|error| panic!("[{name}] connect: {error}"));

    // Walk every prompt state forward, then back to the initial state, so
    // each transition edge (including exits) is exercised on the wire.
    let mut walk = prompt_states.clone();
    walk.extend(prompt_states.iter().rev().skip(1).cloned());
    for state in &walk {
        let output = manager
            .execute_command_with_context(
                device.connection_request().expect("request"),
                command(state, "run e2e-check"),
                device.execution_context(),
            )
            .await
            .unwrap_or_else(|error| panic!("[{name}] exec in mode '{state}': {error}"));
        assert!(
            output.success,
            "[{name}] command in mode '{state}' should succeed; full output: {}",
            output.all
        );
        assert!(
            output.content.contains(&benign_reply),
            "[{name}] output in mode '{state}' should contain the canned reply; got: {}",
            output.content
        );
    }

    // Realistic vendor replies (e.g. `show version`) round-trip verbatim.
    assert!(
        !canned_replies.is_empty(),
        "[{name}] persona should imitate at least one real command"
    );
    for (canned_command, canned_output) in &canned_replies {
        let output = manager
            .execute_command_with_context(
                device.connection_request().expect("request"),
                command(&prompt_states[0], canned_command),
                device.execution_context(),
            )
            .await
            .unwrap_or_else(|error| panic!("[{name}] exec '{canned_command}': {error}"));
        assert!(
            output.success,
            "[{name}] '{canned_command}' should succeed; full output: {}",
            output.all
        );
        // Every line of the canned reply must round-trip into the cleaned
        // output, so persona definitions cannot silently drift or lose
        // lines to output filtering.
        for expected_line in canned_output.lines().filter(|line| !line.trim().is_empty()) {
            assert!(
                output.content.contains(expected_line),
                "[{name}] '{canned_command}' output is missing the line {expected_line:?}; got: {}",
                output.content
            );
        }
    }

    // Vendor-styled error output must be classified as a failure.
    let output = manager
        .execute_command_with_context(
            device.connection_request().expect("request"),
            command(&prompt_states[0], ERROR_COMMAND),
            device.execution_context(),
        )
        .await
        .unwrap_or_else(|error| panic!("[{name}] exec error command: {error}"));
    assert!(
        !output.success,
        "[{name}] error reply must mark the command as failed; output: {}",
        output.all
    );

    // Transaction rollback: forward change, forced failure, compensation —
    // ordering is asserted from the device's own command log.
    let deepest = prompt_states
        .last()
        .expect("template has at least one prompt state")
        .clone();
    let block = TxBlock {
        name: format!("{name}-tx"),
        rollback_policy: RollbackPolicy::PerStep,
        fail_fast: true,
        steps: vec![
            TxStep {
                run: command(&deepest, "apply e2e-change").into(),
                rollback: Some(command(&deepest, "undo e2e-change").into()),
                rollback_on_failure: false,
            },
            TxStep {
                run: command(&deepest, ERROR_COMMAND).into(),
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
        .unwrap_or_else(|error| panic!("[{name}] execute tx block: {error}"));
    assert!(!result.committed, "[{name}] failed block must not commit");
    assert!(result.rollback_attempted, "[{name}] rollback must run");
    assert!(
        result.rollback_succeeded,
        "[{name}] rollback errors: {:?}",
        result.rollback_errors
    );

    let commands = device.received_commands();
    let apply_pos = position_from(&commands, 0, "apply e2e-change", &name);
    let fail_pos = position_from(&commands, apply_pos + 1, ERROR_COMMAND, &name);
    let undo_pos = position_from(&commands, fail_pos + 1, "undo e2e-change", &name);
    assert!(apply_pos < fail_pos && fail_pos < undo_pos);

    // The recorder observed the session.
    let entries = recorder.entries().expect("recorder entries");
    assert!(
        entries
            .iter()
            .any(|entry| matches!(&entry.event, SessionEvent::ConnectionEstablished { .. })),
        "[{name}] recording must contain the connection event"
    );
    assert!(
        entries.iter().any(|entry| matches!(
            &entry.event,
            SessionEvent::CommandOutput { command, .. } if command.contains("e2e-check")
        )),
        "[{name}] recording must contain command outputs"
    );
}
