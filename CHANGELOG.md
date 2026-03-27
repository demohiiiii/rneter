# Changelog

All notable changes to this project are documented in this file.

## [0.4.0] - 2026-03-27

### New Features
- Added generic session execution abstractions through `SessionOperation`, `SessionOperationSummary`, `SessionOperationOutput`, `SessionOperationStepOutput`, and `SessionOperationExecutionError`, so commands, flows, and template-rendered operations now share one execution and result model.
- Added nested transaction and workflow child-step reporting through `TxOperationStepResult`, `TxStepResult.forward_operation_steps`, `TxStepResult.rollback_operation_steps`, and `TxResult.block_rollback_steps`, exposing concrete forward and rollback sub-step outputs to callers.
- Added richer transaction recording details by extending `SessionEvent::TxStepSucceeded`, `SessionEvent::TxStepFailed`, `SessionEvent::TxRollbackStepSucceeded`, and `SessionEvent::TxRollbackStepFailed` with `operation_steps`.

### Optimizations
- Unified command, flow, and template execution on one operation executor path, so manager-level command and flow entrypoints now reuse the same operation-level execution pipeline.
- Preserved partial child-step outputs when multi-step operations fail due to timeouts, disconnects, or other execution errors, improving observability for both transaction rollback handling and direct operation execution.
- Corrected rollback recording to use the original transaction step index instead of the rollback-plan index, keeping recorder output aligned with transaction step reports.

### API Changes
- Transaction and workflow steps are now modeled around `SessionOperation`, including `TxStep.run`, `TxStep.rollback`, and `RollbackPolicy::WholeResource { rollback, .. }`, so callers can pass single commands, command flows, or template-backed operations.
- Added `SshConnectionManager::execute_operation_with_context(...) -> Result<SessionOperationOutput, SessionOperationExecutionError>` as the operation-level execution entrypoint with partial-output-aware failure reporting.
- Transaction result and recording schemas now expose operation-oriented fields such as `operation_summary`, `forward_operation_steps`, `rollback_operation_steps`, `block_rollback_steps`, and recorder `operation_steps`; downstream JSON/log consumers must migrate from the older command-only transaction shape.

### Risks
- This is a breaking release for integrations that deserialize or persist pre-`0.4.0` transaction/workflow results or transaction recording events, because the operation-oriented schema replaces the older command-only shape.
- `SessionRecordLevel::KeyEventsOnly` transaction events can now carry nested `operation_steps`, which may increase JSONL size and include more command output detail than earlier releases.
- Integrations adopting the new operation-level API need to handle `SessionOperationExecutionError` if they want to preserve and surface `partial_output()` instead of treating all failures as bare `ConnectError`.

## [0.3.7] - 2026-03-27

### New Features
- Added structured reusable command-flow template types through `CommandFlowTemplate`, `CommandFlowTemplateText`, `CommandFlowTemplateVar`, `CommandFlowTemplateStep`, `CommandFlowTemplatePrompt`, and `CommandFlowTemplateRuntime`, so interactive device workflows can now be modeled in Rust without protocol-specific request wrappers.
- Added built-in `templates::cisco_like_copy_template()` as a reusable Cisco-like copy wizard template for `copy scp:` / `copy tftp:` flows rendered through the generic command-flow pipeline.
- Updated the crate-level docs plus English and Chinese README examples to demonstrate rendering built-in copy workflows from template runtime vars before executing them with `execute_command_flow_with_context(...)`.

### Optimizations
- Consolidated CLI copy workflows onto the same structured template abstraction used by other interactive command flows, reducing one-off logic in the transfer template module.
- Removed legacy transfer-specific request validation and template-selection plumbing from the public surface, leaving built-in copy behavior defined in one reusable template.
- Simplified error handling by dropping transfer-only error variants now that copy workflows are rendered through generic command-flow templates instead of dedicated helper APIs.

### API Changes
- Removed `FileTransferRequest`, `FileTransferProtocol`, `FileTransferDirection`, `templates::build_file_transfer_flow(...)`, and `templates::build_file_transfer_command(...)`; callers should now render `templates::cisco_like_copy_template()` or another `CommandFlowTemplate` with `CommandFlowTemplateRuntime`.
- Removed `ConnectError::InvalidTransferRequest` and `ConnectError::TransferNotSupported`, so downstream code matching those variants must migrate to template-level validation and generic command-flow errors.
- `templates` now publicly exports `cisco_like_copy_template()` plus the structured command-flow template types as the supported way to package reusable interactive copy workflows.

