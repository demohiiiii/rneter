# Changelog

All notable changes to this project are documented in this file.

## [0.5.0] - 2026-08-08

### New Features

- Added the optional `testkit` feature with in-process SSH devices for every built-in template, including authentication, interactive confirmations, paged output, deterministic fault injection, and a runnable virtual-device example.
- Added `SshAuthMethod` support for passwords, inline or file-backed private keys, SSH agents, and keyboard-interactive authentication, with authentication-aware connection-pool fingerprints.
- Added bounded fleet execution through `FleetTarget`, `FleetOptions`, `FleetExecutionResult`, and `SshConnectionManager::execute_on_fleet(...)`, preserving input order while isolating per-device failures and partial output.
- Added opt-in retry policies with capped exponential backoff for transient connection failures and command-flow resumption from the first unfinished step.
- Added comma- or pipe-separated command modes, allowing execution in the current permitted mode or transition toward the outermost reachable permitted mode, plus replay support for the resolved mode.
- Added LeadSec PowerV support and expanded H3C, Huawei, Check Point, Fortinet, and Hillstone prompt, context, interaction, and error coverage.

### Optimizations

- Hardened connection pooling with configurable capacity and idle timeouts, single-flight connection establishment, dead-session eviction, credential-change detection, idle maintenance, and graceful worker shutdown.
- Added manager-owned autodetect-and-connect methods so caller-created managers retain and reuse the selected connection instead of routing it through the global pool.
- Cached state-transition paths and improved terminal stream handling so carriage-return-delimited vendor errors are retained before the final prompt.
- Updated the SSH dependency stack and algorithm preference order, removed plaintext and unauthenticated legacy algorithms, and moved CI actions to Node 24-compatible releases.
- Expanded end-to-end coverage across built-in templates, authentication methods, connection-pool lifecycle, recording isolation, retry recovery, fleet execution, paging, and interactive confirmations.

### API Changes

- `DetectRequest.password` and `ConnectionRequest.password` were replaced by `auth: SshAuthMethod`. Existing password callers can keep using `new(...)`; struct-literal callers must migrate to `auth` or `new_with_auth(...)`.
- Added `RetryPolicy` and `ExecutionContext::with_retry_policy(...)`; `ExecutionContext` struct literals must now initialize `retry_policy` or use its constructors and builders.
- Added `ConnectionPoolConfig`, `SshConnectionManager::with_pool_config(...)`, fleet execution types, manager-owned autodetect methods, recorder-aware execution entrypoints, and public transient/authentication error classification helpers.
- Marked `ConnectError` as `#[non_exhaustive]` and added authentication, fleet, retry, and shared-connection error variants. Downstream exhaustive matches must include a wildcard arm.
- Added the `testkit` Cargo feature and raised the declared minimum supported Rust version to 1.88.
- `Command.mode` now accepts one mode or a comma-/pipe-separated set such as `"user,root"` or `"login|config"`; single-mode values retain their previous behavior.

### Risks

- Retries have at-least-once semantics: if a device applies a command and disconnects before returning its prompt, retrying can apply the remote side effect again. Keep retries disabled for operations that are not idempotent unless duplication is acceptable.
- The legacy-compatible SSH security profile intentionally skips `known_hosts` verification. Use the secure or balanced profile when server identity verification is required.
- The transitive RSA implementation remains covered by `RUSTSEC-2023-0071` with no fixed upstream release. In-process RSA private keys are rejected by default but can be enabled through explicit advisory-aware constructors; prefer Ed25519, ECDSA, or an SSH agent.
- Public struct field changes to connection, detection, and execution-context models require migration for callers that construct those structs directly.
- Virtual-device tests model known prompts and workflows but do not replace validation against customized prompts, terminal behavior, and production firmware.

## [0.4.7] - 2026-07-23

