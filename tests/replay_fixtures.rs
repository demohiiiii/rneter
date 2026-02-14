use rneter::error::ConnectError;
use rneter::session::{Command, NormalizeOptions, SessionEvent, SessionRecorder, SessionReplayer};

const BASIC_FIXTURE: &str = include_str!("fixtures/session_replay_basic.jsonl");
const EXPECTED_SNAPSHOT: &str = include_str!("fixtures/session_replay_expected_snapshot.txt");
const FAILURE_FIXTURE: &str = include_str!("fixtures/session_replay_failure.jsonl");
const STATE_SWITCH_FIXTURE: &str = include_str!("fixtures/session_replay_state_switch.jsonl");
const LEGACY_FIXTURE: &str = r#"{"ts_ms":1,"event":{"kind":"connection_established","device_addr":"legacy@10.0.0.1:22","prompt":"legacy#","state":"enable"}}
{"ts_ms":2,"event":{"kind":"command_output","command":"show version","mode":"Enable","success":true,"content":"Legacy OS","all":"show version\nLegacy OS\nlegacy#"}}
"#;
const NOISY_FIXTURE: &str = r#"{"ts_ms":1,"event":{"kind":"connection_established","device_addr":"admin@192.168.1.1:22","prompt_after":"router#","fsm_prompt_after":"enable"}}
{"ts_ms":2,"event":{"kind":"raw_chunk","data":"junk"}}
{"ts_ms":3,"event":{"kind":"state_changed","state":"enable"}}
{"ts_ms":4,"event":{"kind":"prompt_changed","prompt":"router#"}}
{"ts_ms":5,"event":{"kind":"command_output","command":"show ip int br","mode":"Enable","prompt_before":"router#","prompt_after":"router#","fsm_prompt_before":"enable","fsm_prompt_after":"enable","success":true,"content":"Gi0/0 up","all":"show ip int br\nGi0/0 up\nrouter#"}}
"#;
const MISSING_PROMPT_AFTER_FIXTURE: &str = r#"{"ts_ms":1,"event":{"kind":"connection_established","device_addr":"admin@192.168.1.1:22","prompt_after":"router#","fsm_prompt_after":"enable"}}
{"ts_ms":2,"event":{"kind":"command_output","command":"show version","mode":"Enable","success":true,"content":"Version 1.0","all":"show version\nVersion 1.0\nrouter#"}}
"#;

#[test]
fn fixture_exposes_connection_context() {
    let replayer = SessionReplayer::from_jsonl(BASIC_FIXTURE).expect("load fixture");
    let ctx = replayer.initial_context().expect("context");

    assert_eq!(ctx.device_addr, "admin@192.168.1.1:22");
    assert_eq!(ctx.prompt, "router#");
    assert_eq!(ctx.fsm_prompt, "enable");
}

#[test]
fn fixture_replays_script_without_ssh() {
    let mut replayer = SessionReplayer::from_jsonl(BASIC_FIXTURE).expect("load fixture");
    let script = vec![
        Command {
            mode: "Enable".to_string(),
            command: "terminal length 0".to_string(),
            timeout: None,
        },
        Command {
            mode: "Enable".to_string(),
            command: "show version".to_string(),
            timeout: None,
        },
    ];

    let outputs = replayer.replay_script(&script).expect("replay script");
    assert_eq!(outputs.len(), 2);
    assert!(outputs[1].success);
    assert_eq!(outputs[1].content, "Version 1.0");
}

#[test]
fn fixture_reports_mismatch_for_wrong_mode() {
    let mut replayer = SessionReplayer::from_jsonl(BASIC_FIXTURE).expect("load fixture");
    let err = match replayer.replay_next_in_mode("show version", "Config") {
        Ok(_) => panic!("mismatch mode should fail"),
        Err(err) => err,
    };

    assert!(matches!(err, ConnectError::ReplayMismatchError(_)));
}

#[test]
fn fixture_replay_next_fills_output_prompt() {
    let mut replayer = SessionReplayer::from_jsonl(BASIC_FIXTURE).expect("load fixture");

    let output = replayer
        .replay_next_in_mode("show version", "Enable")
        .expect("replay show version");

    assert_eq!(output.prompt.as_deref(), Some("router#"));
}

#[test]
fn fixture_mode_match_is_case_insensitive() {
    let mut replayer = SessionReplayer::from_jsonl(BASIC_FIXTURE).expect("load fixture");

    let output = replayer
        .replay_next_in_mode("show version", "eNaBlE")
        .expect("case-insensitive mode match");
    assert!(output.success);
}

#[test]
fn legacy_fixture_still_replays_after_schema_changes() {
    let replayer = SessionReplayer::from_jsonl(LEGACY_FIXTURE).expect("load legacy fixture");
    let ctx = replayer.initial_context().expect("legacy context");

    assert_eq!(ctx.prompt, "legacy#");
    assert_eq!(ctx.fsm_prompt, "enable");
}

