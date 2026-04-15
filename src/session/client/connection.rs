use super::super::*;
use crate::device::{
    STRIP_CSI_ESCAPE, STRIP_DCS_ESCAPE, STRIP_OSC_ESCAPE, STRIP_SIMPLE_ESCAPE, is_private_use,
};

fn sanitize_initial_output_line(line: &str) -> String {
    sanitize_initial_output_line_impl(line, false)
}

fn sanitize_initial_output_line_with_pua_hints(line: &str) -> String {
    sanitize_initial_output_line_impl(line, true)
}

fn sanitize_initial_output_line_impl(line: &str, keep_pua_hints: bool) -> String {
    let without_osc = STRIP_OSC_ESCAPE.replace_all(line, "");
    let without_dcs = STRIP_DCS_ESCAPE.replace_all(without_osc.as_ref(), "");
    let without_csi = STRIP_CSI_ESCAPE.replace_all(without_dcs.as_ref(), "");
    let without_simple = STRIP_SIMPLE_ESCAPE.replace_all(without_csi.as_ref(), "");

    let mut sanitized = String::with_capacity(without_simple.len());
    let mut in_pua_run = false;

    for ch in without_simple.chars() {
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            continue;
        }
        if is_private_use(ch) {
            if keep_pua_hints && !in_pua_run {
                sanitized.push_str("<PUA>");
                in_pua_run = true;
            }
            continue;
        }
        in_pua_run = false;
        sanitized.push(ch);
    }

    sanitized
}

fn latest_terminal_fragment(line: &str) -> &str {
    line.rsplit(['\n', '\r'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(line)
}

#[cfg(test)]
fn normalize_initial_output(text: &str) -> String {
    normalize_initial_output_with(text, false)
}

fn normalize_initial_output_with_pua_hints(text: &str) -> String {
    normalize_initial_output_with(text, true)
}

fn normalize_initial_output_with(text: &str, keep_pua_hints: bool) -> String {
    let mut normalized = String::with_capacity(text.len());

    for chunk in text.split_inclusive('\n') {
        let has_newline = chunk.ends_with('\n');
        let body = if has_newline {
            &chunk[..chunk.len().saturating_sub(1)]
        } else {
            chunk
        };
        let sanitized = if keep_pua_hints {
            sanitize_initial_output_line_with_pua_hints(body)
        } else {
            sanitize_initial_output_line(body)
        };
        let visible = latest_terminal_fragment(&sanitized).trim_end_matches('\r');
        normalized.push_str(visible);
        if has_newline {
            normalized.push('\n');
        }
    }

    normalized
}

fn last_non_empty_line(text: &str) -> Option<&str> {
    text.lines().rev().find(|line| !line.trim().is_empty())
}

fn build_init_timeout_message(initial_output: &str) -> String {
    let signature_output = normalize_initial_output_with_pua_hints(initial_output);
    if signature_output.trim().is_empty() {
        return "waiting for initial prompt".to_string();
    }
    let last_signature =
        last_non_empty_line(&signature_output).unwrap_or(signature_output.as_str());
    format!("prompt_signature:\n{last_signature}")
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
                        let trimmed_line = line.trim_end();
                        handler.read(trimmed_line);
                    }

                    if !buffer.is_empty() {
                        if handler.read_prompt(&buffer) {
                            handler.read(&buffer);
                            prompt.clear();
                            prompt.push_str(handler.current_prompt().unwrap_or(&buffer));
                            return Ok(());
                        }
                        if let Some((c, _)) = handler.read_need_write(&buffer) {
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
    use super::{
        build_init_timeout_message, normalize_initial_output,
        normalize_initial_output_with_pua_hints,
    };

    #[test]
    fn normalize_initial_output_strips_terminal_sequences_and_private_use_symbols() {
        let raw = concat!(
            "Welcome\r\n",
            "\u{1b}[1m\u{1b}[7m%\u{1b}[27m\u{1b}[0m ",
            "\u{1b}[38;2;214;93;14m\u{1b}[0m ",
            "󰌽 adam@host ~ % ",
            "\u{1b}[?2004h"
        );

        let normalized = normalize_initial_output(raw);
        assert_eq!(normalized, "Welcome\n%   adam@host ~ % ");
        assert!(!normalized.contains('󰌽'));
    }

    #[test]
    fn normalize_initial_output_with_pua_hints_keeps_prompt_shape() {
        let raw = concat!("\u{1b}[1m%\u{1b}[0m ", "󰌽", " adam@host ~ ", "", " 10:38");

        let hinted = normalize_initial_output_with_pua_hints(raw);
        assert_eq!(hinted, "% <PUA> adam@host ~ <PUA> 10:38");
    }

    #[test]
    fn init_timeout_message_includes_prompt_candidate_hint_when_pua_exists() {
        let raw = concat!(
            "Welcome\r\n",
            "\u{1b}[1m%\u{1b}[0m ",
            "󰌽",
            " adam-work  ~   10:38  "
        );

        let message = build_init_timeout_message(raw);
        assert_eq!(message, "prompt_signature:\n% <PUA> adam-work  ~   10:38  ");
    }
}
