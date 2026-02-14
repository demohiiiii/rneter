use super::*;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Session recording granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub enum SessionRecordLevel {
    /// Disable recording.
    Off,
    /// Record key events only.
    KeyEventsOnly,
    /// Record key events and raw chunks.
    #[default]
    Full,
}

/// A single recorded session event.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionRecordEntry {
    pub ts_ms: u128,
    pub event: SessionEvent,
}

/// Options for normalizing JSONL recordings into stable fixtures.
#[derive(Debug, Clone, Copy)]
pub struct NormalizeOptions {
    /// Keep raw shell chunk events.
    pub keep_raw_chunks: bool,
    /// Keep prompt-changed events.
    pub keep_prompt_changed: bool,
    /// Keep state-changed events.
    pub keep_state_changed: bool,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            keep_raw_chunks: false,
            keep_prompt_changed: false,
            keep_state_changed: true,
        }
    }
}

/// Supported recorded event types.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEvent {
    ConnectionEstablished {
        device_addr: String,
        #[serde(alias = "prompt")]
        prompt_after: String,
        #[serde(alias = "state")]
        fsm_prompt_after: String,
    },
    ConnectionClosed {
        reason: String,
        #[serde(default)]
        prompt_before: Option<String>,
        #[serde(default)]
        fsm_prompt_before: Option<String>,
    },
    CommandOutput {
        command: String,
        mode: String,
        #[serde(default)]
        prompt_before: Option<String>,
        #[serde(default)]
        prompt_after: Option<String>,
        #[serde(default)]
        fsm_prompt_before: Option<String>,
        #[serde(default)]
        fsm_prompt_after: Option<String>,
        success: bool,
        content: String,
        all: String,
    },
    PromptChanged {
        prompt: String,
    },
    StateChanged {
        state: String,
    },
    RawChunk {
        data: String,
    },
}

/// In-memory session recorder.
#[derive(Debug, Clone)]
pub struct SessionRecorder {
    level: SessionRecordLevel,
    entries: Arc<Mutex<Vec<SessionRecordEntry>>>,
}

impl SessionRecorder {
    /// Create a recorder with the given level.
    pub fn new(level: SessionRecordLevel) -> Self {
        Self {
            level,
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Current recording level.
    pub fn level(&self) -> SessionRecordLevel {
        self.level
    }

    /// Record a key-level event.
    pub fn record_event(&self, event: SessionEvent) -> Result<(), ConnectError> {
        if self.level == SessionRecordLevel::Off {
            return Ok(());
        }
        let mut guard = self
            .entries
            .lock()
            .map_err(|e| ConnectError::InternalServerError(format!("record lock error: {e}")))?;
        guard.push(SessionRecordEntry {
            ts_ms: now_ms(),
            event,
        });
        Ok(())
    }

    /// Record raw shell data chunk when enabled.
    pub fn record_raw_chunk(&self, data: String) -> Result<(), ConnectError> {
        if self.level != SessionRecordLevel::Full {
            return Ok(());
        }
        self.record_event(SessionEvent::RawChunk { data })
    }

    /// Snapshot all records.
    pub fn entries(&self) -> Result<Vec<SessionRecordEntry>, ConnectError> {
        let guard = self
            .entries
            .lock()
            .map_err(|e| ConnectError::InternalServerError(format!("record lock error: {e}")))?;
        Ok(guard.clone())
    }

    /// Clears all recorded events.
    pub fn clear(&self) -> Result<(), ConnectError> {
        let mut guard = self
            .entries
            .lock()
            .map_err(|e| ConnectError::InternalServerError(format!("record lock error: {e}")))?;
        guard.clear();
        Ok(())
    }

    /// Export records as JSONL.
    pub fn to_jsonl(&self) -> Result<String, ConnectError> {
        let entries = self.entries()?;
        let mut lines = Vec::with_capacity(entries.len());
        for entry in entries {
            let line = serde_json::to_string(&entry).map_err(|e| {
                ConnectError::InternalServerError(format!("record encode error: {e}"))
            })?;
            lines.push(line);
        }
        Ok(lines.join("\n"))
    }

    /// Restore recorder from JSONL lines.
    pub fn from_jsonl(jsonl: &str) -> Result<Self, ConnectError> {
        let recorder = Self::new(SessionRecordLevel::Full);
        if jsonl.trim().is_empty() {
            return Ok(recorder);
        }

        let mut parsed = Vec::new();
        for line in jsonl.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: SessionRecordEntry = serde_json::from_str(line).map_err(|e| {
                ConnectError::InternalServerError(format!("record decode error: {e}"))
            })?;
            parsed.push(entry);
        }

        let mut guard = recorder
            .entries
            .lock()
            .map_err(|e| ConnectError::InternalServerError(format!("record lock error: {e}")))?;
        *guard = parsed;
        drop(guard);

        Ok(recorder)
    }

    /// Normalize JSONL recording content into a stable fixture representation.
    ///
    /// This helper sorts events by timestamp and can filter out noisy events
    /// such as raw shell chunks.
    pub fn normalize_jsonl(jsonl: &str, options: NormalizeOptions) -> Result<String, ConnectError> {
        let recorder = Self::from_jsonl(jsonl)?;
        let mut indexed = recorder
            .entries()?
            .into_iter()
            .enumerate()
            .collect::<Vec<(usize, SessionRecordEntry)>>();

        indexed
            .sort_by(|(idx_a, a), (idx_b, b)| a.ts_ms.cmp(&b.ts_ms).then_with(|| idx_a.cmp(idx_b)));

        let filtered = indexed
            .into_iter()
            .filter_map(|(_, entry)| match &entry.event {
                SessionEvent::RawChunk { .. } if !options.keep_raw_chunks => None,
                SessionEvent::PromptChanged { .. } if !options.keep_prompt_changed => None,
                SessionEvent::StateChanged { .. } if !options.keep_state_changed => None,
                _ => Some(entry),
            })
            .collect::<Vec<_>>();

        let normalized = SessionRecorder {
            level: SessionRecordLevel::Full,
            entries: Arc::new(Mutex::new(filtered)),
        };
        normalized.to_jsonl()
    }
}

impl Default for SessionRecorder {
    fn default() -> Self {
        Self::new(SessionRecordLevel::Full)
    }
}

/// Offline replayer backed by session recording data.
#[derive(Debug, Clone)]
pub struct SessionReplayer {
    entries: Vec<SessionRecordEntry>,
    cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayContext {
    pub device_addr: String,
    pub prompt: String,
    pub fsm_prompt: String,
}

impl SessionReplayer {
    /// Build a replayer from a recorder snapshot.
    pub fn from_recorder(recorder: &SessionRecorder) -> Self {
        let entries = recorder.entries().unwrap_or_default();
        Self { entries, cursor: 0 }
    }