### New Features
- Added configurable SSH connection-establishment timeouts through `ExecutionContext::with_connect_timeout(...)` and `ExecutionContext::with_connect_timeout_secs(...)`, with a 60-second default and target-aware `ConnectError::ConnectTimeout` failures.
- Added multiline command execution through `MultilineMode`, `Command::into_flow(...)`, `CommandFlow::expand_multiline(...)`, and `SshConnectionManager::execute_multiline_command_with_context(...)`; split-lines mode preserves one child output per concrete command.
- Preserved the complete device transcript in `Output.all` and command-output recorder events, including echoed commands and syntax-error pointer context, while keeping cleaned parsing in `Output.content`.

### Optimizations
- Simplified command execution around concrete commands and linear command flows, applying multiline expansion consistently across direct operations, command flows, transactions, rollbacks, and legacy command jobs.
- Improved Cisco-like prompt matching by making enable and configuration prompt patterns mutually exclusive and removing redundant carriage-return matching.
- Reduced Linux template surface area by consolidating handler construction around declarative `DeviceHandlerConfig` and the shared command execution path.

### API Changes
- Added `Command.multiline_mode` with `MultilineMode::SplitLines` as the default and `MultilineMode::Whole` for callers that must preserve newline-separated text as one device command. Direct single-command entrypoints now reject commands that expand to multiple concrete steps; callers should use `execute_multiline_command_with_context(...)` for those commands.
- Changed `templates::build_tx_block(...)` to accept an explicit `RollbackPolicy` and removed template-name and inferred-rollback parameters. Callers must provide the intended rollback policy directly.
- Removed command-flow template APIs (`CommandFlowTemplate*`, `templates::cisco_like_copy_template()`, and `SessionOperation::Template`); callers must construct concrete `Command` or `CommandFlow` values and interaction rules.
- Removed legacy Linux configuration and command-classification exports (`LinuxTemplateConfig`, `CustomPrompts`, `SudoMode`, `LinuxCommandType`, `classify_linux_command(...)`, and `linux_with_config(...)`), leaving `linux()` and `linux_handler_config()` as the supported Linux template entrypoints. `CommandDynamicParams::sudo_password` is also no longer available.
- Added `ConnectError::ConnectTimeout` and the `ExecutionContext::connect_timeout` field; exhaustive `ConnectError` matches and serialized context consumers must account for the new timeout behavior.

### Risks
- This release contains breaking API removals and signature changes. Integrations using command-flow templates, inferred transaction rollback, legacy Linux configuration types, or `sudo_password` must migrate before upgrading.
- Newline-separated commands now split into independent commands by default, which can change device behavior for callers that previously relied on whole-text execution; use `MultilineMode::Whole` explicitly when required.
- `Output.all` and failure recorder events now retain raw device data, including command echoes, terminal control sequences, and potentially sensitive command text; downstream log storage and redaction policies should be reviewed.
- Cisco prompt matching, multiline behavior, and raw transcript capture are covered by unit and integration tests, but validation against customized prompts, terminal emulators, and diverse device firmware remains a deployment risk.

## [0.4.6] - 2026-06-04

### New Features
- Added caller-supplied autodetect templates through `DetectTemplateDefinition`, allowing custom handler configs and detect profiles to participate in SSH autodetection and connection selection.
- Added autodetect helpers for custom and merged template sets, including `autodetect_with_templates_and_context(...)`, `autodetect_with_builtin_and_templates_and_context(...)`, `autodetect_and_connect_with_templates_and_context(...)`, and `merge_with_builtin_detect_templates(...)`.
- Added staged SSH error variants for connection and autodetect flows, surfacing the failing stage and target for SSH connect, channel, PTY, shell, probe send, pager continuation, and disconnect failures.

### Optimizations
- Aligned built-in template identifiers and metadata with Netmiko/ntc-style names such as `cisco_ios`, `cisco_xe`, `juniper_junos`, `arista_eos`, `paloalto_panos`, and `checkpoint_gaia`, while preserving legacy aliases for lookup.
- Split several broad detect profiles into more specific built-in profiles, including separate Cisco IOS/IOS-XE and H3C/HP Comware matches, improving ranked autodetect candidates.
- Added repository quality gates with a pre-commit hook for `cargo fmt` and warning-denying `cargo clippy`, plus GitHub Actions CI for `cargo clippy -- -D warnings` and `cargo test`.

