//! In-process fake SSH devices for testing rneter-based automation.
//!
//! Enabled with the `testkit` cargo feature. Downstream crates that build on
//! `rneter` can spin up a scripted SSH server that impersonates any built-in
//! device template (or a custom [`DeviceHandlerConfig`]) and run their
//! automation logic against it — no real hardware, no network, no mocks of
//! `rneter` itself. The full stack is exercised: SSH handshake, prompt
//! detection, state transitions, lifecycle hooks, recording, and
//! transactions. [`FaultInjection`] adds deterministic authentication and
//! command delays, flaky authentication, and command-triggered disconnects.
//!
//! The fake device derives its state machine directly from the same
//! [`DeviceHandlerConfig`] the client template uses, so template changes can
//! never silently diverge from the simulation. A [`DevicePersona`] supplies
//! only what a regex cannot: concrete prompt strings, interactive challenges
//! (enable/sudo passwords), and vendor-styled error text.
//!
//! ```no_run
//! use rneter::session::{Command, ExecutionContext, SshConnectionManager};
//! use rneter::testkit::{DevicePersona, FakeSshDevice};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let device = FakeSshDevice::spawn(DevicePersona::builtin("cisco_ios")?).await?;
//! let manager = SshConnectionManager::new();
//! let output = manager
//!     .execute_command_with_context(
//!         device.connection_request()?,
//!         Command {
//!             mode: "Enable".to_string(),
//!             command: "show version".to_string(),
//!             ..Command::default()
//!         },
//!         device.execution_context(),
//!     )
//!     .await?;
//! assert!(output.success);
//! assert!(device.received_commands().contains(&"show version".to_string()));
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_ssh2_tokio::ServerCheckMethod;
use rand_core::OsRng;
use russh::keys::{Algorithm, PrivateKey, PublicKey};
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId, CryptoVec};

use crate::device::{DeviceCommandExecutionConfig, DeviceHandlerConfig, EXIT_STATUS_SUFFIX};
use crate::error::ConnectError;
use crate::session::{
    ConnectionRequest, ConnectionSecurityOptions, DetectRequest, ExecutionContext, SecurityLevel,
    SshAuthMethod,
};
use crate::templates;

mod personas;

/// Default login username accepted by fake devices.
pub const DEFAULT_USERNAME: &str = "admin";
/// Default login password accepted by fake devices.
pub const DEFAULT_PASSWORD: &str = "testkit-login-pw";
/// Default enable/sudo password expected by fake device challenges.
pub const DEFAULT_ENABLE_PASSWORD: &str = "testkit-enable-pw";
/// Command every fake device answers with vendor-styled error output.
pub const ERROR_COMMAND: &str = "make-error";

/// Interactive challenge a fake device issues before completing a command.
#[derive(Debug, Clone)]
pub struct PersonaChallenge {
    /// Command that triggers the challenge (e.g. `enable`, `sudo -i`).
    pub command: String,
    /// Challenge text sent to the client, without a trailing newline.
    pub prompt: String,
    /// Response the device expects (without newline).
    pub response: String,
}

/// Deterministic failures and delays injected by a fake device.
///
/// Attempt budgets are shared by every connection to the device, so a new
/// SSH connection does not reset a flaky-authentication or dropped-command
/// scenario.
#[derive(Debug, Clone, Default)]
pub struct FaultInjection {
    /// Delay applied before each logical authentication attempt.
    pub auth_delay: Duration,
    /// Number of otherwise valid authentication attempts to reject.
    pub reject_auth_attempts: usize,
    /// Delay applied before responding to each complete command line.
    pub command_delay: Duration,
    /// Exact logical commands to disconnect on, mapped to remaining attempts.
    pub disconnect_commands: HashMap<String, usize>,
}

impl FaultInjection {
    /// Creates a fault configuration with no active faults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Delays each logical authentication attempt by `duration`.
    pub fn with_auth_delay(mut self, duration: Duration) -> Self {
        self.auth_delay = duration;
        self
    }

    /// Rejects the first `attempts` otherwise valid authentication attempts.
    pub fn with_rejected_auth_attempts(mut self, attempts: usize) -> Self {
        self.reject_auth_attempts = attempts;
        self
    }