    /// Build a replayer from JSONL recording data.
    pub fn from_jsonl(jsonl: &str) -> Result<Self, ConnectError> {
        let recorder = SessionRecorder::from_jsonl(jsonl)?;
        Ok(Self::from_recorder(&recorder))
    }

    /// Returns initial connection context if present in recording.
    pub fn initial_context(&self) -> Option<ReplayContext> {
        for entry in &self.entries {
            if let SessionEvent::ConnectionEstablished {
                device_addr,
                prompt_after,
                fsm_prompt_after,
            } = &entry.event
            {
                return Some(ReplayContext {
                    device_addr: device_addr.clone(),
                    prompt: prompt_after.clone(),
                    fsm_prompt: fsm_prompt_after.clone(),
                });
            }
        }
        None
    }

    /// Replay the next recorded output for the given command.
    pub fn replay_next(&mut self, command: &str) -> Result<Output, ConnectError> {
        self.replay_next_internal(command, None)
    }

    /// Replay the next recorded output for a specific command and mode.
    pub fn replay_next_in_mode(
        &mut self,
        command: &str,
        mode: &str,
    ) -> Result<Output, ConnectError> {
        self.replay_next_internal(command, Some(mode))
    }

    /// Replay a script without SSH by consuming recorded command outputs.
    pub fn replay_script(&mut self, script: &[Command]) -> Result<Vec<Output>, ConnectError> {
        let mut outputs = Vec::with_capacity(script.len());
        for cmd in script {
            outputs.push(self.replay_next_in_mode(&cmd.command, &cmd.mode)?);
        }
        Ok(outputs)
    }

    fn replay_next_internal(
        &mut self,
        command: &str,
        mode: Option<&str>,
    ) -> Result<Output, ConnectError> {
        while self.cursor < self.entries.len() {
            let entry = &self.entries[self.cursor];
            self.cursor += 1;

            if let SessionEvent::CommandOutput {
                command: recorded_command,
                mode: recorded_mode,
                prompt_after,
                success,
                content,
                all,
                ..
            } = &entry.event
            {
                let command_match = recorded_command == command;
                let mode_match = mode
                    .map(|expected| expected.eq_ignore_ascii_case(recorded_mode))
                    .unwrap_or(true);
                if !command_match || !mode_match {
                    continue;
                }
                return Ok(Output {
                    success: *success,
                    content: content.clone(),
                    all: all.clone(),
                    prompt: prompt_after.clone(),
                });
            }
        }

        let msg = if let Some(mode) = mode {
            format!("no replayable output found for command '{command}' in mode '{mode}'")
        } else {
            format!("no replayable output found for command '{command}'")
        };
        Err(ConnectError::ReplayMismatchError(msg))
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOISY_FIXTURE: &str = r#"{"ts_ms":3,"event":{"kind":"raw_chunk","data":"chunk-2"}}
{"ts_ms":1,"event":{"kind":"connection_established","device_addr":"admin@10.0.0.1:22","prompt_after":"router#","fsm_prompt_after":"enable"}}
{"ts_ms":2,"event":{"kind":"prompt_changed","prompt":"router#"}}
{"ts_ms":4,"event":{"kind":"state_changed","state":"config"}}
{"ts_ms":5,"event":{"kind":"command_output","command":"show version","mode":"Enable","success":true,"content":"ok","all":"show version\nok\nrouter#"}}
"#;

    #[test]
    fn recorder_jsonl_roundtrip() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Full);
        recorder
            .record_event(SessionEvent::PromptChanged {
                prompt: "router#".to_string(),
            })
            .expect("record prompt");