### API Changes
- Public autodetect exports now include `DetectTemplateDefinition`, `collect_detect_snapshot_with_context(...)`, `autodetect_with_profiles_and_context(...)`, `autodetect_with_templates_and_context(...)`, `autodetect_with_builtin_and_templates_and_context(...)`, `autodetect_and_connect_with_templates_and_context(...)`, `autodetect_and_connect_with_builtin_and_templates_and_context(...)`, `builtin_detect_template_definitions()`, and `merge_with_builtin_detect_templates(...)`.
- `available_templates()`, `template_catalog()`, and autodetect reports now prefer canonical template names such as `cisco_ios`, `h3c_comware`, `hillstone_stoneos`, `ruijie_os`, and `checkpoint_gaia`; callers comparing returned names should migrate from legacy strings to canonical names.
- `by_name_config(...)` and `template_metadata(...)` continue to resolve legacy names such as `cisco`, `juniper`, `arista`, `paloalto`, `ruijie`, and `checkpoint`, so existing lookup callers can upgrade without changing inputs.
- `ConnectError` now includes staged variants (`Ssh2StageError`, `RusshStageError`, and `ChannelDisconnectStageError`); exhaustive matches over `ConnectError` must add arms for these variants.

### Risks
- Canonical template names may affect integrations that persist or compare names returned by catalogs or autodetect results, even though legacy lookup aliases remain supported.
- Custom autodetect templates can override built-in definitions by name, so callers should ensure custom handler configs and detect profiles are kept in sync to avoid selecting an incompatible handler.
- Staged SSH errors improve diagnostics but add new enum variants, which is a source-compatible risk for downstream code using exhaustive `ConnectError` matching.

## [0.4.5] - 2026-05-10

### New Features
- Added declarative session lifecycle hooks (`after_connect`, `before_disconnect`, `after_enter_state`, `before_exit_state`) with recorder events, enabling built-in and template-level session preparation such as automatic paging disable commands.
- Added SSH-based template autodetect with ranked candidates, scored probe facts, confidence gating, and a direct `autodetect_and_connect_with_context(...)` entrypoint for built-in templates.
- Added first-batch Netmiko-derived network templates for Aruba AOS-CX, Cisco ASA, Cisco NX-OS, Dell OS10, Ruijie RGOS, and ZTE ZXROS, and registered them across template exports, registry lookup, metadata, and autodetect coverage.

### Optimizations
- Simplified command flows to a linear prompt-driven execution model by removing output-branch control paths and executing flow steps strictly in declaration order with bounded step-count safety checks.
- Improved autodetect observability and resilience with probe-level `debug`/`trace` logging, contextual timeout messages, automatic pager continuation for common `More` prompts, and stronger Hillstone/Maipu-adjacent prompt diagnostics.
- Refined built-in template defaults and session preparation behavior by aligning more vendor hooks and prompt ordering with proven Netmiko patterns, including Hillstone automatic paging disable and expanded template consistency tests.

### API Changes
- Removed command-flow branching APIs from the public session/template model: `CommandBranchTarget`, `CommandOutputBranchRule`, `CommandOutputBranchSource`, `Command.output_branches`, and `Command.output_fallback` are no longer available.
- `CommandFlowTemplateStep` now models only linear command attributes (`command`, `mode`, `timeout`, `prompts`), so existing flow templates that relied on branch targets must be rewritten as ordered step sequences.
- Added autodetect-facing public models and entrypoints including `DetectRequest`, `TemplateDetectReport`, `TemplateDetectCandidate`, `DetectConnectPolicy`, and `autodetect_and_connect_with_context(...)`; built-in network template surface also now includes distinct names such as `cisco_asa`, `cisco_nxos`, `aruba_aoscx`, `dell_os10`, `ruijie`, and `zte_zxros`.

### Risks
- This release contains a breaking command-flow model simplification; downstream code using branch-based flow control must migrate to linear prompt-driven sequences before upgrading.
- Autodetect still depends on heuristic prompt/probe matching and vendor-specific pagination behavior, so environments with customized prompts, banners, or CLI dialects may require additional template tuning.
- The newly added network templates and autodetect profiles are backed by unit coverage and Netmiko-derived behavior, but they still need broader device-side validation across real firmware variants before being treated as universally reliable.