    /// Delays the response to every complete command line by `duration`.
    pub fn with_command_delay(mut self, duration: Duration) -> Self {
        self.command_delay = duration;
        self
    }

    /// Closes the SSH shell channel the first `times` this exact command is received.
    pub fn with_disconnect_command(mut self, command: impl Into<String>, times: usize) -> Self {
        self.disconnect_commands.insert(command.into(), times);
        self
    }
}

/// Blueprint describing how a fake device impersonates one device type.
///
/// The state machine (states, transition commands, exit-status strategy) is
/// derived from `config`; the persona adds the concrete artifacts a regex
/// cannot produce: prompt strings per state, challenges, and error text.
#[derive(Debug, Clone)]
pub struct DevicePersona {
    /// Display name used in diagnostics (usually the template name).
    pub name: String,
    /// Handler configuration this persona simulates.
    pub config: DeviceHandlerConfig,
    /// Lowercased state presented right after login.
    pub initial_state: String,
    /// Lowercased state name to concrete prompt string (e.g. `fake(config)#`).
    pub prompts: HashMap<String, String>,
    /// Interactive challenges keyed by triggering command.
    pub challenges: Vec<PersonaChallenge>,
    /// Vendor-styled error line; must match one of `config.error_regex`.
    pub error_reply: String,
    /// Canned output returned for any unrecognized command; must NOT match
    /// any of `config.error_regex`.
    pub benign_reply: String,
    /// Realistic command replies: exact command text to the multi-line
    /// output the real device would print (e.g. `show version`).
    pub canned_replies: Vec<(String, String)>,
    /// Login username the device accepts.
    pub username: String,
    /// Login password the device accepts.
    pub password: String,
    /// Enable/sudo password used by challenge-based personas.
    pub enable_password: Option<String>,
    /// Authorized OpenSSH public keys accepted for public-key login.
    pub authorized_public_keys: Vec<String>,
    /// Optional keyboard-interactive challenge `(prompt, expected response)`.
    pub keyboard_interactive: Option<(String, String)>,
    /// Deterministic delays and failures applied by the virtual device.
    pub faults: FaultInjection,
}

impl DevicePersona {
    /// Creates a persona for a custom handler configuration.
    ///
    /// `prompts` maps each prompt state (case-insensitive) to the concrete
    /// prompt string the device shows in that state; the first entry's state
    /// is not special — `initial_state` selects the post-login state.
    pub fn for_config(
        name: impl Into<String>,
        config: DeviceHandlerConfig,
        initial_state: impl Into<String>,
        prompts: &[(&str, &str)],
    ) -> Self {
        Self {
            name: name.into(),
            config,
            initial_state: initial_state.into().to_ascii_lowercase(),
            prompts: prompts
                .iter()
                .map(|(state, prompt)| (state.to_ascii_lowercase(), (*prompt).to_string()))
                .collect(),
            challenges: Vec::new(),
            error_reply: "ERROR: forced failure".to_string(),
            benign_reply: "testkit-ok sample output".to_string(),
            canned_replies: Vec::new(),
            username: DEFAULT_USERNAME.to_string(),
            password: DEFAULT_PASSWORD.to_string(),
            enable_password: None,
            authorized_public_keys: Vec::new(),
            keyboard_interactive: None,
            faults: FaultInjection::default(),
        }
    }

    /// Adds a realistic reply for one exact command, imitating what the
    /// real device would print (multi-line output supported).
    pub fn with_canned_reply(
        mut self,
        command: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        self.canned_replies.push((command.into(), output.into()));
        self
    }

    /// Adds an interactive challenge (e.g. an enable-password prompt).
    pub fn with_challenge(
        mut self,
        command: impl Into<String>,
        prompt: impl Into<String>,
        response: impl Into<String>,
    ) -> Self {
        self.challenges.push(PersonaChallenge {
            command: command.into(),
            prompt: prompt.into(),
            response: response.into(),
        });
        self
    }

    /// Overrides the vendor-styled error line.
    pub fn with_error_reply(mut self, error_reply: impl Into<String>) -> Self {
        self.error_reply = error_reply.into();
        self
    }

