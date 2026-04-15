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
        .await?;
        debug!("{} TCP connection successful", device_addr);

        let mut channel = client.get_channel().await?;
        channel
            .request_pty(false, "xterm", 800, 600, 0, 0, &[])
            .await?;
        channel.request_shell(false).await?;
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
                        if terminal_fragment_has_pua(&line) {
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
                    return Err(ConnectError::ChannelDisconnectError);
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
        if let Some(session_recorder) = recorder.as_ref() {
            let _ = session_recorder.record_event(SessionEvent::ConnectionEstablished {
                device_addr: device_addr.clone(),
                prompt_after: prompt.clone(),
                fsm_prompt_after: handler.current_state().to_string(),
            });
        }

        Ok(Self {
            client,
            sender: sender_to_shell,
            recv: receiver_from_shell,
            handler,
            prompt,
            password_hash,
            enable_password_hash,
            security_options,
            recorder,
        })
    }

    /// Checks if the underlying SSH connection is still active.
    pub fn is_connected(&self) -> bool {
        !self.client.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::build_init_timeout_message;
    use crate::device::normalize_terminal_output;

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
}