## [0.4.4] - 2026-04-25

### New Features
- Added stream-level prompt-prefix buffering through `DeviceHandlerConfig.prompt_prefix` and `DeviceHandler::read_prompt_prefix(...)`, enabling multiline prompt handling for CLIs like JunOS `[edit]` + `user@host#`.
- Added shared prompt-prefix merge handling in both connection initialization and command execution loops, so prefix lines are joined with trailing prompt fragments before state matching.
- Added Juniper template coverage for `[edit]` context prompts with regression tests for merged prompt matching and prefix-line buffering.

### Optimizations
- Unified pending prompt-line handling so private-use (`<PUA>`) themed prompt fragments and vendor prompt-prefix fragments follow the same buffering and merge path.
- Reduced prompt-context leakage into `Output.content` by keeping detected prompt-prefix lines in prompt matching flow instead of treating them as normal command output.
- Extended handler equivalence checks to include prompt-prefix patterns, preventing cached-session reuse across mismatched prompt-prefix configurations.

### API Changes
- `DeviceHandlerConfig` now includes `prompt_prefix: Vec<String>` (with `serde(default)`), allowing templates and callers to declare prompt-context prefix regexes.
- `DeviceHandler` now exposes `read_prompt_prefix(&self, line: &str) -> bool` for runtime prompt-prefix detection.
- Built-in `juniper` template defaults now include optional `[edit]` context matching and a default `prompt_prefix` rule (`^\[edit\]\s*$`); custom JunOS templates should keep equivalent rules when using context prompts.

### Risks
- Overly broad prompt-prefix regexes can temporarily hold real output lines in pending buffers; custom templates should keep prefix patterns narrowly scoped.
- Output cleanup and prompt detection remain heuristic-driven for highly customized CLI themes, so edge prompts may still need template-specific regex adjustments.
- Coverage includes full test-suite validation, but device-side verification across mixed firmware prompt variants is still recommended before broad rollout.

## [0.4.3] - 2026-04-13

### New Features
- Added output-driven command-flow branching with `CommandOutputBranchRule`, `CommandOutputBranchSource` (`all`/`content`/`prompt`), and `CommandBranchTarget` (`next`/`stop_success`/`stop_failure`/`jump`), so flows can branch on device output instead of only caller-provided inputs.
- Added flow-level loop guards through `CommandFlow.max_steps` and runtime validation for branch configuration (`InvalidCommandFlow` on invalid regex/targets), improving safety for jump-based interactive workflows.
- Added simplified inline template ergonomics for command-flow templates (`CommandFlowTemplateText` as `{{var}}` text plus string-based step/prompt builders), reducing boilerplate when defining reusable interactive workflows.

### Optimizations
- Simplified built-in Cisco-like copy workflow modeling by switching `cisco_like_copy_template()` to a command-driven variable set (`command`, `server_addr`, `remote_path`, optional credentials), removing input-side conditional template trees.
- Unified template and runtime branching design around post-command output evaluation, so SCP/TFTP/HTTP-style wizard flows can be extended without introducing protocol-specific control paths in the core executor.
- Simplified transaction execution internals by removing block-kind branching and relying on `RollbackPolicy` directly for rollback planning and failure handling.

### API Changes
- `Command` now supports output-branch controls via `output_branches` and `output_fallback`; `CommandFlow` now includes optional `max_steps` for bounded branching loops.
- `TxBlock` no longer contains `kind`, and `CommandBlockKind` is removed from public session exports; rollback behavior is now fully driven by `RollbackPolicy` (`None`, `WholeResource`, `PerStep`).
- `SessionEvent::TxBlockStarted` no longer includes `block_kind`; JSONL consumers that deserialize transaction events must update to the new event shape.
- `templates::classify_command(...)` is no longer publicly exported; transaction strategy selection now occurs inside `templates::build_tx_block(...)`.
- Built-in `cisco_like_copy_template()` runtime inputs are now command-oriented (`command` + shared prompt vars) rather than direction/protocol-driven conditional rendering, so existing callers should migrate their runtime var payloads.