    /// Creates the ready-made persona for a built-in template name.
    ///
    /// Accepts the same names as [`templates::by_name`], including aliases.
    /// Personas imitate the real device: hostname-styled prompts, realistic
    /// command replies, password challenges, and vendor-styled errors. They
    /// are defined one module per vendor under `personas`, mirroring
    /// `crate::templates`.
    pub fn builtin(template: &str) -> Result<Self, ConnectError> {
        personas::builtin(template)
    }

    /// Sets the enable/sudo password handed to connections against this
    /// persona (and expected by its challenges).
    pub fn with_enable_password(mut self, enable_password: impl Into<String>) -> Self {
        self.enable_password = Some(enable_password.into());
        self
    }

    /// Authorizes an OpenSSH public key for public-key authentication.
    ///
    /// `public_key` may be a full `ssh-ed25519 AAAA... comment` line or just
    /// the base64 key body.
    pub fn with_authorized_public_key(mut self, public_key: impl Into<String>) -> Self {
        self.authorized_public_keys.push(public_key.into());
        self
    }

    /// Configures keyboard-interactive authentication.
    ///
    /// On first challenge the device sends `prompt`; the client must answer
    /// with `response` (matched by exact equality).
    pub fn with_keyboard_interactive(
        mut self,
        prompt: impl Into<String>,
        response: impl Into<String>,
    ) -> Self {
        self.keyboard_interactive = Some((prompt.into(), response.into()));
        self
    }

    /// Applies deterministic delays and failures to this persona.
    pub fn with_faults(mut self, faults: FaultInjection) -> Self {
        self.faults = faults;
        self
    }
}

/// Ready-made personas for every built-in template.
pub fn builtin_personas() -> Result<Vec<DevicePersona>, ConnectError> {
    templates::available_templates()
        .iter()
        .map(|name| DevicePersona::builtin(name))
        .collect()
}

/// One transition edge of the simulated state machine.
#[derive(Debug, Clone)]
struct EdgeSpec {
    command: String,
    target: String,
    needs_format: bool,
}

fn parse_authorized_public_key(key: &str) -> Result<PublicKey, String> {
    let trimmed = key.trim();
    if let Ok(public_key) = PublicKey::from_openssh(trimmed) {
        return Ok(public_key);
    }
    // Accept a bare base64 key body without the algorithm prefix.
    russh::keys::parse_public_key_base64(trimmed)
        .map_err(|error| format!("failed to parse public key: {error}"))
}

impl EdgeSpec {
    /// Matches the received command, substituting `{}` in format edges.
    fn matches(&self, received: &str) -> bool {
        if !self.needs_format {
            return self.command == received;
        }
        match self.command.split_once("{}") {
            Some((prefix, suffix)) => {
                received.len() > prefix.len() + suffix.len()
                    && received.starts_with(prefix)
                    && received.ends_with(suffix)
            }
            None => self.command == received,
        }
    }
}

/// Immutable behavior shared by every connection of one fake device.
#[derive(Debug)]
struct EngineSpec {
    prompts: HashMap<String, String>,
    edges: HashMap<String, Vec<EdgeSpec>>,
    challenges: HashMap<String, PersonaChallenge>,
    initial_state: String,
    error_reply: String,
    benign_reply: String,
    /// Realistic replies for exact commands (e.g. `show version`).
    canned: HashMap<String, String>,
    username: String,
    password: String,
    /// Parsed authorized public keys for public-key authentication.
    authorized_public_keys: Vec<PublicKey>,
    /// Keyboard-interactive challenge, if configured.
    keyboard_interactive: Option<(String, String)>,
    /// Marker used by shell exit-status templates (e.g. the Linux template).
    exit_marker: Option<String>,
    banner: String,
    auth_delay: Duration,
    command_delay: Duration,
    remaining_auth_rejections: AtomicUsize,
    remaining_command_disconnects: Mutex<HashMap<String, usize>>,
}