#[test]
fn replay_script_returns_error_when_middle_command_missing() {
    let mut replayer = SessionReplayer::from_jsonl(NOISY_FIXTURE).expect("load noisy fixture");
    let script = vec![
        Command {
            mode: "Enable".to_string(),
            command: "show ip int br".to_string(),
            timeout: None,
        },
        Command {
            mode: "Enable".to_string(),
            command: "show version".to_string(),
            timeout: None,
        },
    ];

    let err = match replayer.replay_script(&script) {
        Ok(_) => panic!("expected replay script failure"),
        Err(err) => err,
    };
    assert!(matches!(err, ConnectError::ReplayMismatchError(_)));
}

#[test]
fn replay_script_snapshot_matches_expected_output_sequence() {
    let mut replayer = SessionReplayer::from_jsonl(BASIC_FIXTURE).expect("load fixture");
    let script = vec![
        Command {
            mode: "Enable".to_string(),
            command: "terminal length 0".to_string(),
            timeout: None,
        },
        Command {
            mode: "Enable".to_string(),
            command: "show version".to_string(),
            timeout: None,
        },
    ];

    let outputs = replayer.replay_script(&script).expect("replay script");
    let actual = script
        .iter()
        .zip(outputs.iter())
        .map(|(cmd, out)| {
            format!(
                "{}|{}|{}|{}",
                cmd.command,
                out.success,
                out.content,
                out.prompt.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(actual, EXPECTED_SNAPSHOT.trim());
}

#[test]
fn replay_without_prompt_after_yields_none_output_prompt() {
    let mut replayer =
        SessionReplayer::from_jsonl(MISSING_PROMPT_AFTER_FIXTURE).expect("load fixture");
    let output = replayer
        .replay_next_in_mode("show version", "Enable")
        .expect("replay");
    assert_eq!(output.prompt, None);
}

#[test]
fn failure_fixture_replays_unsuccessful_command_output() {
    let mut replayer = SessionReplayer::from_jsonl(FAILURE_FIXTURE).expect("load fixture");
    let output = replayer
        .replay_next_in_mode("show running-config", "Enable")
        .expect("replay failure command");

    assert!(!output.success);
    assert!(output.content.contains("Invalid input"));
    assert_eq!(output.prompt.as_deref(), Some("router#"));
}

#[test]
fn state_switch_fixture_selects_output_by_mode() {
    let mut replayer = SessionReplayer::from_jsonl(STATE_SWITCH_FIXTURE).expect("load fixture");

    let config_output = replayer
        .replay_next_in_mode("show version", "Config")
        .expect("replay config mode output");
    assert_eq!(config_output.content, "Version in config context");
    assert_eq!(config_output.prompt.as_deref(), Some("router(config)#"));

    let enable_output = replayer
        .replay_next_in_mode("show version", "Enable")
        .expect("replay enable mode output");
    assert_eq!(enable_output.content, "Version in enable context");
    assert_eq!(enable_output.prompt.as_deref(), Some("router#"));
}

#[test]
fn replay_fixtures_have_basic_quality_guarantees() {
    let fixtures = [
        ("basic", BASIC_FIXTURE),
        ("failure", FAILURE_FIXTURE),
        ("state_switch", STATE_SWITCH_FIXTURE),
        ("legacy", LEGACY_FIXTURE),
        ("noisy", NOISY_FIXTURE),
        ("missing_prompt_after", MISSING_PROMPT_AFTER_FIXTURE),
    ];

    for (name, content) in fixtures {
        let recorder = SessionRecorder::from_jsonl(content).expect("parse fixture");
        let entries = recorder.entries().expect("entries");
        assert!(!entries.is_empty(), "fixture '{name}' should not be empty");

        let mut has_connection_established = false;
        let mut has_command_output = false;
        let mut last_ts = 0_u128;

        for (idx, entry) in entries.iter().enumerate() {
            if idx > 0 {
                assert!(
                    entry.ts_ms >= last_ts,
                    "fixture '{name}' has non-monotonic timestamp at index {idx}"
                );
            }
            last_ts = entry.ts_ms;

            match &entry.event {
                SessionEvent::ConnectionEstablished { .. } => has_connection_established = true,
                SessionEvent::CommandOutput { command, mode, .. } => {
                    has_command_output = true;
                    assert!(
                        !command.trim().is_empty(),
                        "fixture '{name}' contains empty command"
                    );
                    assert!(
                        !mode.trim().is_empty(),
                        "fixture '{name}' contains empty mode"
                    );
                }
                _ => {}
            }
        }

        assert!(
            has_connection_established,
            "fixture '{name}' should include connection_established"
        );
        assert!(
            has_command_output,
            "fixture '{name}' should include at least one command_output"
        );
    }
}

#[test]
fn fixture_normalization_removes_noise_by_default() {
    let normalized = SessionRecorder::normalize_jsonl(NOISY_FIXTURE, NormalizeOptions::default())
        .expect("normalize noisy fixture");
    let recorder = SessionRecorder::from_jsonl(&normalized).expect("parse normalized");
    let entries = recorder.entries().expect("entries");

    assert!(!entries.is_empty());
    assert!(
        !entries
            .iter()
            .any(|e| matches!(e.event, SessionEvent::RawChunk { .. }))
    );
    assert!(
        !entries
            .iter()
            .any(|e| matches!(e.event, SessionEvent::PromptChanged { .. }))
    );
}