### Risks
- Branch-enabled command flows can still terminate early or loop unexpectedly if regex rules are misconfigured; callers should validate rule order and tune `max_steps` for long-running wizard flows.
- This release includes a breaking transaction API/model migration (`TxBlock.kind`, `CommandBlockKind`, and `TxBlockStarted.block_kind` removal), so downstream schema and serialization consumers require coordinated upgrades.
- Integrations using the previous copy-template variable contract (for example `protocol`/`direction`-style inputs) need runtime payload migration to avoid empty or mismatched interactive responses.

## [0.4.2] - 2026-04-09

### New Features
- Added normalized runtime output extraction for shell command execution, including explicit handling of ANSI/control-sequence redraw noise before building command content.
- Added resilient command-echo stripping for wrapped fish status-capture commands (for example `date; printf ... "$status"`), so `Output.content` returns the actual command result instead of terminal redraw text.
- Added regression coverage for noisy fish output parsing through `extract_command_content_strips_fish_wrapper_echo_and_prompt`, using a real-world fish redraw sample with exit-code wrapper output.

### Optimizations
- Normalized timeout error payloads by applying the same output sanitization path to `ConnectError::ExecTimeout`, improving readability when shells emit terminal control chatter.
- Unified command content extraction into dedicated helpers (`normalize_runtime_output`, `strip_sent_command_prefix`, `extract_command_content`) to reduce fish-specific parsing edge cases.
- Improved trailing prompt trimming by normalizing prompt text and stripping it from parsed command output when present.

### API Changes
- No public type signatures changed in `0.4.2`.
- `Output.content` formatting behavior for interactive shells is now more aggressive about removing command echoes, redraw artifacts, and trailing prompts; integrations that relied on raw echoed shell lines should switch to `Output.all`.
- `ConnectError::ExecTimeout` now returns normalized output text instead of raw terminal control sequences in many fish/interactive-terminal scenarios.

### Risks
- More aggressive normalization can hide terminal artifacts that some troubleshooting workflows previously relied on when reading `Output.content`; use `Output.all` for full raw context.
- Prompt and echo stripping is heuristic-based, so heavily customized shell prompts or wrapper commands may still need environment-specific validation.
- The fix is validated by unit and integration-oriented test coverage plus the reproduced fish sample, but unusual terminal emulator escape behaviors may require follow-up tuning.

## [0.4.1] - 2026-04-09

### New Features
- Added fish-focused prompt recovery that matches only the latest terminal fragment after carriage-return redraws, so command completion no longer stalls when prompts are prefixed by transient terminal noise.
- Added default Linux template prompt coverage for bracket-style fish prompts such as `[host] path>`, `[host] path#`, and `[host]#`.
- Added regression tests for fish prompt matching and runtime interactive input matching when ANSI escapes and carriage returns appear in the same buffer.

### Optimizations
- Extended simple escape stripping to remove `\x1b>` and `\x1b=` probes before prompt-state classification.
- Unified prompt parsing in `read`, `read_prompt`, `read_sys_prompt`, `read_need_write`, and runtime interaction matching to use the same final-fragment normalization path.
- Reduced false prompt mismatches by storing normalized prompt text from the matched terminal fragment instead of preserving earlier non-prompt buffer segments.

### API Changes
- No public API signatures changed in `0.4.1`.
- Prompt/state detection behavior now prioritizes the latest terminal fragment, which may change matching outcomes for custom prompt regexes that previously depended on full-buffer content.
- Built-in Linux prompt regex defaults now include broader fish prompt variants, reducing the need for custom Linux prompt overrides in common fish deployments.

### Risks
- Broader bracket-style Linux prompt patterns can increase accidental prompt matches on unusual command output lines that end with shell markers.
- Final-fragment prompt matching intentionally ignores earlier carriage-return redraw content, so highly customized multiline prompts may still require custom prompt regex configuration.
- Fish compatibility is validated through unit and integration-oriented test cases, but highly customized fish themes and terminal emulators may still need environment-specific regex tuning.

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
