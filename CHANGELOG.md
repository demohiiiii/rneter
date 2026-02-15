# Changelog

All notable changes to this project are documented in this file.

## [0.1.6] - 2026-02-15

### New Features
- Added a release-oriented changelog workflow that standardizes version notes into feature, optimization, API-change, and risk categories before publishing.

### Optimizations
- Simplified template/state-machine diagnostics by removing low-signal required-mode checks and focusing diagnostics on graph consistency and prompt/transition quality.
- Reduced maintenance overhead by removing redundant required-mode metadata wiring from template catalog generation.

### API Changes
- Removed `StateMachineDiagnostics.unreachable_required_modes`.
- Removed `DeviceHandler::diagnose_state_machine_with_required_modes(&[&str])`.
- Removed `TemplateMetadata.required_modes`.
- `templates::diagnose_template(name)` now directly uses `handler.diagnose_state_machine()`.

### Risks
- Any downstream code that referenced removed required-mode fields/methods will fail to compile until migrated.
- Integrations that relied on required-mode diagnostics semantics must switch to other diagnostics fields (for example `unreachable_states`, `dead_end_states`).

---

## [0.1.5] - 2026-02-15

### Added
- State-machine diagnostics coverage improvements.

### Changed
- Removed required-mode diagnostics to keep template validation focused on graph structure and prompt/transition quality.

### Usage
```rust
let handler = rneter::templates::cisco()?;
let report = handler.diagnose_state_machine();
assert!(!report.graph_states.is_empty());
```

### Migration Notes
- If your code used required-mode diagnostics APIs/fields, remove those usages and rely on graph diagnostics fields.

---

## [0.1.4] - 2026-02-15

### Added
- Session recording/replay system:
  - `SessionRecorder`, `SessionReplayer`
  - Recording levels: `Off`, `KeyEventsOnly`, `Full`
  - JSONL export/import and fixture normalization (`NormalizeOptions`)
- Connection security profiles:
  - `ConnectionSecurityOptions::secure_default()`
  - `ConnectionSecurityOptions::balanced()`
  - `ConnectionSecurityOptions::legacy_compatible()`
- Template ecosystem APIs:
  - `available_templates()`, `by_name()`
  - `template_catalog()`, `template_metadata()`
  - `diagnose_template_json()`, `diagnose_all_templates_json()`
- Prompt/state observability improvements:
  - `CommandOutput` event now records `prompt_before/prompt_after` and `fsm_prompt_before/fsm_prompt_after`
  - `Output.prompt` added

### Changed
- Session module split into focused files:
  - `src/session/security.rs`
  - `src/session/manager.rs`
  - `src/session/client.rs`
  - `src/session/recording.rs`
- Stability improvements in channel-close and SSH I/O select paths.
- Public API error handling hardened toward `Result` style in core paths.

### Usage

#### Secure defaults and custom security
```rust
use rneter::session::{MANAGER, ConnectionSecurityOptions};

let sender = MANAGER.get(
    "admin".to_string(),
    "192.168.1.1".to_string(),
    22,
    "password".to_string(),
    None,
    rneter::templates::cisco()?,
).await?;

let sender_legacy = MANAGER.get_with_security(
    "admin".to_string(),
    "192.168.1.1".to_string(),
    22,
    "password".to_string(),
    None,
    rneter::templates::cisco()?,
    ConnectionSecurityOptions::legacy_compatible(),
).await?;
```

#### Record and replay
```rust
use rneter::session::{MANAGER, SessionRecordLevel, SessionReplayer};

let (_sender, recorder) = MANAGER.get_with_recording_level(
    "admin".to_string(),
    "192.168.1.1".to_string(),
    22,
    "password".to_string(),
    None,
    rneter::templates::cisco()?,
    SessionRecordLevel::Full,
).await?;

let jsonl = recorder.to_jsonl()?;
let mut replayer = SessionReplayer::from_jsonl(&jsonl)?;
let output = replayer.replay_next_in_mode("show version", "Enable")?;
println!("{:?}", output.prompt);
```

#### Normalize fixtures for CI
```bash
cargo run --example normalize_fixture -- raw_session.jsonl tests/fixtures/session_new.jsonl
```

### Migration Notes
- `Command.cmd_type` and `Command.template` removed.
- Update callers to rely on `Command { mode, command, timeout }`.

---

## [0.1.3] - 2026-02-15

### Added
- CI quality improvements (including clippy checks).