impl EngineSpec {
    fn from_persona(persona: &DevicePersona) -> Result<Self, ConnectError> {
        let mut edges: HashMap<String, Vec<EdgeSpec>> = HashMap::new();
        for rule in &persona.config.edges {
            edges
                .entry(rule.from_state.to_ascii_lowercase())
                .or_default()
                .push(EdgeSpec {
                    command: rule.command.clone(),
                    target: rule.to_state.to_ascii_lowercase(),
                    needs_format: rule.needs_format,
                });
        }
        let exit_marker = match &persona.config.command_execution {
            DeviceCommandExecutionConfig::ShellExitStatus { marker, .. } => Some(marker.clone()),
            DeviceCommandExecutionConfig::PromptDriven => None,
        };
        let mut authorized_public_keys = Vec::new();
        for key in &persona.authorized_public_keys {
            authorized_public_keys.push(parse_authorized_public_key(key).map_err(|error| {
                ConnectError::InvalidDeviceHandlerConfig(format!(
                    "persona '{}': invalid authorized public key: {error}",
                    persona.name
                ))
            })?);
        }
        Ok(Self {
            prompts: persona.prompts.clone(),
            edges,
            challenges: persona
                .challenges
                .iter()
                .map(|challenge| (challenge.command.clone(), challenge.clone()))
                .collect(),
            initial_state: persona.initial_state.clone(),
            error_reply: persona.error_reply.clone(),
            benign_reply: persona.benign_reply.clone(),
            canned: persona.canned_replies.iter().cloned().collect(),
            username: persona.username.clone(),
            password: persona.password.clone(),
            authorized_public_keys,
            keyboard_interactive: persona.keyboard_interactive.clone(),
            exit_marker,
            banner: format!("Welcome to fake {} device", persona.name),
            auth_delay: persona.faults.auth_delay,
            command_delay: persona.faults.command_delay,
            remaining_auth_rejections: AtomicUsize::new(persona.faults.reject_auth_attempts),
            remaining_command_disconnects: Mutex::new(persona.faults.disconnect_commands.clone()),
        })
    }

    fn prompt(&self, state: &str) -> &str {
        self.prompts
            .get(state)
            .map(String::as_str)
            .unwrap_or("testkit-missing-prompt>")
    }

    async fn accepts_auth(&self, credentials_valid: bool) -> bool {
        if !self.auth_delay.is_zero() {
            tokio::time::sleep(self.auth_delay).await;
        }
        credentials_valid
            && self
                .remaining_auth_rejections
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_err()
    }

    fn should_disconnect_command(&self, command: &str) -> bool {
        let mut remaining = self
            .remaining_command_disconnects
            .lock()
            .expect("fake device command fault state");
        let Some(attempts) = remaining.get_mut(command) else {
            return false;
        };
        if *attempts == 0 {
            return false;
        }
        *attempts -= 1;
        true
    }
}

/// Pending interactive challenge on one connection.
#[derive(Debug, Clone)]
struct PendingChallenge {
    response: String,
    target_state: Option<String>,
}

enum LineAction {
    Reply(String),
    Disconnect,
}

/// Per-connection scripted CLI session.
struct ScriptedHandler {
    spec: Arc<EngineSpec>,
    log: Arc<Mutex<Vec<String>>>,
    state: String,
    pending: Option<PendingChallenge>,
    line_buffer: String,
    /// Set after a `\r`-terminated line so a following `\n` (from a `\r\n`
    /// pair split across packets) is not treated as an extra empty command.
    skip_leading_lf: bool,
}

impl ScriptedHandler {
    fn new(spec: Arc<EngineSpec>, log: Arc<Mutex<Vec<String>>>) -> Self {
        let state = spec.initial_state.clone();
        Self {
            spec,
            log,
            state,
            pending: None,
            line_buffer: String::new(),
            skip_leading_lf: false,
        }
    }