### Risks
- This release is a breaking API change for any integration still compiling against the removed CLI transfer helper types, builder functions, or transfer-specific error variants.
- The built-in `cisco_like_copy_template()` still assumes Cisco-like prompt wording and a single-step `copy` wizard; vendors with different prompt text still need their own template definitions.
- Protocol-specific requirements such as SCP credentials are no longer enforced by a dedicated builder API, so missing runtime vars will render empty prompt responses unless callers validate them beforehand.

## [0.3.6] - 2026-03-27

### New Features
- Added multi-step interactive command execution through `CommandFlow`, `CommandInteraction`, `PromptResponseRule`, `CommandFlowOutput`, and `SshConnectionManager::execute_command_flow_with_context(...)`, allowing one cached session to drive wizard-like CLI workflows.
- Added template-layer file transfer builders through `FileTransferRequest`, `FileTransferProtocol`, `FileTransferDirection`, and `templates::build_file_transfer_flow(...)`, so CLI `scp`/`tftp` flows are now provided as reusable template helpers instead of session-specific APIs.
- Added runtime interaction validation via `ConnectError::InvalidCommandInteraction`, surfacing empty or invalid prompt regex definitions before command execution enters the SSH read loop.

### Optimizations
- Prioritized per-command runtime prompt-response rules ahead of template static input rules, allowing protocol-specific interactions to be injected on demand without mutating device handler definitions.
- Moved Cisco-like CLI transfer prompt handling out of the built-in device handlers and into template-side flow builders, simplifying the network templates back to prompt/state-machine concerns only.
- Reduced session-layer coupling by collapsing transfer-specific request modeling into the `templates` module while keeping the core SSH executor focused on generic command and flow execution.

### API Changes
- Removed the session-layer CLI transfer request and manager APIs (`DeviceFileTransferRequest`, `DeviceFileTransferProtocol`, `DeviceFileTransferDirection`, `SshConnectionManager::transfer_file_with_context(...)`, and `SshConnectionManager::transfer_file_flow_with_context(...)`); callers should now build flows through `templates::build_file_transfer_flow(...)` and execute them with `execute_command_flow_with_context(...)`.
- `CommandDynamicParams` is now back to generic runtime overrides (`EnablePassword`, `SudoPassword`, and `extra`) and no longer exposes transfer-specific fields; interactive protocol wizards should use `Command.interaction`.
- The public CLI transfer helper types now live under `templates` (`FileTransferRequest`, `FileTransferProtocol`, `FileTransferDirection`) instead of `session`.

### Risks
- This release is a breaking API change for integrations that still depend on the removed session-layer SCP/TFTP request or manager entrypoints.
- Built-in CLI transfer flow builders still cover only the Cisco-like template set (`cisco`, `arista`, `chaitin`, `maipu`, `venustech`); additional vendors still need their own builder implementations.
- Runtime prompt matching is now driven by flow-level regexes, so vendor prompt wording drift may require builder-level prompt updates even when the underlying device template remains unchanged.

## [0.3.5] - 2026-03-26

### New Features
- Added SFTP upload support through `FileUploadRequest` and `SshConnectionManager::upload_file_with_context(...)`, plus `FileUploadStarted` and `FileUploadFinished` session recording events.
- Added device-driven CLI transfer support through `DeviceFileTransferRequest`, `templates::build_file_transfer_command(...)`, and `SshConnectionManager::transfer_file_with_context(...)` for the built-in `cisco`, `arista`, `chaitin`, `maipu`, and `venustech` templates.
- Added `SshConnectionManager::execute_command_with_context(...)` so callers can run a structured `Command` directly without building a `CmdJob`, which the CLI transfer workflow now reuses internally.

### Optimizations
- Changed per-command interactive prompt overrides to merge and restore around one command execution, so transfer credentials and confirmations do not leak into cached connection state.
- Preserved template-defined dynamic prompt parameters during connection initialization by merging `EnablePassword` into the existing handler configuration instead of overwriting the template map.
- Made Linux shell exit-status wrappers configurable per shell flavor, so POSIX shells keep using `$?` while `fish` sessions use `$status`.

