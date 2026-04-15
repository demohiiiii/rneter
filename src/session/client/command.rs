use super::super::*;
use super::tx::{
    OperationRunError, OperationRunFuture, TxCommandRunner, execute_tx_block_with_runner,
    execute_tx_workflow_with_runner,
};
use crate::device::{
    STRIP_CSI_ESCAPE, STRIP_DCS_ESCAPE, STRIP_OSC_ESCAPE, STRIP_SIMPLE_ESCAPE, is_private_use,
};
use regex::RegexSet;

fn sanitize_runtime_prompt(line: &str) -> String {
    let without_osc = STRIP_OSC_ESCAPE.replace_all(line, "");
    let without_dcs = STRIP_DCS_ESCAPE.replace_all(without_osc.as_ref(), "");
    let without_csi = STRIP_CSI_ESCAPE.replace_all(without_dcs.as_ref(), "");
    let without_simple = STRIP_SIMPLE_ESCAPE.replace_all(without_csi.as_ref(), "");
    without_simple
        .chars()
        .filter(|ch| (!ch.is_control() || matches!(ch, '\n' | '\r' | '\t')) && !is_private_use(*ch))
        .collect()
}

fn latest_terminal_fragment(line: &str) -> &str {
    line.rsplit(['\n', '\r'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(line)
}

fn normalize_runtime_output(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());

    for chunk in text.split_inclusive('\n') {
        let has_newline = chunk.ends_with('\n');
        let body = if has_newline {
            &chunk[..chunk.len().saturating_sub(1)]
        } else {
            chunk
        };
        let sanitized = sanitize_runtime_prompt(body);
        let visible = latest_terminal_fragment(&sanitized).trim_end_matches('\r');
        normalized.push_str(visible);
        if has_newline {
            normalized.push('\n');
        }
    }

    normalized
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_sent_command_prefix(output: &str, sent_command: &str) -> String {
    let trimmed = output.trim_start_matches(['\n', '\r']);
    let sent = sent_command.trim();
    if sent.is_empty() {
        return trimmed.to_string();
    }

    if let Some(rest) = trimmed.strip_prefix(sent) {
        return rest.trim_start_matches(['\n', '\r']).to_string();
    }

    if let Some((first_line, rest)) = trimmed.split_once('\n') {
        let first = first_line.trim_end_matches('\r');
        if first == sent || normalize_whitespace(first) == normalize_whitespace(sent) {
            return rest.trim_start_matches(['\n', '\r']).to_string();
        }
    }

    trimmed.to_string()
}

fn extract_command_content(all: &str, sent_command: &str, prompt: Option<&str>) -> String {
    let normalized_all = normalize_runtime_output(all);
    let normalized_command = normalize_runtime_output(sent_command);
    let stripped = strip_sent_command_prefix(&normalized_all, &normalized_command);
    let trimmed = stripped.trim_end_matches(['\n', '\r']);

    if let Some(prompt_text) = prompt {
        let normalized_prompt = normalize_runtime_output(prompt_text);
        if !normalized_prompt.is_empty()
            && let Some(without_prompt) = trimmed.strip_suffix(&normalized_prompt)
        {
            return without_prompt.trim_end_matches(['\n', '\r']).to_string();
        }
    }

    trimmed.to_string()
}

#[derive(Debug)]
struct RuntimePromptMatcher {
    patterns: RegexSet,
    response: String,
    record_input: bool,
}

#[derive(Debug, Default)]
struct RuntimeCommandInteraction {
    prompts: Vec<RuntimePromptMatcher>,
}

impl RuntimeCommandInteraction {
    fn build(interaction: &CommandInteraction) -> Result<Self, ConnectError> {
        let mut prompts = Vec::with_capacity(interaction.prompts.len());

        for (index, prompt) in interaction.prompts.iter().enumerate() {
            if prompt.patterns.is_empty() {
                return Err(ConnectError::InvalidCommandInteraction(format!(
                    "prompt rule at index {index} must include at least one regex pattern"
                )));
            }

            let patterns = RegexSet::new(&prompt.patterns).map_err(|err| {
                ConnectError::InvalidCommandInteraction(format!(
                    "invalid prompt regex at index {index}: {err}"
                ))
            })?;

            prompts.push(RuntimePromptMatcher {
                patterns,
                response: prompt.response.clone(),
                record_input: prompt.record_input,
            });
        }

        Ok(Self { prompts })
    }

    fn read_need_write(&self, line: &str) -> Option<(String, bool)> {
        let sanitized = sanitize_runtime_prompt(line);
        let prompt = latest_terminal_fragment(&sanitized);
        self.prompts
            .iter()
            .find(|rule| rule.patterns.is_match(prompt))
            .map(|prompt| (prompt.response.clone(), prompt.record_input))
    }
}

fn branch_source_text(
    source: &CommandOutputBranchSource,
    output: &SessionOperationStepOutput,
) -> String {
    match source {
        CommandOutputBranchSource::All => normalize_runtime_output(&output.all),
        CommandOutputBranchSource::Content => normalize_runtime_output(&output.content),
        CommandOutputBranchSource::Prompt => output
            .prompt
            .as_deref()
            .map(normalize_runtime_output)
            .unwrap_or_default(),
    }
}

fn resolve_output_branch_target(
    command: &Command,
    output: &SessionOperationStepOutput,
) -> Result<CommandBranchTarget, ConnectError> {
    for (rule_index, rule) in command.output_branches.iter().enumerate() {
        if rule.patterns.is_empty() {
            return Err(ConnectError::InvalidCommandFlow(format!(
                "command '{}' has an output branch rule with no patterns at index {}",
                command.command, rule_index
            )));
        }

        let patterns = RegexSet::new(&rule.patterns).map_err(|err| {
            ConnectError::InvalidCommandFlow(format!(
                "command '{}' has invalid output branch regex at index {}: {}",
                command.command, rule_index, err
            ))
        })?;
        let source_text = branch_source_text(&rule.source, output);
        if patterns.is_match(&source_text) {
            return Ok(rule.target.clone());
        }
    }

    Ok(command.output_fallback.clone())
}

impl SharedSshClient {
    async fn execute_command_step(
        &mut self,
        step_index: usize,
        command: &Command,
        sys: Option<&String>,
    ) -> Result<SessionOperationStepOutput, ConnectError> {
        let timeout = Duration::from_secs(command.timeout.unwrap_or(60));
        let output = self
            .write_with_mode_and_timeout_using_command(
                &command.command,
                &command.mode,
                sys,
                timeout,
                &command.dyn_params,
                &command.interaction,
            )
            .await?;

        Ok(SessionOperationStepOutput {
            step_index,
            mode: command.mode.clone(),
            operation_summary: command.command.clone(),
            success: output.success,
            exit_code: output.exit_code,
            content: output.content,
            all: output.all,
            prompt: output.prompt,
        })
    }

    async fn execute_command_flow_detailed(
        &mut self,
        flow: &CommandFlow,
        sys: Option<&String>,
    ) -> Result<SessionOperationOutput, OperationRunError> {
        let CommandFlow {
            steps,
            stop_on_error,
            max_steps,
        } = flow;
        if steps.is_empty() {
            return Ok(SessionOperationOutput {
                success: true,
                steps: Vec::new(),
            });
        }

        let mut outputs = Vec::with_capacity(steps.len());
        let mut cursor = 0usize;
        let mut executed_steps = 0usize;
        let limit = max_steps.unwrap_or_else(|| steps.len().saturating_mul(16).max(steps.len()));

        while cursor < steps.len() {
            if executed_steps >= limit {
                return Err(OperationRunError::new(
                    ConnectError::InvalidCommandFlow(format!(
                        "command flow exceeded max executed steps (limit: {limit})"
                    )),
                    SessionOperationOutput {
                        success: false,
                        steps: outputs,
                    },
                ));
            }
            let command = &steps[cursor];
            let output = match self.execute_command_step(cursor, command, sys).await {
                Ok(output) => output,
                Err(error) => {
                    return Err(OperationRunError::new(
                        error,
                        SessionOperationOutput {
                            success: false,
                            steps: outputs,
                        },
                    ));
                }
            };

            let step_success = output.success;
            let branch_target =
                resolve_output_branch_target(command, &output).map_err(|error| {
                    OperationRunError::new(
                        error,
                        SessionOperationOutput {
                            success: false,
                            steps: {
                                let mut partial = outputs.clone();
                                partial.push(output.clone());
                                partial
                            },
                        },
                    )
                })?;
            outputs.push(output);
            executed_steps += 1;
            if *stop_on_error && !step_success {
                return Ok(SessionOperationOutput {
                    success: false,
                    steps: outputs,
                });
            }

            match branch_target {
                CommandBranchTarget::Next => {
                    cursor += 1;
                }
                CommandBranchTarget::StopSuccess => {
                    return Ok(SessionOperationOutput {
                        success: true,
                        steps: outputs,
                    });
                }
                CommandBranchTarget::StopFailure => {
                    return Ok(SessionOperationOutput {
                        success: false,
                        steps: outputs,
                    });
                }
                CommandBranchTarget::Jump { step_index } => {
                    if step_index >= steps.len() {
                        return Err(OperationRunError::new(
                            ConnectError::InvalidCommandFlow(format!(
                                "command flow branch target step {} is out of range (total steps: {})",
                                step_index,
                                steps.len()
                            )),
                            SessionOperationOutput {
                                success: false,
                                steps: outputs,
                            },
                        ));
                    }
                    cursor = step_index;
                }
            }
        }

        let success = outputs.iter().all(|output| output.success);
        Ok(SessionOperationOutput {
            success,
            steps: outputs,
        })
    }

    pub(crate) async fn execute_operation_detailed(
        &mut self,
        operation: &SessionOperation,
        sys: Option<&String>,
    ) -> Result<SessionOperationOutput, OperationRunError> {
        match operation {
            SessionOperation::Command(command) => {
                let step = self
                    .execute_command_step(0, command, sys)
                    .await
                    .map_err(OperationRunError::from)?;
                Ok(SessionOperationOutput {
                    success: step.success,
                    steps: vec![step],
                })
            }
            SessionOperation::Flow(flow) => self.execute_command_flow_detailed(flow, sys).await,
            SessionOperation::Template { template, runtime } => {
                let flow = template
                    .to_command_flow(runtime)
                    .map_err(OperationRunError::from)?;
                self.execute_command_flow_detailed(&flow, sys).await
            }
        }
    }

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
        self.write_with_timeout_internal(command, timeout, true, &CommandInteraction::default())
            .await
    }

    async fn write_with_timeout_internal(
        &mut self,
        command: &str,
        timeout: Duration,
        capture_exit_status: bool,
        interaction: &CommandInteraction,
    ) -> Result<Output, ConnectError> {
        let runtime_interaction = RuntimeCommandInteraction::build(interaction)?;
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
                        if let Some((c, is_record)) =
                            runtime_interaction.read_need_write(&line_buffer)
                        {
                            handler.read(&line_buffer);
                            if !is_record {
                                line_buffer.clear();
                            }
                            trace!("Runtime input required: '{:?}'", c);
                            self.sender.send(c).await?;
                        } else if let Some((c, is_record)) = handler.read_need_write(&line_buffer) {
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
                return Err(ConnectError::ExecTimeout(normalize_runtime_output(
                    &clean_output,
                )));
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
        let prompt_after = self.handler.current_prompt().map(|v| v.to_string());
        let content = extract_command_content(&all, &sent_command, prompt_after.as_deref());

        let output = Output {
            success,
            exit_code,
            content,
            all,
            prompt: prompt_after,
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
        self.write_with_mode_and_timeout_using_command(
            command,
            mode,
            sys,
            timeout,
            &CommandDynamicParams::default(),
            &CommandInteraction::default(),
        )
        .await
    }

    /// Executes a command in a specific device mode with per-command overrides.
    pub(crate) async fn write_with_mode_and_timeout_using_command(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
        timeout: Duration,
        dyn_params: &CommandDynamicParams,
        interaction: &CommandInteraction,
    ) -> Result<Output, ConnectError> {
        let previous = self.merge_command_dyn_params(dyn_params);
        let result = self
            .write_with_mode_and_timeout_without_overrides(command, mode, sys, timeout, interaction)
            .await;
        self.restore_command_dyn_params(previous);
        result
    }

    async fn write_with_mode_and_timeout_without_overrides(
        &mut self,
        command: &str,
        mode: &str,
        sys: Option<&String>,
        timeout: Duration,
        interaction: &CommandInteraction,
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
                .write_with_timeout_internal(&t_cmd, timeout, false, &CommandInteraction::default())
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
            .write_with_timeout_internal(command, timeout, true, interaction)
            .await?;
        all.push_str(cmd_output.all.as_str());

        cmd_output.all = all;
        Ok(cmd_output)
    }

    /// Execute a transaction-like command block.
    ///
    /// Forward steps run sequentially. On failure, rollback follows
    /// the block's [`RollbackPolicy`] (`none`, `whole_resource`, `per_step`).
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

    fn run_operation<'a>(
        &'a mut self,
        operation: &'a SessionOperation,
        sys: Option<&'a String>,
    ) -> OperationRunFuture<'a> {
        Box::pin(async move { self.execute_operation_detailed(operation, sys).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_step_output(
        content: &str,
        all: &str,
        prompt: Option<&str>,
    ) -> SessionOperationStepOutput {
        SessionOperationStepOutput {
            step_index: 0,
            mode: "Enable".to_string(),
            operation_summary: "show version".to_string(),
            success: true,
            exit_code: Some(0),
            content: content.to_string(),
            all: all.to_string(),
            prompt: prompt.map(ToString::to_string),
        }
    }

    #[test]
    fn runtime_command_interaction_matches_sanitized_prompt() {
        let interaction = RuntimeCommandInteraction::build(&CommandInteraction {
            prompts: vec![PromptResponseRule::new(
                vec![r"^Password:\s*$".to_string()],
                "secret\n".to_string(),
            )],
        })
        .expect("build interaction");

        let prompt = "\u{1b}[31mPassword:\u{1b}[0m";
        assert_eq!(
            interaction.read_need_write(prompt),
            Some(("secret\n".to_string(), false))
        );
    }

    #[test]
    fn runtime_command_interaction_matches_last_carriage_return_fragment() {
        let interaction = RuntimeCommandInteraction::build(&CommandInteraction {
            prompts: vec![PromptResponseRule::new(
                vec![r"^Password:\s*$".to_string()],
                "secret\n".to_string(),
            )],
        })
        .expect("build interaction");

        let prompt = "noise\r\u{1b}[31mPassword:\u{1b}[0m";
        assert_eq!(
            interaction.read_need_write(prompt),
            Some(("secret\n".to_string(), false))
        );
    }

    #[test]
    fn runtime_command_interaction_rejects_invalid_regex() {
        let err = RuntimeCommandInteraction::build(&CommandInteraction {
            prompts: vec![PromptResponseRule::new(
                vec!["[".to_string()],
                "secret\n".to_string(),
            )],
        })
        .expect_err("invalid regex should fail");

        assert!(matches!(err, ConnectError::InvalidCommandInteraction(_)));
    }

    #[test]
    fn extract_command_content_strips_fish_wrapper_echo_and_prompt() {
        let sent_command = r#"date; printf '\n__RNETER_EXIT_CODE__:%s:__\n' "$status""#;
        let raw_output = concat!(
            "date;\r",
            "\u{1b}[27C \r\u{1b}[28Cprintf \r\u{1b}[35C'\\n__RNETER_EXIT_CODE__:%s:__\\n' ",
            "\r\u{1b}[68C\u{1b}[?2004l\u{1b}[>4;0m\u{1b}>\"$status\"\r",
            "\u{1b}[77C\u{1b}[55Ddate\u{1b}[32m;\u{1b}[m printf \u{1b}[33m'\\n__RNETER_EXIT_CODE__:%s:__\\n'\u{1b}[m ",
            "\u{1b}[33m\"\u{1b}[96m$status\u{1b}[33m\"\u{1b}[m\r\u{1b}[77C\n",
            "Thu Apr  9 10:51:14 AM CST 2026\n",
            "[192-168-30] ~# "
        );

        let content = extract_command_content(raw_output, sent_command, Some("[192-168-30] ~# "));
        assert_eq!(content, "Thu Apr  9 10:51:14 AM CST 2026");
    }

    #[test]
    fn resolve_output_branch_target_matches_content_rule() {
        let command = Command {
            command: "copy scp: flash:/image.bin".to_string(),
            output_branches: vec![
                CommandOutputBranchRule::new(
                    vec![r"(?i)retry".to_string()],
                    CommandBranchTarget::Jump { step_index: 1 },
                )
                .with_source(CommandOutputBranchSource::Content),
            ],
            output_fallback: CommandBranchTarget::StopFailure,
            ..Command::default()
        };
        let output = sample_step_output("please retry transfer", "please retry transfer", None);

        let target = resolve_output_branch_target(&command, &output).expect("resolve branch");
        assert_eq!(target, CommandBranchTarget::Jump { step_index: 1 });
    }

    #[test]
    fn resolve_output_branch_target_uses_fallback_when_no_rule_matches() {
        let command = Command {
            command: "show version".to_string(),
            output_branches: vec![
                CommandOutputBranchRule::new(
                    vec![r"(?i)fatal".to_string()],
                    CommandBranchTarget::StopFailure,
                )
                .with_source(CommandOutputBranchSource::All),
            ],
            output_fallback: CommandBranchTarget::Next,
            ..Command::default()
        };
        let output = sample_step_output("completed", "completed", Some("router#"));

        let target = resolve_output_branch_target(&command, &output).expect("resolve branch");
        assert_eq!(target, CommandBranchTarget::Next);
    }

    #[test]
    fn resolve_output_branch_target_rejects_invalid_regex() {
        let command = Command {
            command: "show version".to_string(),
            output_branches: vec![CommandOutputBranchRule::new(
                vec!["[".to_string()],
                CommandBranchTarget::StopFailure,
            )],
            ..Command::default()
        };
        let output = sample_step_output("completed", "completed", Some("router#"));

        let err =
            resolve_output_branch_target(&command, &output).expect_err("invalid regex should fail");
        assert!(matches!(err, ConnectError::InvalidCommandFlow(_)));
    }
}
