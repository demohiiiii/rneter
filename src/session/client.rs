use super::*;

impl SharedSshClient {
    async fn rollback_committed_block(
        &mut self,
        block: &TxBlock,
        sys: Option<&String>,
    ) -> Result<(bool, Vec<String>), ConnectError> {
        if block.kind == CommandBlockKind::Show {
            return Ok((true, Vec::new()));
        }
        let executed = (0..block.steps.len()).collect::<Vec<_>>();
        let plan = block.plan_rollback(&executed, None)?;
        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::TxRollbackStarted {
                block_name: block.name.clone(),
            });
        }
        let mut rollback_succeeded = true;
        let mut rollback_errors = Vec::new();
        for (plan_idx, rollback) in plan.into_iter().enumerate() {
            let timeout = Duration::from_secs(rollback.timeout_secs.unwrap_or(60));
            match self
                .write_with_mode_and_timeout(&rollback.command, &rollback.mode, sys, timeout)
                .await
            {
                Ok(output) if output.success => {
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxRollbackStepSucceeded {
                            block_name: block.name.clone(),
                            step_index: Some(plan_idx),
                            mode: rollback.mode.clone(),
                            command: rollback.command.clone(),
                        });
                    }
                }
                Ok(output) => {
                    rollback_succeeded = false;
                    let reason = format!(
                        "workflow rollback command failed for block '{}': '{}' output='{}'",
                        block.name, rollback.command, output.content
                    );
                    rollback_errors.push(reason.clone());
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                            block_name: block.name.clone(),
                            step_index: Some(plan_idx),
                            mode: rollback.mode.clone(),
                            command: rollback.command.clone(),
                            reason,
                        });
                    }
                }
                Err(err) => {
                    rollback_succeeded = false;
                    let reason = format!(
                        "workflow rollback command error for block '{}': '{}' err={}",
                        block.name, rollback.command, err
                    );
                    rollback_errors.push(reason.clone());
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                            block_name: block.name.clone(),
                            step_index: Some(plan_idx),
                            mode: rollback.mode.clone(),
                            command: rollback.command.clone(),
                            reason,
                        });
                    }
                }
            }
        }

        Ok((rollback_succeeded, rollback_errors))
    }

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
        // Compare password hash
        let password_hash = Self::calculate_password_hash(password);
        if self.password_hash != password_hash {
            debug!("Password hash mismatch");
            return false;
        }

        // Compare enable password hash
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

        // 1. Stop receiving new data
        self.recv.close();

        // 2. Try sending exit command (if connected)
        if self.is_connected() {
            // Send exit command to attempt graceful exit
            if let Err(e) = self.sender.send("exit\n".to_string()).await {
                debug!("Failed to send exit command: {:?}", e);
            }

            // Give some time for command execution
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // 3. Close underlying SSH client
        // async-ssh2-tokio Client currently closes automatically on drop
        // but we can explicitly call disconnect if available/needed

        debug!("SSH connection safely closed");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn new(
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

        // Initialize dynamic params in handler.
        let mut params = HashMap::new();
        if let Some(enable) = enable_password.as_ref() {
            params.insert("EnablePassword".to_string(), format!("{}\n", enable));
        }
        handler.dyn_param = params;

        // Wait for prompt output.
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
                    // Channel closed
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

        // Calculate and store password hash
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

    /// Executes a command and waits for the full output by matching the prompt.
    ///
    /// Uses the default timeout of 60 seconds.
    pub async fn write(&mut self, command: &str) -> Result<Output, ConnectError> {
        self.write_with_timeout(command, Duration::from_secs(60))
            .await
    }

    /// Executes a command with a custom timeout.
    pub async fn write_with_timeout(
        &mut self,
        command: &str,
        timeout: Duration,
    ) -> Result<Output, ConnectError> {
        // Ensure handler exists
        let handler = &mut self.handler;

        let recv = &mut self.recv;
        let prompt = &mut self.prompt;
        let prompt_before = prompt.clone();
        let mode = handler.current_state().to_string();
        let fsm_prompt_before = handler.current_state().to_string();

        // 1. Clear any residual data in the receiver
        while recv.try_recv().is_ok() {}

        // 2. Send command to remote shell
        let full_command = format!("{}\n", command);

        self.sender.send(full_command).await?;

        // 3. Receive data
        let mut clean_output = String::new();
        let mut line_buffer = String::new(); // Accumulates data into complete lines

        let mut line = String::new();

        let result = tokio::time::timeout(timeout, async {
            let mut is_error = false;
            loop {
                if let Some(data) = recv.recv().await {
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_raw_chunk(data.clone());
                    }
                    line_buffer.push_str(&data);

                    while let Some(newline_pos) = line_buffer.find('\n') {
                        line.clear(); // Clear buffer for reuse
                        line.extend(line_buffer.drain(..=newline_pos));
                        let trim_start = IGNORE_START_LINE.replace(&line, "");
                        let trimmed_line = trim_start.trim_end();

                        handler.read(trimmed_line);

                        if handler.error() {
                            is_error = true;
                        }

                        clean_output.push_str(&trim_start);
                    }

                    // Stage 2: Check remaining incomplete line in buffer (likely the prompt)
                    // Critical for prompts without newlines
                    if !line_buffer.is_empty() {
                        if handler.read_prompt(&line_buffer) {
                            handler.read(&line_buffer);
                            clean_output.push_str(&line_buffer);
                            if let Some(recorder) = self.recorder.as_ref()
                                && *prompt != line_buffer
                            {
                                let _ = recorder.record_event(SessionEvent::PromptChanged {
                                    prompt: line_buffer.clone(),
                                });
                            }
                            *prompt = line_buffer;
                            if is_error {
                                return Ok(false);
                            }
                            return Ok(true);
                        }
                        if let Some((c, is_record)) = handler.read_need_write(&line_buffer) {
                            handler.read(&line_buffer);
                            if !is_record {
                                line_buffer.clear();
                            }
                            trace!("Input required: '{:?}'", c);
                            self.sender.send(c).await?;
                        }
                    }
                } else {
                    // Channel closed
                    return Err(ConnectError::ChannelDisconnectError);
                }
            }
        })
        .await;

        let success = match result {
            Err(_) => {
                if let Some(recorder) = self.recorder.as_ref() {
                    let _ = recorder.record_event(SessionEvent::CommandOutput {
                        command: command.to_string(),
                        mode: mode.clone(),
                        prompt_before: Some(prompt_before.clone()),
                        prompt_after: Some(prompt.clone()),
                        fsm_prompt_before: Some(fsm_prompt_before.clone()),
                        fsm_prompt_after: Some(self.handler.current_state().to_string()),
                        success: false,
                        content: clean_output.clone(),
                        all: clean_output.clone(),
                    });
                }
                return Err(ConnectError::ExecTimeout(clean_output));
            }
            Ok(Err(err)) => {
                if let Some(recorder) = self.recorder.as_ref() {
                    let _ = recorder.record_event(SessionEvent::CommandOutput {
                        command: command.to_string(),
                        mode: mode.clone(),
                        prompt_before: Some(prompt_before.clone()),
                        prompt_after: Some(prompt.clone()),
                        fsm_prompt_before: Some(fsm_prompt_before.clone()),
                        fsm_prompt_after: Some(self.handler.current_state().to_string()),
                        success: false,
                        content: clean_output.clone(),
                        all: clean_output.clone(),
                    });
                }
                return Err(err);
            }
            Ok(Ok(success)) => success,
        };

        let all = clean_output;

        let mut content = all.as_str();

        // Remove the echoed command from the beginning of the output
        if !command.is_empty() && content.starts_with(command) {
            content = content
                .strip_prefix(command)
                .unwrap_or(content)
                .trim_start_matches(['\n', '\r']);
        }

        // Remove the trailing prompt
        let content = if let Some(pos) = content.rfind('\n') {
            &content[..pos]
        } else {
            ""
        };

        let output = Output {
            success,
            content: content.to_string(),
            all,
            prompt: self.handler.current_prompt().map(|v| v.to_string()),
        };

        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::CommandOutput {
                command: command.to_string(),
                mode,
                prompt_before: Some(prompt_before),
                prompt_after: Some(prompt.clone()),
                fsm_prompt_before: Some(fsm_prompt_before),
                fsm_prompt_after: Some(self.handler.current_state().to_string()),
                success: output.success,
                content: output.content.clone(),
                all: output.all.clone(),
            });
        }

        Ok(output)
    }

    /// Executes a command in a specific device mode.
    ///
    /// Automatically handles state transitions to reach the target mode.
    pub async fn write_with_mode(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
    ) -> Result<Output, ConnectError> {
        self.write_with_mode_and_timeout(command, mode, sys, Duration::from_secs(60))
            .await
    }

    /// Executes a command in a specific device mode with a custom timeout.
    pub async fn write_with_mode_and_timeout(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
        timeout: Duration,
    ) -> Result<Output, ConnectError> {
        // Ensure handler exists
        let handler = &self.handler;

        let temp_mode = mode.to_ascii_lowercase();
        let mode = temp_mode.as_str();
        let mut last_state = self.handler.current_state().to_string();

        let trans_cmds = handler.trans_state_write(mode, sys)?;
        let mut all = self.prompt.clone();

        // Execute transition commands
        for (t_cmd, target_state) in trans_cmds {
            debug!("Trans state command: {}", t_cmd);
            let mut mode_output = self.write_with_timeout(&t_cmd, timeout).await?;
            all.push_str(mode_output.all.as_str());
            if !mode_output.success {
                mode_output.all = all;
                return Ok(mode_output);
            }

            if !self.handler.current_state().eq(&target_state) {
                mode_output.success = false;
                mode_output.all = all;
                return Ok(mode_output);
            }

            let current_state = self.handler.current_state().to_string();
            if let Some(recorder) = self.recorder.as_ref()
                && current_state != last_state
            {
                let _ = recorder.record_event(SessionEvent::StateChanged {
                    state: current_state.clone(),
                });
            }
            last_state = current_state;
        }

        // Execute the actual command
        let mut cmd_output = self.write_with_timeout(command, timeout).await?;

        all.push_str(cmd_output.all.as_str());

        cmd_output.all = all;
        Ok(cmd_output)
    }

    /// Execute a transaction-like command block.
    ///
    /// For `show` blocks, commands are executed sequentially without rollback.
    /// For `config` blocks, failure triggers rollback according to policy.
    pub async fn execute_tx_block(
        &mut self,
        block: &TxBlock,
        sys: Option<&String>,
    ) -> Result<TxResult, ConnectError> {
        // Validate invariants once at entry, so runtime branches can assume a valid model.
        block.validate()?;
        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::TxBlockStarted {
                block_name: block.name.clone(),
                block_kind: block.kind,
            });
        }

        let mut executed_indices = Vec::new();
        let mut failure_reason = None;
        let mut failed_step = None;

        // Forward phase: execute each step in order.
        for (idx, step) in block.steps.iter().enumerate() {
            let timeout = Duration::from_secs(step.timeout_secs.unwrap_or(60));
            match self
                .write_with_mode_and_timeout(&step.command, &step.mode, sys, timeout)
                .await
            {
                Ok(output) if output.success => {
                    executed_indices.push(idx);
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxStepSucceeded {
                            block_name: block.name.clone(),
                            step_index: idx,
                            mode: step.mode.clone(),
                            command: step.command.clone(),
                        });
                    }
                }
                Ok(output) => {
                    failed_step = Some(idx);
                    failure_reason = Some(format!(
                        "step[{idx}] command failed: '{}' output='{}'",
                        step.command, output.content
                    ));
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxStepFailed {
                            block_name: block.name.clone(),
                            step_index: idx,
                            mode: step.mode.clone(),
                            command: step.command.clone(),
                            reason: failure_reason.clone().unwrap_or_default(),
                        });
                    }
                    if block.fail_fast {
                        break;
                    }
                }
                Err(err) => {
                    failed_step = Some(idx);
                    failure_reason = Some(format!("step[{idx}] command error: {err}"));
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxStepFailed {
                            block_name: block.name.clone(),
                            step_index: idx,
                            mode: step.mode.clone(),
                            command: step.command.clone(),
                            reason: failure_reason.clone().unwrap_or_default(),
                        });
                    }
                    if block.fail_fast {
                        break;
                    }
                }
            }
        }

        // All forward steps succeeded -> committed.
        if failed_step.is_none() {
            let result = TxResult::committed(block.name.clone(), executed_indices.len());
            if let Some(recorder) = self.recorder.as_ref() {
                let _ = recorder.record_event(SessionEvent::TxBlockFinished {
                    block_name: block.name.clone(),
                    committed: true,
                    rollback_attempted: false,
                    rollback_succeeded: false,
                });
            }
            return Ok(result);
        }

        // `show` blocks intentionally do not rollback even on failures.
        if block.kind == CommandBlockKind::Show {
            let result = TxResult {
                block_name: block.name.clone(),
                committed: false,
                failed_step,
                executed_steps: executed_indices.len(),
                rollback_attempted: false,
                rollback_succeeded: false,
                rollback_steps: 0,
                failure_reason,
                rollback_errors: Vec::new(),
            };
            if let Some(recorder) = self.recorder.as_ref() {
                let _ = recorder.record_event(SessionEvent::TxBlockFinished {
                    block_name: block.name.clone(),
                    committed: false,
                    rollback_attempted: false,
                    rollback_succeeded: false,
                });
            }
            return Ok(result);
        }

        // Compensation phase: build rollback commands from executed success path.
        let rollback_plan = block.plan_rollback(&executed_indices, failed_step)?;
        let rollback_attempted = !rollback_plan.is_empty();
        if rollback_attempted && let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::TxRollbackStarted {
                block_name: block.name.clone(),
            });
        }
        let mut rollback_succeeded = rollback_attempted;
        let mut rollback_errors = Vec::new();
        let mut rollback_steps = 0;
        if !rollback_attempted {
            let reason = format!(
                "rollback not attempted: no rollback commands for executed steps; forward_failure={}",
                failure_reason
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            );
            rollback_errors.push(reason);
        }

        // Execute rollback commands in planned order (already reversed for per-step policy).
        for (plan_idx, rollback) in rollback_plan.into_iter().enumerate() {
            let timeout = Duration::from_secs(rollback.timeout_secs.unwrap_or(60));
            match self
                .write_with_mode_and_timeout(&rollback.command, &rollback.mode, sys, timeout)
                .await
            {
                Ok(output) if output.success => {
                    rollback_steps += 1;
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxRollbackStepSucceeded {
                            block_name: block.name.clone(),
                            step_index: Some(plan_idx),
                            mode: rollback.mode.clone(),
                            command: rollback.command.clone(),
                        });
                    }
                }
                Ok(output) => {
                    rollback_succeeded = false;
                    rollback_steps += 1;
                    let reason = format!(
                        "rollback command failed: '{}' output='{}'",
                        rollback.command, output.content
                    );
                    rollback_errors.push(reason.clone());
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                            block_name: block.name.clone(),
                            step_index: Some(plan_idx),
                            mode: rollback.mode.clone(),
                            command: rollback.command.clone(),
                            reason,
                        });
                    }
                }
                Err(err) => {
                    rollback_succeeded = false;
                    let reason =
                        format!("rollback command error: '{}' err={}", rollback.command, err);
                    rollback_errors.push(reason.clone());
                    if let Some(recorder) = self.recorder.as_ref() {
                        let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                            block_name: block.name.clone(),
                            step_index: Some(plan_idx),
                            mode: rollback.mode.clone(),
                            command: rollback.command.clone(),
                            reason,
                        });
                    }
                }
            }
        }

        let result = TxResult {
            block_name: block.name.clone(),
            committed: false,
            failed_step,
            executed_steps: executed_indices.len(),
            rollback_attempted,
            rollback_succeeded,
            rollback_steps,
            failure_reason,
            rollback_errors,
        };

        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::TxBlockFinished {
                block_name: block.name.clone(),
                committed: false,
                rollback_attempted: result.rollback_attempted,
                rollback_succeeded: result.rollback_succeeded,
            });
        }

        Ok(result)
    }

    /// Execute multi-block workflow with global rollback on failure.
    pub async fn execute_tx_workflow(
        &mut self,
        workflow: &TxWorkflow,
        sys: Option<&String>,
    ) -> Result<TxWorkflowResult, ConnectError> {
        workflow.validate()?;
        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::TxWorkflowStarted {
                workflow_name: workflow.name.clone(),
                total_blocks: workflow.blocks.len(),
            });
        }

        let mut block_results = Vec::with_capacity(workflow.blocks.len());
        let mut committed_block_indices = Vec::new();
        let mut failed_block = None;

        for (idx, block) in workflow.blocks.iter().enumerate() {
            let result = self.execute_tx_block(block, sys).await?;
            let committed = result.committed;
            block_results.push(result);
            if committed {
                committed_block_indices.push(idx);
                continue;
            }
            failed_block = Some(idx);
            if workflow.fail_fast {
                break;
            }
        }

        if failed_block.is_none() {
            if let Some(recorder) = self.recorder.as_ref() {
                let _ = recorder.record_event(SessionEvent::TxWorkflowFinished {
                    workflow_name: workflow.name.clone(),
                    committed: true,
                    rollback_attempted: false,
                    rollback_succeeded: false,
                });
            }
            return Ok(TxWorkflowResult {
                workflow_name: workflow.name.clone(),
                committed: true,
                failed_block: None,
                block_results,
                rollback_attempted: false,
                rollback_succeeded: false,
                rollback_errors: Vec::new(),
            });
        }

        let failed_idx = failed_block.unwrap_or(0);
        let (mut rollback_attempted, mut rollback_succeeded, mut rollback_errors) =
            failed_block_rollback_summary(block_results.get(failed_idx));

        // Roll back previously committed blocks in reverse order.
        for block_idx in workflow_rollback_order(&committed_block_indices, failed_idx) {
            rollback_attempted = true;
            if let Some(block) = workflow.blocks.get(block_idx) {
                let (ok, errors) = self.rollback_committed_block(block, sys).await?;
                if !ok {
                    rollback_succeeded = false;
                }
                rollback_errors.extend(errors);
            }
        }

        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::TxWorkflowFinished {
                workflow_name: workflow.name.clone(),
                committed: false,
                rollback_attempted,
                rollback_succeeded,
            });
        }

        Ok(TxWorkflowResult {
            workflow_name: workflow.name.clone(),
            committed: false,
            failed_block,
            block_results,
            rollback_attempted,
            rollback_succeeded,
            rollback_errors,
        })
    }
}