### API Changes
- `Command.dyn_params` is now the structured `CommandDynamicParams` type instead of a raw `HashMap<String, String>`, with named transfer fields plus an `extra` map for template-specific prompts.
- Added public transfer-facing types and helpers: `FileUploadRequest`, `DeviceFileTransferRequest`, `DeviceFileTransferProtocol`, `DeviceFileTransferDirection`, `templates::build_file_transfer_command(...)`, `ConnectError::InvalidTransferRequest`, and `ConnectError::TransferNotSupported`.
- Added `DeviceShellFlavor` plus `shell_flavor` on Linux shell exit-status configuration so callers can explicitly target `posix` or `fish`.

### Risks
- `upload_file_with_context(...)` requires the remote SSH server to expose the `sftp` subsystem; many network devices still do not.
- Built-in CLI transfer workflows currently cover only the listed Cisco-like templates, and real device prompt wording may still require template regex tuning.
- Device-side `copy scp:` and `copy tftp:` flows depend on the device being able to reach the target SCP/TFTP server directly; `rneter` only drives the CLI exchange and does not proxy the file transfer itself.

## [0.3.4] - 2026-03-24

### New Features
- Added public handler configuration exports under `device`, including `DeviceHandlerConfig`, `DeviceCommandExecutionConfig`, prompt/input/transition rule structs, and helper constructors for building custom templates from declarative data.
- Added built-in template config exporters such as `templates::cisco_config()`, `templates::huawei_config()`, `templates::fortinet_config()`, and `templates::linux_handler_config(...)`, so callers can start from a shipped template and extend it before building a handler.
- Added `templates::by_name_config(...)` for case-insensitive lookup of built-in template configurations without immediately constructing a `DeviceHandler`.

### Optimizations
- Unified template construction so direct template builders, registry lookups, and exported configs now share the same config-based build path, reducing drift between `templates::*`, `templates::by_name(...)`, and their underlying FSM definitions.
- Expanded network template coverage to verify direct builders, config rebuilds, and registry resolution all produce equivalent handlers across the built-in vendor set.
- Hardened Linux prompt parsing during connection initialization by stripping ANSI/OSC/CSI/DCS terminal control sequences and recognizing common `fish`-style prompts, reducing false initialization timeouts on interactive shells.

### API Changes
- `DeviceHandler::new(...)` now accepts a single `DeviceHandlerConfig` instead of the previous multi-argument state-machine constructor. Callers that instantiated handlers directly must migrate to the config-based form.
- `DeviceHandlerConfig::build()` and `DeviceHandler::from_config(...)` are now the supported construction helpers for declarative handler creation.
- Built-in template modules now expose config-oriented entrypoints in addition to handler builders, and `templates::by_name(...)` is internally backed by `templates::by_name_config(...).build()`.

### Risks
- This release is a breaking API change for any downstream code that still called the old multi-argument `DeviceHandler::new(...)` signature directly.
- Exported template configs make it easier for callers to mutate low-level regex and transition rules; invalid customizations will still fail at build time, but downstream wrappers should be prepared to surface `InvalidDeviceHandlerConfig`.
- Linux prompt compatibility is broader than before, but hosts with heavily customized prompts may still need explicit `LinuxTemplateConfig.custom_prompts` overrides.

## [0.3.3] - 2026-03-24

### New Features
- Added Linux shell exit-status execution support so `templates::linux()` handlers can append an exit-code marker, parse `$?`, and return it through command results.
- Added step-level transaction observability via `TxStepResult`, `TxStepExecutionState`, `TxStepRollbackState`, and `TxResult.step_results`, making per-step forward/rollback outcomes available to callers.
- Extended session recording and replay so `SessionEvent::CommandOutput` can persist and restore optional `exit_code` values for offline Linux-oriented test flows.

### Optimizations
- Refined rollback planning and reporting so rollback commands stay associated with their originating steps and missing-rollback reasons are propagated more clearly.
- Updated workflow compensation handling to write rollback outcomes back into previously committed block results, so final workflow reports reflect both forward execution and later compensation.
- Expanded the firewall workflow example and README snippets to print step-level execution and rollback details directly from workflow results.

