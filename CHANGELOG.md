# Changelog

All notable changes to this project are documented in this file.

## [0.2.0] - 2026-02-18

### New Features
- Added transaction block execution APIs for command groups with rollback support:
  - `SshConnectionManager::execute_tx_block(...)`
  - `SharedSshClient::execute_tx_block(...)`
- Added workflow-level all-or-nothing orchestration for multi-block scenarios (for example firewall address/service/policy publishing):
  - `TxWorkflow`, `TxWorkflowResult`
  - `SshConnectionManager::execute_tx_workflow(...)`
- Added template-level transaction helpers:
  - `templates::classify_command(...)`
  - `templates::build_tx_block(...)`
- Added firewall workflow example with diagnostics precheck and dry-run planning output:
  - `examples/firewall_workflow.rs`

### Optimizations
- Improved rollback determinism by extracting global workflow rollback ordering into reusable logic (`workflow_rollback_order`), with dedicated tests.
- Improved transaction observability by recording lifecycle events for blocks and workflows, including rollback phases.
- Improved maintainability by centralizing transaction model validation and rollback planning in `src/session/transaction.rs`.

### API Changes
- Added new transaction model types:
  - `CommandBlockKind`, `RollbackPolicy`, `TxStep`, `TxBlock`, `TxResult`
  - `TxWorkflow`, `TxWorkflowResult`
- Added new error variant: `ConnectError::InvalidTransaction(String)`.
- Added new session recording events:
  - `tx_block_started`, `tx_step_succeeded`, `tx_step_failed`
  - `tx_rollback_started`, `tx_rollback_step_succeeded`, `tx_rollback_step_failed`
  - `tx_block_finished`, `tx_workflow_started`, `tx_workflow_finished`

### Risks
- Workflow rollback across previously committed blocks is compensation-based (CLI Saga style), not device-native atomic rollback; devices with side effects outside modeled commands can still drift.
- Template rollback inference is heuristic per vendor style (`no` / `undo` / `set->delete`); ambiguous commands should use explicit `resource_rollback_command` to avoid incorrect compensation.
- Existing integrations that parse recording JSONL by strict event whitelist must be updated to tolerate new transaction event kinds.

---

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