    /// Takes the next complete line out of the buffer.
    ///
    /// Automation clients terminate lines with `\n`, while interactive SSH
    /// terminals in raw mode send a bare `\r` when Enter is pressed — both
    /// (and `\r\n`) must count as end-of-line for the device to be usable
    /// from a plain `ssh` client as well.
    fn take_line(&mut self) -> Option<String> {
        if self.skip_leading_lf {
            if self.line_buffer.is_empty() {
                return None;
            }
            if self.line_buffer.starts_with('\n') {
                self.line_buffer.remove(0);
            }
            self.skip_leading_lf = false;
        }
        let terminator_pos = self.line_buffer.find(['\n', '\r'])?;
        let terminator = self.line_buffer.as_bytes()[terminator_pos];
        let mut line: String = self.line_buffer.drain(..=terminator_pos).collect();
        line.pop();
        if terminator == b'\r' {
            self.skip_leading_lf = true;
        }
        Some(line)
    }

    /// Splits a shell exit-status wrapped line into its core command.
    fn unwrap_exit_status_command<'a>(&self, line: &'a str) -> (&'a str, bool) {
        if let Some(marker) = self.spec.exit_marker.as_deref()
            && line.contains(marker)
            && let Some(idx) = line.find("; printf '")
        {
            return (&line[..idx], true);
        }
        (line, false)
    }

    fn marker_line(&self, code: i32) -> String {
        let marker = self.spec.exit_marker.as_deref().unwrap_or_default();
        format!("{marker}{code}{EXIT_STATUS_SUFFIX}\r\n")
    }

    /// Processes one received line and returns the device action.
    fn handle_line(&mut self, raw_line: &str) -> LineAction {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let (core, wants_marker) = self.unwrap_exit_status_command(line);
        let core = core.to_string();
        self.log
            .lock()
            .expect("fake device command log")
            .push(core.clone());

        if self.spec.should_disconnect_command(&core) {
            return LineAction::Disconnect;
        }

        if let Some(pending) = self.pending.take() {
            return LineAction::Reply(if core == pending.response {
                if let Some(target) = pending.target_state {
                    self.state = target;
                }
                format!("\r\n{}", self.spec.prompt(&self.state))
            } else {
                format!(
                    "\r\n{}\r\n{}",
                    self.spec.error_reply,
                    self.spec.prompt(&self.state)
                )
            });
        }

        let echo = format!("{line}\r\n");

        if core.is_empty() {
            return LineAction::Reply(format!("\r\n{}", self.spec.prompt(&self.state)));
        }

        // Transition edges win over canned replies so mode switching stays
        // faithful to the template's state machine.
        let matched_edge = self
            .spec
            .edges
            .get(&self.state)
            .and_then(|edges| edges.iter().find(|edge| edge.matches(&core)))
            .cloned();
        if let Some(edge) = matched_edge {
            if let Some(challenge) = self.spec.challenges.get(&core) {
                self.pending = Some(PendingChallenge {
                    response: challenge.response.clone(),
                    target_state: Some(edge.target),
                });
                return LineAction::Reply(format!("{echo}{}", challenge.prompt));
            }
            self.state = edge.target;
            return LineAction::Reply(format!("{echo}{}", self.spec.prompt(&self.state)));
        }

        // Challenges not tied to an edge (e.g. save confirmations).
        if let Some(challenge) = self.spec.challenges.get(&core) {
            self.pending = Some(PendingChallenge {
                response: challenge.response.clone(),
                target_state: None,
            });
            return LineAction::Reply(format!("{echo}{}", challenge.prompt));
        }

        if core == ERROR_COMMAND {
            let marker = if wants_marker {
                self.marker_line(1)
            } else {
                String::new()
            };
            return LineAction::Reply(format!(
                "{echo}{}\r\n{marker}{}",
                self.spec.error_reply,
                self.spec.prompt(&self.state)
            ));
        }

        // Realistic vendor output for known commands (e.g. `show version`).
        if let Some(reply) = self.spec.canned.get(&core) {
            let marker = if wants_marker {
                self.marker_line(0)
            } else {
                String::new()
            };
            let body = reply.replace('\n', "\r\n");
            return LineAction::Reply(format!(
                "{echo}{body}\r\n{marker}{}",
                self.spec.prompt(&self.state)
            ));
        }

        let marker = if wants_marker {
            self.marker_line(0)
        } else {
            String::new()
        };
        LineAction::Reply(format!(
            "{echo}{}\r\n{marker}{}",
            self.spec.benign_reply,
            self.spec.prompt(&self.state)
        ))
    }
}