### API Changes
- `Output` now includes `exit_code: Option<i32>`, which gives callers a shell-level success signal in addition to prompt-based success.
- `TxResult` now includes `step_results: Vec<TxStepResult>`, and `session` now re-exports `TxStepResult`, `TxStepExecutionState`, and `TxStepRollbackState`.
- `SessionEvent::CommandOutput` now carries an optional `exit_code` field with `serde(default)`, so JSONL consumers should allow the additional field when decoding newer recordings.

### Risks
- Linux exit-status capture wraps shell commands with an appended `printf`; nonstandard shells or tooling that depends on exact echoed command text should be verified before broad rollout.
- Transaction payloads are now larger because each block can return full `step_results`; downstream log pipelines, snapshot fixtures, or strict schema consumers may need adjustment.
- Workflow rollback now mutates previously committed block results to annotate compensation outcomes, so consumers that assumed committed blocks never show rollback activity should update their assumptions.

## [0.3.2] - 2026-03-23

### New Features
- Added Linux server support through `templates::linux()` and `templates::linux_with_config(...)`, including `sudo -i`, `sudo -s`, `su -`, and direct-root privilege escalation modes plus Linux-specific command classification.
- Added SSH security profiles via `SecurityLevel` and `ConnectionSecurityOptions`, so callers can choose secure, balanced, or legacy-compatible connection defaults through the structured session context.
- Expanded the built-in template catalog with additional network vendor templates (`arista`, `chaitin`, `checkpoint`, `dptech`, `fortinet`, `maipu`, `paloalto`, `qianxin`, `topsec`, `venustech`) and Fortinet VDOM-aware template support.

### Optimizations
- Split the `templates` module into catalog, registry, transaction, Linux, and per-vendor network submodules, reducing the size and coupling of the previous monolithic template implementation.
- Split the large session client and device state-machine implementations into focused internal submodules (`connection`, `command`, `tx`, `builder`, `runtime`, `diagnostics`, `transitions`) while keeping the public entrypoints stable.
- Hardened Linux transaction helpers by rejecting shell metacharacter injection patterns and validating package/service identifiers before classifying or building rollback-capable operations.

### API Changes
- `templates::build_tx_block(...)` no longer infers rollback commands automatically; config-style blocks now require an explicit `resource_rollback_command`.
- New public template exports are available for Linux and the expanded vendor set through `templates::*`, and `templates::by_name(...)` now recognizes the new built-in template names.
- Session security configuration is now exposed as public structured types: `ConnectionSecurityOptions` and `SecurityLevel`.

### Risks
- This release includes a behavioral break for callers that relied on automatic rollback inference; those integrations must now construct explicit compensating commands before calling `build_tx_block(...)`.
- Linux privilege escalation depends on prompt matching; hosts with unusual shell prompts may require `LinuxTemplateConfig.custom_prompts` to avoid mode-detection drift.
- `ConnectionSecurityOptions::legacy_compatible()` disables host-key verification (`NoCheck`) to maximize compatibility with older devices, which is a deliberate security tradeoff that callers should choose explicitly.

## [0.3.1] - 2026-03-19

### New Features
- Added real-time session event subscription via `SessionRecorder::subscribe() -> tokio::sync::broadcast::Receiver<SessionRecordEntry>`, so callers can consume transaction/workflow events while execution is still in progress.
- Added recorder tests covering live event delivery and `SessionRecordLevel::Off` behavior for real-time subscribers.

### Optimizations
- Updated `SessionRecorder::record_event(...)` to fan out each recorded entry to subscribers while keeping the existing in-memory snapshot and JSONL export workflow intact.
- Expanded README and Chinese README recording examples to show how to subscribe to live recorder events before starting command or workflow execution.

### API Changes
- `SessionRecorder` now exposes a new public method: `subscribe()`.
- Real-time consumers now receive the existing `SessionRecordEntry` / `SessionEvent` model directly; no parallel event type was introduced, so upper layers can reuse current event conversion logic.

### Risks
- `subscribe()` uses a Tokio broadcast channel; slow consumers can observe `RecvError::Lagged(...)` if they fall behind a busy session and should handle that explicitly.
- Real-time subscription only streams future events after subscription creation; historical events still need to be read from `entries()` / `to_jsonl()`.
- The `rauto` integration sample pattern still needs to rebuild `ConnectionRequest` (or wrap it in a helper) between setup and execution calls because manager APIs consume requests by value.

---

## [0.3.0] - 2026-03-14

