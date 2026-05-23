use super::super::*;
use crate::device::{
    merge_terminal_prompt_fragments, normalize_terminal_output, terminal_fragment_has_pua,
};

fn build_init_timeout_message(initial_output: &str) -> String {
    let normalized_output = normalize_terminal_output(initial_output);
    if normalized_output.trim().is_empty() {
        return "waiting for initial prompt".to_string();
    }
    normalized_output
}

fn should_run_hook_actions(in_hook: bool, actions: &[HookAction]) -> bool {
    !in_hook && !actions.is_empty()
}

fn should_propagate_hook_failure(policy: &HookFailurePolicy) -> bool {
    matches!(policy, HookFailurePolicy::Required)
}

fn hook_output_summary(output: &SessionOperationOutput) -> Option<String> {
    let summary = output
        .steps
        .iter()
        .filter_map(|step| {
            let trimmed = step.content.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}

impl SharedSshClient {
    /// Calculates SHA-256 hash of the password.
    fn calculate_password_hash(password: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        hasher.finalize().into()
    }

    /// Calculates SHA-256 hash of the enable password (if present).
    fn calculate_enable_password_hash(enable_password: &Option<String>) -> Option<[u8; 32]> {
        enable_password.as_ref().map(|pwd| {
            let mut hasher = Sha256::new();
            hasher.update(pwd.as_bytes());
            hasher.finalize().into()
        })
    }

    /// Checks if connection parameters match (used for cache validation).
    pub fn matches_connection_params(
        &self,
        password: &str,
        enable_password: &Option<String>,
        handler: &DeviceHandler,
        security_options: &ConnectionSecurityOptions,
    ) -> bool {
        let password_hash = Self::calculate_password_hash(password);
        if self.password_hash != password_hash {
            debug!("Password hash mismatch");
            return false;
        }

        let enable_password_hash = Self::calculate_enable_password_hash(enable_password);
        if self.enable_password_hash != enable_password_hash {
            debug!("Enable password hash mismatch");
            return false;
        }

        if !self.handler.is_equivalent(handler) {
            debug!("Device handler configuration mismatch");
            return false;
        }

        if &self.security_options != security_options {
            debug!("Security options mismatch");
            return false;
        }

        true
    }

    /// Safely closes the connection.
    pub async fn close(&mut self) -> Result<(), ConnectError> {
        debug!("Safely closing SSH connection...");

        let before_disconnect_hooks = self.hooks.before_disconnect.clone();
        if let Err(error) = self
            .run_hook_actions(
                HookTrigger::BeforeDisconnect,
                &before_disconnect_hooks,
                None,
            )
            .await
        {
            debug!("before_disconnect hook failure: {error}");
        }

        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::ConnectionClosed {
                reason: "client_close_called".to_string(),
                prompt_before: Some(self.prompt.clone()),
                fsm_prompt_before: Some(self.handler.current_state().to_string()),
            });
        }

        self.recv.close();

        if self.is_connected() {
            if let Err(e) = self.sender.send("exit\n".to_string()).await {
                debug!("Failed to send exit command: {:?}", e);
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        debug!("SSH connection safely closed");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new(
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        mut handler: DeviceHandler,
        security_options: ConnectionSecurityOptions,
        recorder: Option<SessionRecorder>,
    ) -> Result<SharedSshClient, ConnectError> {
        let device_addr = format!("{user}@{addr}:{port}");

        let config = Config {
            preferred: security_options.preferred(),
            inactivity_timeout: Some(Duration::from_secs(60)),
            ..Default::default()
        };

        let client = Client::connect_with_config(
            (addr, port),
            &user,
            AuthMethod::with_password(&password),
            security_options.server_check.clone(),
            config,
        )
        .await
        .map_err(|error| ConnectError::ssh2_stage("connect", device_addr.clone(), error))?;
        debug!("{} TCP connection successful", device_addr);

        let mut channel = client.get_channel().await.map_err(|error| {
            ConnectError::ssh2_stage("open_channel", device_addr.clone(), error)
        })?;
        channel
            .request_pty(false, "xterm", 800, 600, 0, 0, &[])
            .await
            .map_err(|error| {
                ConnectError::russh_stage("request_pty", device_addr.clone(), error)
            })?;
        channel.request_shell(false).await.map_err(|error| {
            ConnectError::russh_stage("request_shell", device_addr.clone(), error)
        })?;
        debug!("{} Shell request successful", device_addr);

        let (sender_to_shell, mut receiver_from_user) = mpsc::channel::<String>(256);
        let (sender_to_user, mut receiver_from_shell) = mpsc::channel::<String>(256);

        let io_task_device_addr = device_addr.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    data = receiver_from_user.recv() => {
                        match data {
                            Some(data) => {
                                if let Err(e) = channel.data(data.as_bytes()).await {
                                    debug!("{} Failed to send data to shell: {:?}", io_task_device_addr, e);
                                    break;
                                }
                            }
                            None => {
                                debug!("{} Shell input sender dropped. Closing task.", io_task_device_addr);
                                break;
                            }
                        }
                    },
                    msg = channel.wait() => {
                        match msg {
                            Some(msg) => match msg {
                                ChannelMsg::Data { ref data } => {
                                    if let Ok(s) = std::str::from_utf8(data)
                                        && sender_to_user.send(s.to_string()).await.is_err() {
                                            debug!("{} Shell output receiver dropped. Closing task.", io_task_device_addr);
                                            break;
                                        }
                                }
                                ChannelMsg::ExitStatus { exit_status } => {
                                    debug!("{} Shell exited with status code: {}", io_task_device_addr, exit_status);
                                    let _ = channel.eof().await;
                                    break;
                                }
                                ChannelMsg::Eof => {
                                    debug!("{} Shell sent EOF.", io_task_device_addr);
                                    break;
                                }
                                _ => {}
                            },
                            None => {
                                debug!("{} Shell channel closed. Closing task.", io_task_device_addr);
                                break;
                            }
                        }
                    }
                    else => {
                        debug!("{} All I/O branches disabled. Closing task.", io_task_device_addr);
                        break;
                    }
                }
            }
            let _ = MANAGER.cache.invalidate(&io_task_device_addr).await;
            debug!("{} SSH I/O task ended.", io_task_device_addr);
        });

        let mut buffer = String::new();
        let mut prompt = String::new();
        let mut initial_output = String::new();
        let mut pending_prompt_lines = Vec::new();

        let mut params = handler.dyn_param.clone();
        if let Some(enable) = enable_password.as_ref() {
            params.insert("EnablePassword".to_string(), format!("{}\n", enable));
            trace!(
                "Connection dynamic param injected: key='EnablePassword', source='connection.enable_password', raw_len={}",
                enable.len()
            );
        } else {
            trace!(
                "Connection dynamic param missing: key='EnablePassword' (connection.enable_password=None)"
            );
        }
        handler.dyn_param = params;

        let init_result = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(data) = receiver_from_shell.recv().await {
                    trace!("{:?}", data);
                    buffer.push_str(&data);
                    initial_output.push_str(&data);

                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer.drain(..=newline_pos).collect::<String>();
                        if terminal_fragment_has_pua(&line) || handler.read_prompt_prefix(&line) {
                            pending_prompt_lines.push(line);
                            continue;
                        }

                        for pending_line in pending_prompt_lines.drain(..) {
                            let trimmed_pending = pending_line.trim_end();
                            handler.read(trimmed_pending);
                        }

                        let trimmed_line = line.trim_end();
                        handler.read(trimmed_line);
                    }

                    if let Some(prompt_candidate) =
                        merge_terminal_prompt_fragments(&pending_prompt_lines, Some(&buffer))
                        && handler.read_prompt(&prompt_candidate)
                    {
                        handler.read(&prompt_candidate);
                        prompt.clear();
                        prompt.push_str(handler.current_prompt().unwrap_or(&prompt_candidate));
                        return Ok(());
                    }

                    if !pending_prompt_lines.is_empty()
                        && buffer.is_empty()
                        && let Some(prompt_candidate) =
                            merge_terminal_prompt_fragments(&pending_prompt_lines, None)
                        && handler.read_prompt(&prompt_candidate)
                    {
                        handler.read(&prompt_candidate);
                        prompt.clear();
                        prompt.push_str(handler.current_prompt().unwrap_or(&prompt_candidate));
                        return Ok(());
                    }

                    if !buffer.is_empty() {
                        if handler.read_prompt(&buffer) {
                            for pending_line in pending_prompt_lines.drain(..) {
                                let trimmed_pending = pending_line.trim_end();
                                handler.read(trimmed_pending);
                            }
                            handler.read(&buffer);
                            prompt.clear();
                            prompt.push_str(handler.current_prompt().unwrap_or(&buffer));
                            return Ok(());
                        }
                        if let Some((c, _)) = handler.read_need_write(&buffer) {
                            for pending_line in pending_prompt_lines.drain(..) {
                                let trimmed_pending = pending_line.trim_end();
                                handler.read(trimmed_pending);
                            }
                            handler.read(&buffer);
                            sender_to_shell.send(c).await?;
                        }
                    }
                } else {
                    return Err(ConnectError::channel_disconnect_stage(
                        "connection_wait_initial_prompt",
                        device_addr.clone(),
                    ));
                }
            }
        })
        .await;

        match init_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(err),
            Err(_) => {
                return Err(ConnectError::InitTimeout(build_init_timeout_message(
                    &initial_output,
                )));
            }
        }

        let password_hash = Self::calculate_password_hash(&password);
        let enable_password_hash = Self::calculate_enable_password_hash(&enable_password);
        let mut shared = Self {
            client,
            sender: sender_to_shell,
            recv: receiver_from_shell,
            device_addr: device_addr.clone(),
            hooks: handler.hooks().clone(),
            in_hook: false,
            handler,
            prompt,
            password_hash,
            enable_password_hash,
            security_options,
            recorder,
        };

        if let Some(session_recorder) = shared.recorder.as_ref() {
            let _ = session_recorder.record_event(SessionEvent::ConnectionEstablished {
                device_addr: device_addr.clone(),
                prompt_after: shared.prompt.clone(),
                fsm_prompt_after: shared.handler.current_state().to_string(),
            });
        }

        let after_connect_hooks = shared.hooks.after_connect.clone();
        shared
            .run_hook_actions(HookTrigger::AfterConnect, &after_connect_hooks, None)
            .await?;

        Ok(shared)
    }

    /// Checks if the underlying SSH connection is still active.
    pub fn is_connected(&self) -> bool {
        !self.client.is_closed()
    }

    pub(crate) async fn run_hook_actions(
        &mut self,
        trigger: HookTrigger<'_>,
        actions: &[HookAction],
        sys: Option<&String>,
    ) -> Result<(), ConnectError> {
        if !should_run_hook_actions(self.in_hook, actions) {
            return Ok(());
        }

        self.in_hook = true;
        let result = async {
            for action in actions {
                if let Err(error) = self.run_single_hook(trigger, action, sys).await {
                    if should_propagate_hook_failure(&action.failure_policy) {
                        return Err(error);
                    }
                    debug!(
                        "best-effort hook '{}' failed during {}: {}",
                        action.name,
                        trigger.label(),
                        error
                    );
                }
            }
            Ok(())
        }
        .await;
        self.in_hook = false;
        result
    }

    async fn run_single_hook(
        &mut self,
        trigger: HookTrigger<'_>,
        action: &HookAction,
        sys: Option<&String>,
    ) -> Result<(), ConnectError> {
        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::HookStarted {
                trigger: trigger.label().to_string(),
                hook_name: action.name.clone(),
                state: trigger.state().map(str::to_string),
            });
        }

        let output = Box::pin(self.execute_operation_detailed(&action.operation, sys))
            .await
            .map_err(|error| {
                let (error, _partial_output) = error.into_parts();
                let error = ConnectError::InternalServerError(format!(
                    "hook '{}' failed during {}: {}",
                    action.name,
                    trigger.label(),
                    error
                ));
                if let Some(recorder) = self.recorder.as_ref() {
                    let _ = recorder.record_event(SessionEvent::HookFailed {
                        trigger: trigger.label().to_string(),
                        hook_name: action.name.clone(),
                        state: trigger.state().map(str::to_string),
                        error: error.to_string(),
                    });
                }
                error
            })?;

        if !output.success {
            let error = ConnectError::InternalServerError(format!(
                "hook '{}' failed during {}: operation returned unsuccessful result",
                action.name,
                trigger.label()
            ));
            if let Some(recorder) = self.recorder.as_ref() {
                let _ = recorder.record_event(SessionEvent::HookFailed {
                    trigger: trigger.label().to_string(),
                    hook_name: action.name.clone(),
                    state: trigger.state().map(str::to_string),
                    error: error.to_string(),
                });
            }
            return Err(error);
        }

        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::HookSucceeded {
                trigger: trigger.label().to_string(),
                hook_name: action.name.clone(),
                state: trigger.state().map(str::to_string),
                output_summary: action
                    .record_output
                    .then(|| hook_output_summary(&output))
                    .flatten(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{build_init_timeout_message, should_run_hook_actions};
    use crate::device::normalize_terminal_output;
    use crate::session::{Command, HookAction, HookFailurePolicy, SessionOperation};

    fn sample_hook_action() -> HookAction {
        HookAction::new(
            "disable-paging",
            SessionOperation::from(Command {
                mode: "Enable".to_string(),
                command: "terminal length 0".to_string(),
                ..Command::default()
            }),
        )
    }

    #[test]
    fn normalize_initial_output_uses_shared_pua_placeholder_logic() {
        let raw = concat!(
            "Welcome\r\n",
            "\u{1b}[1m\u{1b}[7m%\u{1b}[27m\u{1b}[0m ",
            "\u{1b}[38;2;214;93;14m\u{1b}[0m ",
            "󰌽 adam@host ~ % ",
            "\u{1b}[?2004h"
        );

        let normalized = normalize_terminal_output(raw);
        assert_eq!(normalized, "Welcome\n% <PUA> <PUA> adam@host ~ % ");
    }

    #[test]
    fn init_timeout_message_reports_shared_sanitized_output() {
        let raw = concat!("Welcome\r\n", "", " adam-work  ~   10:38  ");

        let message = build_init_timeout_message(raw);
        assert_eq!(message, "Welcome\n<PUA> adam-work  ~   10:38  ");
    }

    #[test]
    fn hook_execution_requires_non_empty_actions_and_no_active_hook_scope() {
        let actions = vec![sample_hook_action()];

        assert!(should_run_hook_actions(false, &actions));
        assert!(!should_run_hook_actions(true, &actions));
        assert!(!should_run_hook_actions(false, &[]));
    }

    #[test]
    fn only_required_hooks_abort_the_parent_flow() {
        assert!(super::should_propagate_hook_failure(
            &HookFailurePolicy::Required
        ));
        assert!(!super::should_propagate_hook_failure(
            &HookFailurePolicy::BestEffort
        ));
    }
}