impl server::Handler for ScriptedHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        let credentials_valid = user == self.spec.username && password == self.spec.password;
        if self.spec.accepts_auth(credentials_valid).await {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        let credentials_valid = user == self.spec.username
            && self
                .spec
                .authorized_public_keys
                .iter()
                .any(|authorized| authorized == public_key);
        if self.spec.accepts_auth(credentials_valid).await {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        if user == self.spec.username
            && self
                .spec
                .authorized_public_keys
                .iter()
                .any(|authorized| authorized == public_key)
        {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_keyboard_interactive(
        &mut self,
        user: &str,
        _submethods: &str,
        response: Option<server::Response<'_>>,
    ) -> Result<Auth, Self::Error> {
        match response {
            None => {
                let credentials_valid =
                    user == self.spec.username && self.spec.keyboard_interactive.is_some();
                if !self.spec.accepts_auth(credentials_valid).await {
                    Ok(Auth::reject())
                } else {
                    let (prompt, _) = self
                        .spec
                        .keyboard_interactive
                        .as_ref()
                        .expect("validated keyboard-interactive challenge");
                    Ok(Auth::Partial {
                        name: std::borrow::Cow::Borrowed("rneter-testkit"),
                        instructions: std::borrow::Cow::Borrowed(""),
                        prompts: std::borrow::Cow::Owned(vec![(
                            std::borrow::Cow::Owned(prompt.clone()),
                            false,
                        )]),
                    })
                }
            }
            Some(mut answers) => {
                if user != self.spec.username {
                    return Ok(Auth::reject());
                }
                let Some((_, expected)) = self.spec.keyboard_interactive.as_ref() else {
                    return Ok(Auth::reject());
                };
                let Some(answer) = answers.next() else {
                    return Ok(Auth::reject());
                };
                let answer = String::from_utf8_lossy(&answer);
                if answer == expected.as_str() {
                    Ok(Auth::Accept)
                } else {
                    Ok(Auth::reject())
                }
            }
        }
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(
            channel,
            CryptoVec::from(format!(
                "{}\r\n{}",
                self.spec.banner,
                self.spec.prompt(&self.state)
            )),
        )?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.line_buffer.push_str(&String::from_utf8_lossy(data));
        while let Some(line) = self.take_line() {
            let action = self.handle_line(&line);
            if !self.spec.command_delay.is_zero() {
                tokio::time::sleep(self.spec.command_delay).await;
            }
            match action {
                LineAction::Reply(reply) => {
                    session.data(channel, CryptoVec::from(reply))?;
                }
                LineAction::Disconnect => {
                    session.eof(channel)?;
                    session.close(channel)?;
                    self.line_buffer.clear();
                    break;
                }
            }
        }
        Ok(())
    }
}

/// Handle to a running in-process fake SSH device.
///
/// The listener stops when this handle is dropped.
pub struct FakeSshDevice {
    addr: SocketAddr,
    persona: DevicePersona,
    log: Arc<Mutex<Vec<String>>>,
    accept_task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeSshDevice {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

impl FakeSshDevice {
    /// Starts a fake device on an ephemeral local port.
    ///
    /// Validates that every prompt state of the persona's config has a
    /// concrete prompt and that each prompt string resolves to its intended
    /// state through the real prompt-matching engine, so personas can never
    /// drift from the template they simulate.
    pub async fn spawn(persona: DevicePersona) -> Result<Self, ConnectError> {
        Self::spawn_on(persona, ("127.0.0.1", 0)).await
    }

    /// Starts a fake device on a specific bind address.
    ///
    /// Use this to run a virtual device on a well-known port so external
    /// processes (or a plain `ssh` client) can connect to it; `spawn` is the
    /// ephemeral-port variant for in-process tests.
    pub async fn spawn_on(
        persona: DevicePersona,
        bind_addr: impl tokio::net::ToSocketAddrs,
    ) -> Result<Self, ConnectError> {
        Self::validate_persona(&persona)?;

        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|error| {
                ConnectError::InternalServerError(format!("bind fake device listener: {error}"))
            })?;
        let addr = listener.local_addr().map_err(|error| {
            ConnectError::InternalServerError(format!("fake device local addr: {error}"))
        })?;

        let host_key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(|error| {
            ConnectError::InternalServerError(format!("generate fake host key: {error}"))
        })?;
        let config = Arc::new(server::Config {
            keys: vec![host_key],
            auth_rejection_time: Duration::from_millis(10),
            ..Default::default()
        });

        let spec = Arc::new(EngineSpec::from_persona(&persona)?);
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let accept_log = log.clone();
        let accept_task = tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    break;
                };
                let handler = ScriptedHandler::new(spec.clone(), accept_log.clone());
                let config = config.clone();
                tokio::spawn(async move {
                    if let Ok(session) = server::run_stream(config, stream, handler).await {
                        let _ = session.await;
                    }
                });
            }
        });

        Ok(Self {
            addr,
            persona,
            log,
            accept_task,
        })
    }

    fn validate_persona(persona: &DevicePersona) -> Result<(), ConnectError> {
        if !persona.prompts.contains_key(&persona.initial_state) {
            return Err(ConnectError::InvalidDeviceHandlerConfig(format!(
                "persona '{}': initial state '{}' has no concrete prompt",
                persona.name, persona.initial_state
            )));
        }
        for rule in &persona.config.prompt {
            let state = rule.state.to_ascii_lowercase();
            if !persona.prompts.contains_key(&state) {
                return Err(ConnectError::InvalidDeviceHandlerConfig(format!(
                    "persona '{}': prompt state '{state}' has no concrete prompt",
                    persona.name
                )));
            }
        }
        // Each concrete prompt must resolve to its intended state through
        // the real prompt-matching engine.
        for (state, prompt) in &persona.prompts {
            let mut handler = persona.config.build()?;
            handler.read(prompt);
            let resolved = handler.current_state().to_string();
            if &resolved != state {
                return Err(ConnectError::InvalidDeviceHandlerConfig(format!(
                    "persona '{}': prompt '{prompt}' resolves to state '{resolved}', expected '{state}'",
                    persona.name
                )));
            }
        }
        Ok(())
    }

    /// The address the fake device listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The ephemeral port the fake device listens on.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The persona this device impersonates.
    pub fn persona(&self) -> &DevicePersona {
        &self.persona
    }

    /// Snapshot of every command line the device has received, in order.
    ///
    /// Shell exit-status wrappers are stripped, so entries are the logical
    /// commands the automation sent.
    pub fn received_commands(&self) -> Vec<String> {
        self.log.lock().expect("fake device command log").clone()
    }

    /// Builds an autodetect request wired to this device and its persona.
    pub fn detect_request(&self) -> DetectRequest {
        DetectRequest::new(
            self.persona.username.clone(),
            self.addr.ip().to_string(),
            self.addr.port(),
            self.persona.password.clone(),
        )
    }

    /// Builds a connection request wired to this device and its persona.
    ///
    /// Uses password authentication by default.
    pub fn connection_request(&self) -> Result<ConnectionRequest, ConnectError> {
        self.connection_request_with_auth(SshAuthMethod::password(self.persona.password.clone()))
    }

    /// Builds a connection request with an explicit authentication method.
    pub fn connection_request_with_auth(
        &self,
        auth: SshAuthMethod,
    ) -> Result<ConnectionRequest, ConnectError> {
        Ok(ConnectionRequest::new_with_auth(
            self.persona.username.clone(),
            self.addr.ip().to_string(),
            self.addr.port(),
            auth,
            self.persona.enable_password.clone(),
            self.persona.config.build()?,
        ))
    }

    /// Execution context suitable for talking to this device.
    ///
    /// Host-key verification is disabled because the device generates a
    /// fresh throwaway key at spawn.
    pub fn execution_context(&self) -> ExecutionContext {
        ExecutionContext::new()
            .with_security_options(ConnectionSecurityOptions {
                level: SecurityLevel::Secure,
                server_check: ServerCheckMethod::NoCheck,
            })
            .with_connect_timeout_secs(15)
    }
}
