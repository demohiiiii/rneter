use super::super::*;
use super::tx::{
    CommandRunFuture, TxCommandRunner, execute_tx_block_with_runner,
    execute_tx_workflow_with_runner,
};

impl SharedSshClient {
    fn merge_command_dyn_params(
        &mut self,
        dyn_params: &CommandDynamicParams,
    ) -> Vec<(String, Option<String>)> {
        let runtime_values = dyn_params.runtime_values();
        let mut previous = Vec::with_capacity(runtime_values.len());
        for (key, value) in runtime_values {
            previous.push((key.clone(), self.handler.dyn_param.insert(key, value)));
        }
        previous
    }

    fn restore_command_dyn_params(&mut self, previous: Vec<(String, Option<String>)>) {
        for (key, old_value) in previous {
            if let Some(old_value) = old_value {
                self.handler.dyn_param.insert(key, old_value);
            } else {
                self.handler.dyn_param.remove(&key);
            }
        }
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
        self.write_with_timeout_internal(command, timeout, true)
            .await
    }

    async fn write_with_timeout_internal(
        &mut self,
        command: &str,
        timeout: Duration,
        capture_exit_status: bool,
    ) -> Result<Output, ConnectError> {
        let handler = &mut self.handler;

        let recv = &mut self.recv;
        let prompt = &mut self.prompt;
        let prompt_before = prompt.clone();
        let mode = handler.current_state().to_string();
        let fsm_prompt_before = handler.current_state().to_string();

        while recv.try_recv().is_ok() {}

        let sent_command = handler.prepare_command_for_execution(command, capture_exit_status);
        let full_command = format!("{}\n", sent_command);
        self.sender.send(full_command).await?;

        let mut clean_output = String::new();
        let mut line_buffer = String::new();
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
                        line.clear();
                        line.extend(line_buffer.drain(..=newline_pos));
                        let trim_start = IGNORE_START_LINE.replace(&line, "");
                        let trimmed_line = trim_start.trim_end();

                        handler.read(trimmed_line);

                        if handler.error() {
                            is_error = true;
                        }

                        clean_output.push_str(&trim_start);
                    }

                    if !line_buffer.is_empty() {
                        if handler.read_prompt(&line_buffer) {
                            handler.read(&line_buffer);
                            let matched_prompt =
                                handler.current_prompt().unwrap_or(&line_buffer).to_string();
                            clean_output.push_str(&line_buffer);
                            if let Some(recorder) = self.recorder.as_ref()
                                && *prompt != matched_prompt
                            {
                                let _ = recorder.record_event(SessionEvent::PromptChanged {
                                    prompt: matched_prompt.clone(),
                                });
                            }
                            *prompt = matched_prompt;
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
                        exit_code: None,
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
                        exit_code: None,
                        content: clean_output.clone(),
                        all: clean_output.clone(),
                    });
                }
                return Err(err);
            }
            Ok(Ok(success)) => success,
        };

        let parsed =
            self.handler
                .finalize_command_output(&clean_output, success, capture_exit_status);
        let success = parsed.success;
        let exit_code = parsed.exit_code;
        let all = parsed.output;

        let mut content = all.as_str();
        if !sent_command.is_empty() && content.starts_with(&sent_command) {
            content = content
                .strip_prefix(&sent_command)
                .unwrap_or(content)
                .trim_start_matches(['\n', '\r']);
        }

        let content = if let Some(pos) = content.rfind('\n') {
            &content[..pos]
        } else {
            ""
        };

        let output = Output {
            success,
            exit_code,
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
                exit_code: output.exit_code,
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
        self.write_with_mode_and_timeout_using_dyn_params(
            command,
            mode,
            sys,
            timeout,
            &CommandDynamicParams::default(),
        )
        .await
    }

    /// Executes a command in a specific device mode with per-command dynamic prompt responses.
    pub(crate) async fn write_with_mode_and_timeout_using_dyn_params(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
        timeout: Duration,
        dyn_params: &CommandDynamicParams,
    ) -> Result<Output, ConnectError> {
        let previous = self.merge_command_dyn_params(dyn_params);
        let result = self
            .write_with_mode_and_timeout_without_dyn_params(command, mode, sys, timeout)
            .await;
        self.restore_command_dyn_params(previous);
        result
    }

    async fn write_with_mode_and_timeout_without_dyn_params(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
        timeout: Duration,
    ) -> Result<Output, ConnectError> {
        let handler = &self.handler;

        let temp_mode = mode.to_ascii_lowercase();
        let mode = temp_mode.as_str();
        let mut last_state = self.handler.current_state().to_string();

        let trans_cmds = handler.trans_state_write(mode, sys)?;
        let mut all = self.prompt.clone();

        for (t_cmd, target_state) in trans_cmds {
            debug!("Trans state command: {}", t_cmd);
            let mut mode_output = self
                .write_with_timeout_internal(&t_cmd, timeout, false)
                .await?;
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

        let mut cmd_output = self
            .write_with_timeout_internal(command, timeout, true)
            .await?;
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
        execute_tx_block_with_runner(self, block, sys).await
    }

    /// Execute multi-block workflow with global rollback on failure.
    pub async fn execute_tx_workflow(
        &mut self,
        workflow: &TxWorkflow,
        sys: Option<&String>,
    ) -> Result<TxWorkflowResult, ConnectError> {
        execute_tx_workflow_with_runner(self, workflow, sys).await
    }
}

impl TxCommandRunner for SharedSshClient {
    fn recorder(&self) -> Option<&SessionRecorder> {
        self.recorder.as_ref()
    }

    fn run_command<'a>(
        &'a mut self,
        command: &'a str,
        mode: &'a str,
        sys: Option<&'a String>,
        timeout: Duration,
    ) -> CommandRunFuture<'a> {
        Box::pin(async move {
            self.write_with_mode_and_timeout(command, mode, sys, timeout)
                .await
        })
    }
}