        let jsonl = recorder.to_jsonl().expect("encode jsonl");
        let restored = SessionRecorder::from_jsonl(&jsonl).expect("decode jsonl");
        let entries = restored.entries().expect("entries");

        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0].event,
            SessionEvent::PromptChanged { .. }
        ));
    }

    #[test]
    fn replayer_returns_matching_command_output() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Full);
        recorder
            .record_event(SessionEvent::CommandOutput {
                command: "show version".to_string(),
                mode: "enable".to_string(),
                prompt_before: Some("router#".to_string()),
                prompt_after: Some("router#".to_string()),
                fsm_prompt_before: Some("enable".to_string()),
                fsm_prompt_after: Some("enable".to_string()),
                success: true,
                content: "ok".to_string(),
                all: "show version\nok\nrouter#".to_string(),
            })
            .expect("record command output");

        let mut replayer = SessionReplayer::from_recorder(&recorder);
        let output = replayer.replay_next("show version").expect("replay");

        assert!(output.success);
        assert_eq!(output.content, "ok");
    }

    #[test]
    fn replayer_supports_initial_context_for_offline_connection_tests() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Full);
        recorder
            .record_event(SessionEvent::ConnectionEstablished {
                device_addr: "admin@192.168.1.1:22".to_string(),
                prompt_after: "router#".to_string(),
                fsm_prompt_after: "enable".to_string(),
            })
            .expect("record connect");

        let replayer = SessionReplayer::from_recorder(&recorder);
        let ctx = replayer.initial_context().expect("context");

        assert_eq!(ctx.device_addr, "admin@192.168.1.1:22");
        assert_eq!(ctx.prompt, "router#");
        assert_eq!(ctx.fsm_prompt, "enable");
    }

    #[test]
    fn replay_script_can_test_command_flow_without_ssh() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Full);
        recorder
            .record_event(SessionEvent::CommandOutput {
                command: "terminal length 0".to_string(),
                mode: "enable".to_string(),
                prompt_before: Some("router#".to_string()),
                prompt_after: Some("router#".to_string()),
                fsm_prompt_before: Some("enable".to_string()),
                fsm_prompt_after: Some("enable".to_string()),
                success: true,
                content: "".to_string(),
                all: "terminal length 0\nrouter#".to_string(),
            })
            .expect("record output 1");
        recorder
            .record_event(SessionEvent::CommandOutput {
                command: "show version".to_string(),
                mode: "enable".to_string(),
                prompt_before: Some("router#".to_string()),
                prompt_after: Some("router#".to_string()),
                fsm_prompt_before: Some("enable".to_string()),
                fsm_prompt_after: Some("enable".to_string()),
                success: true,
                content: "Version 1.0".to_string(),
                all: "show version\nVersion 1.0\nrouter#".to_string(),
            })
            .expect("record output 2");

        let mut replayer = SessionReplayer::from_recorder(&recorder);
        let script = vec![
            Command {
                mode: "enable".to_string(),
                command: "terminal length 0".to_string(),
                timeout: None,
            },
            Command {
                mode: "enable".to_string(),
                command: "show version".to_string(),
                timeout: None,
            },
        ];
        let outputs = replayer.replay_script(&script).expect("replay script");
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[1].content, "Version 1.0");
    }

    #[test]
    fn replay_next_in_mode_detects_mismatch() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Full);
        recorder
            .record_event(SessionEvent::CommandOutput {
                command: "show version".to_string(),
                mode: "enable".to_string(),
                prompt_before: Some("router#".to_string()),
                prompt_after: Some("router(config)#".to_string()),
                fsm_prompt_before: Some("enable".to_string()),
                fsm_prompt_after: Some("config".to_string()),
                success: true,
                content: "ok".to_string(),
                all: "show version\nok\nrouter#".to_string(),
            })
            .expect("record output");

        let mut replayer = SessionReplayer::from_recorder(&recorder);
        let err = match replayer.replay_next_in_mode("show version", "config") {
            Ok(_) => panic!("mismatch mode should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, ConnectError::ReplayMismatchError(_)));
    }

    #[test]
    fn key_events_only_skips_raw_chunks() {
        let recorder = SessionRecorder::new(SessionRecordLevel::KeyEventsOnly);

        recorder
            .record_raw_chunk("raw-shell-data".to_string())
            .expect("record raw");
        recorder
            .record_event(SessionEvent::PromptChanged {
                prompt: "router#".to_string(),
            })
            .expect("record prompt");

        let entries = recorder.entries().expect("entries");
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0].event,
            SessionEvent::PromptChanged { .. }
        ));
    }

    #[test]
    fn off_level_records_nothing() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Off);

        recorder
            .record_event(SessionEvent::StateChanged {
                state: "enable".to_string(),
            })
            .expect("record state");
        recorder
            .record_raw_chunk("raw-shell-data".to_string())
            .expect("record raw");

        let entries = recorder.entries().expect("entries");
        assert!(entries.is_empty());
    }

    #[test]
    fn replay_next_returns_error_when_command_not_found() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Full);
        recorder
            .record_event(SessionEvent::CommandOutput {
                command: "show clock".to_string(),
                mode: "enable".to_string(),
                prompt_before: Some("router#".to_string()),
                prompt_after: Some("router#".to_string()),
                fsm_prompt_before: Some("enable".to_string()),
                fsm_prompt_after: Some("enable".to_string()),
                success: true,
                content: "12:00:00".to_string(),
                all: "show clock\n12:00:00\nrouter#".to_string(),
            })
            .expect("record command output");

        let mut replayer = SessionReplayer::from_recorder(&recorder);
        let err = match replayer.replay_next("show version") {
            Ok(_) => panic!("missing replay should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, ConnectError::ReplayMismatchError(_)));
    }

    #[test]
    fn from_jsonl_accepts_empty_input() {
        let restored = SessionRecorder::from_jsonl("").expect("decode empty jsonl");
        let entries = restored.entries().expect("entries");
        assert!(entries.is_empty());
    }

    #[test]
    fn recorder_clear_removes_all_entries() {
        let recorder = SessionRecorder::new(SessionRecordLevel::Full);
        recorder
            .record_event(SessionEvent::StateChanged {
                state: "enable".to_string(),
            })
            .expect("record state");
        recorder.clear().expect("clear");
        let entries = recorder.entries().expect("entries");
        assert!(entries.is_empty());
    }

    #[test]
    fn from_jsonl_supports_legacy_connection_prompt_field() {
        let legacy = r#"{"ts_ms":1,"event":{"kind":"connection_established","device_addr":"u@h:22","prompt":"r#","state":"enable"}}"#;
        let replayer = SessionReplayer::from_jsonl(legacy).expect("parse legacy");
        let ctx = replayer.initial_context().expect("context");
        assert_eq!(ctx.prompt, "r#");
        assert_eq!(ctx.fsm_prompt, "enable");
    }

    #[test]
    fn command_default_has_no_timeout() {
        let cmd = Command::default();
        assert_eq!(cmd.timeout, None);
        assert!(cmd.mode.is_empty());
        assert!(cmd.command.is_empty());
    }

    #[test]
    fn normalize_jsonl_filters_noise_and_sorts_by_timestamp() {
        let normalized =
            SessionRecorder::normalize_jsonl(NOISY_FIXTURE, NormalizeOptions::default())
                .expect("normalize");
        let restored = SessionRecorder::from_jsonl(&normalized).expect("restore normalized");
        let entries = restored.entries().expect("entries");

        assert_eq!(entries.len(), 3);
        assert!(matches!(
            entries[0].event,
            SessionEvent::ConnectionEstablished { .. }
        ));
        assert!(matches!(
            entries[1].event,
            SessionEvent::StateChanged { .. }
        ));
        assert!(matches!(
            entries[2].event,
            SessionEvent::CommandOutput { .. }
        ));
        assert!(entries[0].ts_ms <= entries[1].ts_ms && entries[1].ts_ms <= entries[2].ts_ms);
    }

    #[test]
    fn normalize_jsonl_can_keep_all_event_types() {
        let options = NormalizeOptions {
            keep_raw_chunks: true,
            keep_prompt_changed: true,
            keep_state_changed: true,
        };
        let normalized =
            SessionRecorder::normalize_jsonl(NOISY_FIXTURE, options).expect("normalize");
        let restored = SessionRecorder::from_jsonl(&normalized).expect("restore normalized");
        let entries = restored.entries().expect("entries");
        assert_eq!(entries.len(), 5);
    }
}
