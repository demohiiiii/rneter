# Roadmap

Planned direction for `rneter`. Items are ordered by priority within each
section; nothing here is a commitment, and feedback via issues is welcome.

## Near Term (current release cycle)

- [x] **Autodetect E2E on virtual devices** — run `autodetect_with_context`
  against every testkit virtual device and assert the top-ranked candidate
  is the matching template. The built-in detect probes (`show version`,
  `display version`, `get system status`, ...) already overlap with the
  personas' simulated commands, so this closes the last untested core
  subsystem (`templates/detect`) and doubles as a fidelity audit of the
  personas themselves.
- [ ] **Release 0.5.0** — ship the pending breaking changes
  (`#[non_exhaustive] ConnectError`, sys-required transitions now erroring
  instead of silently sending an empty command) together with the `testkit`
  feature, the connection-lifecycle hardening, and the security fixes.
  Update `CHANGELOG.md` as part of the release.

## Tier 1 — Adoption Gaps

- [x] **SSH authentication methods** — public-key, ssh-agent, and
  keyboard-interactive authentication via `SshAuthMethod`;
  `ConnectionRequest`/`DetectRequest` carry the method, and pooled
  connections fingerprint it so credential changes force a reconnect.
- [x] **Fleet execution API** — run one command/flow across many
  devices concurrently with a concurrency limit and per-device error
  isolation via `execute_on_fleet(targets, operation, FleetOptions)`, built
  on the existing pool and Tokio. Transaction/workflow fleet helpers can be
  added after retry and resume semantics are defined.
- [ ] **Structured output parsing** — first a lightweight named-capture
  regex table parser (`Output::parse_table(&spec)` returning rows), later
  evaluate a TextFSM/ntc-templates compatibility layer.

## Tier 2 — Reliability & Usability

- [ ] **Reconnect & retry policies** — backoff retry for transient failures
  and resumable flows after disconnects.
- [ ] **Testkit fault injection** — latency, dropped connections, slow
  banners, and flaky authentication on virtual devices; pairs with the
  reconnect/retry work to close the resilience test loop.
- [ ] **Playbook runner CLI** — `Command`/`TxWorkflow` already derive serde;
  a `rneter run playbook.yml` binary plus record/replay gives a
  no-device-rehearsal → real-execution workflow.
- [ ] **Config backup & diff helpers** — high-level fleet config collection
  and before/after diffing on top of session recording.
- [ ] **Observability** — `tracing` spans per command (device, mode,
  duration) and optional metrics hooks.

## Quality & Coverage Debt

- [ ] Save-confirmation interactions (huawei / hillstone / juniper Y-N
  input rules) are never exercised end-to-end; personas already support
  non-transition challenges, only scenarios are missing.
- [ ] Pager (`more_regex`) handling has no E2E coverage; virtual devices
  could simulate paged output.
- [ ] `autodetect_and_connect_*` is bound to the global `MANAGER`; custom
  manager users silently fall back to the global pool.