### New Features
- Added structured manager request/context APIs:
  - `ConnectionRequest`
  - `ExecutionContext`
  - `SshConnectionManager::get_with_context(...)`
  - `SshConnectionManager::execute_tx_block_with_context(...)`
  - `SshConnectionManager::execute_tx_workflow_with_context(...)`
  - `SshConnectionManager::get_with_recording_and_context(...)`
  - `SshConnectionManager::get_with_recording_level_and_context(...)`
- Added client-layer transaction execution tests that validate rollback behavior without requiring a real SSH session.

### Optimizations
- Refactored transaction execution in `src/session/client.rs` around an internal command runner abstraction, making rollback sequencing easier to test and maintain.
- Updated library docs, README examples, and the firewall workflow example to use the structured request/context API consistently.
- Improved workflow dry-run output to expose step-level `rollback_on_failure` behavior in the example printer.

### API Changes
- Removed the old high-parameter manager entrypoints:
  - `SshConnectionManager::get(...)`
  - `SshConnectionManager::get_with_security(...)`
  - `SshConnectionManager::get_with_recording(...)`
  - `SshConnectionManager::get_with_recording_level(...)`
  - `SshConnectionManager::execute_tx_block(...)`
  - `SshConnectionManager::execute_tx_workflow(...)`
- Callers must now build `ConnectionRequest` and pass `ExecutionContext` to manager entrypoints.
- Public examples and migration path now assume `RollbackPolicy::WholeResource { trigger_step_index, ... }` and `TxStep { rollback_on_failure, ... }`.

### Risks
- This is a breaking API release for callers still using the removed positional-argument manager methods; all such integrations must migrate before upgrading.
- The new client-layer execution tests use an internal fake runner and improve behavioral coverage, but they do not replace real-device compatibility testing.
- Downstream wrappers that mirrored the previous manager method signatures may need their own facade refactor to avoid leaking the old shape.

---

## [0.2.2] - 2026-02-21

### New Features
- Added per-step rollback control flag `TxStep.rollback_on_failure` (default `false`), allowing a failed step to optionally run its own rollback command.
- Added whole-resource rollback trigger control `RollbackPolicy::WholeResource { trigger_step_index }`, so whole-block rollback runs only after the configured step has executed successfully (default trigger is step `0`).

### Optimizations
- Improved per-step rollback planning to skip steps without rollback commands instead of rejecting the block.
- Improved transaction rollback reporting: when no rollback plan is generated, results now record explicit "rollback not attempted" reasons instead of ambiguous success semantics.

### API Changes
- `TxStep` now includes `rollback_on_failure: bool` (serde default `false`).
- `RollbackPolicy::WholeResource` now includes `trigger_step_index: usize` (serde default `0`).
- `TxBlock::plan_rollback(...)` now accepts `failed_step_index: Option<usize>` so planners can include failed-step rollback when enabled.

### Risks
- Existing code constructing `RollbackPolicy::WholeResource` directly must provide or accept the new trigger semantics; behavior now depends on trigger-step execution status.
- Tooling that assumed every `PerStep` command has a rollback command may need updates because rollback planning now permits and skips missing/empty rollback commands.
- Consumers parsing rollback status should handle explicit "not attempted" error messages in addition to command-failure errors.

---

## [0.2.1] - 2026-02-19

### New Features
- Added reusable transaction helper `failed_block_rollback_summary(...)` to derive workflow rollback state from the failed block execution result.
- Added regression tests for failed-block rollback state propagation and default fallback behavior in transaction workflow summaries.

### Optimizations
- Fixed workflow rollback aggregation so failed block rollback errors are merged into `TxWorkflowResult.rollback_errors`.
- Corrected workflow rollback metadata reporting (`rollback_attempted`, `rollback_succeeded`) to reflect actual rollback paths instead of unconditional success defaults.

### API Changes
- Re-exported `failed_block_rollback_summary` from `session` transaction public exports.
- `TxWorkflowResult` rollback status semantics are now stricter: failed block internal rollback outcome is included before committed-block compensation rollback runs.

### Risks
- Integrations that assumed previous optimistic rollback summary behavior may see changed failure/attempt flags and need assertion updates.
- Current coverage for this fix is unit-level; end-to-end device rollback behavior still depends on command/device-specific rollback correctness.

---

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
