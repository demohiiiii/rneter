use super::super::*;

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
                    Some(data) = receiver_from_user.recv() => {
                        if let Err(e) = channel.data(data.as_bytes()).await {
                            debug!("{} Failed to send data to shell: {:?}", io_task_device_addr, e);
                            break;
                        }
                    },
                    Some(msg) = channel.wait() => {
                        match msg {
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
                        }
                    }
                }
            }
            let _ = MANAGER.cache.invalidate(&io_task_device_addr).await;
            debug!("{} SSH I/O task ended.", io_task_device_addr);
        });

        let mut buffer = String::new();
        let mut prompt = String::new();
        let mut initial_output = String::new();

        let mut params = HashMap::new();
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
                            prompt.push_str(&buffer);
                            handler.read(&buffer);
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
                return Err(ConnectError::InitTimeout(if initial_output.is_empty() {
                    "waiting for initial prompt".to_string()
                } else {
                    initial_output.clone()
                }));
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
