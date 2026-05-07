use super::super::*;
use super::tx::{
    OperationRunError, OperationRunFuture, TxCommandRunner, execute_tx_block_with_runner,
    execute_tx_workflow_with_runner,
};
use crate::device::{
    latest_terminal_fragment, merge_terminal_prompt_fragments, normalize_terminal_output,
    sanitize_terminal_text, terminal_fragment_has_pua,
};
use regex::RegexSet;

fn sanitize_runtime_prompt(line: &str) -> String {
    sanitize_terminal_text(line)
}

fn normalize_runtime_output(text: &str) -> String {
    normalize_terminal_output(text)
}

fn build_exec_timeout_message(output: &str) -> String {
    let normalized_output = normalize_runtime_output(output);
    if normalized_output.trim().is_empty() {
        return "waiting for prompt".to_string();
    }
    normalized_output
}

fn flush_pending_prompt_lines(
    handler: &mut DeviceHandler,
    pending_prompt_lines: &mut Vec<String>,
    clean_output: &mut String,
    is_error: &mut bool,
) {
    for pending_line in pending_prompt_lines.drain(..) {
        let trimmed_line = pending_line.trim_end();
        handler.read(trimmed_line);
        if handler.error() {
            *is_error = true;
        }
        clean_output.push_str(&pending_line);
    }
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
        let normalized_first = normalize_whitespace(first);
        let normalized_sent = normalize_whitespace(sent);
        if first == sent
            || normalized_first == normalized_sent
            || normalized_first.contains(&normalized_sent)
        {
            return rest.trim_start_matches(['\n', '\r']).to_string();
        }
    }

    trimmed.to_string()
}

fn normalize_echo_text(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '-' | '_' | '/' | '.' | ':'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn common_subsequence_len(left: &str, right: &str) -> usize {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut previous = vec![0usize; right.len() + 1];
    let mut current = vec![0usize; right.len() + 1];

    for left_byte in left {
        for (index, right_byte) in right.iter().enumerate() {
            current[index + 1] = if left_byte == right_byte {
                previous[index] + 1
            } else {
                previous[index + 1].max(current[index])
            };
        }
        std::mem::swap(&mut previous, &mut current);
        current.fill(0);
    }

    previous[right.len()]
}

fn normalized_command_first_word(command: &str) -> String {
    command
        .split_whitespace()
        .next()
        .map(|token| {
            token
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '-' | '_'))
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .unwrap_or_default()
}

fn terminal_render_line(raw: &str) -> String {
    let mut cells = Vec::<char>::new();
    let mut cursor = 0usize;
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    let mut params = String::new();
                    let mut final_byte = None;
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            final_byte = Some(next);
                            break;
                        }
                        params.push(next);
                    }
                    apply_csi_sequence(&mut cells, &mut cursor, &params, final_byte);
                }
                Some(']') => {
                    chars.next();
                    skip_until_string_terminator(&mut chars);
                }
                Some('P') => {
                    chars.next();
                    skip_until_string_terminator(&mut chars);
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            '\r' => cursor = 0,
            '\n' => {}
            '\x08' => cursor = cursor.saturating_sub(1),
            '\t' => {
                let spaces = 4 - (cursor % 4);
                for _ in 0..spaces {
                    put_terminal_char(&mut cells, &mut cursor, ' ');
                }
            }
            ch if ch.is_control() => {}
            ch => put_terminal_char(&mut cells, &mut cursor, ch),
        }
    }

    cells.into_iter().collect()
}

fn terminal_control_summary(raw: &str) -> Vec<String> {
    let mut controls = Vec::new();
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => controls.push("CR".to_string()),
            '\n' => controls.push("LF".to_string()),
            '\x08' => controls.push("BS".to_string()),
            '\x1b' => match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    let mut params = String::new();
                    let mut final_byte = None;
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            final_byte = Some(next);
                            break;
                        }
                        params.push(next);
                    }
                    controls.push(format!("CSI({}{})", params, final_byte.unwrap_or('?')));
                }
                Some(']') => {
                    chars.next();
                    skip_until_string_terminator(&mut chars);
                    controls.push("OSC".to_string());
                }
                Some('P') => {
                    chars.next();
                    skip_until_string_terminator(&mut chars);
                    controls.push("DCS".to_string());
                }
                Some(next) => {
                    chars.next();
                    controls.push(format!("ESC({next})"));
                }
                None => controls.push("ESC".to_string()),
            },
            ch if ch.is_control() => controls.push(format!("CTRL({:#04x})", ch as u32)),
            _ => {}
        }
    }

    controls
}

fn put_terminal_char(cells: &mut Vec<char>, cursor: &mut usize, ch: char) {
    if *cursor >= cells.len() {
        cells.resize(*cursor + 1, ' ');
    }
    cells[*cursor] = ch;
    *cursor += 1;
}

fn skip_until_string_terminator(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(ch) = chars.next() {
        if ch == '\x07' {
            break;
        }
        if ch == '\x1b' && matches!(chars.peek(), Some('\\')) {
            chars.next();
            break;
        }
    }
}

fn first_csi_param(params: &str, default: usize) -> usize {
    params
        .split(';')
        .next()
        .and_then(|value| {
            let digits = value.trim_start_matches('?');
            if digits.is_empty() {
                None
            } else {
                digits.parse::<usize>().ok()
            }
        })
        .unwrap_or(default)
}

fn apply_csi_sequence(
    cells: &mut Vec<char>,
    cursor: &mut usize,
    params: &str,
    final_byte: Option<char>,
) {
    match final_byte {
        Some('C') => *cursor += first_csi_param(params, 1),
        Some('D') => *cursor = cursor.saturating_sub(first_csi_param(params, 1)),
        Some('G') => *cursor = first_csi_param(params, 1).saturating_sub(1),
        Some('K') => {
            let mode = first_csi_param(params, 0);
            match mode {
                0 => cells.truncate(*cursor),
                1 => {
                    let end = (*cursor).min(cells.len().saturating_sub(1));
                    for cell in cells.iter_mut().take(end + 1) {
                        *cell = ' ';
                    }
                }
                2 => {
                    cells.clear();
                    *cursor = 0;
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn rendered_line_is_command_echo(rendered_line: &str, sent_command: &str) -> bool {
    let first_word = normalized_command_first_word(sent_command);
    if first_word.len() < 2 {
        return false;
    }

    let rendered = rendered_line.trim();
    if rendered.is_empty() {
        return false;
    }

    let rendered_lower = rendered.to_ascii_lowercase();
    if !rendered_lower.starts_with(&first_word) {
        return false;
    }

    let normalized_rendered = normalize_whitespace(rendered);
    let normalized_sent = normalize_whitespace(sent_command);
    if normalized_rendered == normalized_sent || normalized_rendered.contains(&normalized_sent) {
        return true;
    }

    let rendered_echo = normalize_echo_text(rendered);
    let sent_echo = normalize_echo_text(sent_command);
    if rendered_echo.len() < first_word.len() || sent_echo.len() < 8 {
        return false;
    }

    let common = common_subsequence_len(&rendered_echo, &sent_echo);
    common * 100 / sent_echo.len().min(rendered_echo.len()) >= 70
}

fn rendered_line_is_command_echo_fragment(rendered_line: &str, sent_command: &str) -> bool {
    if rendered_line_is_command_echo(rendered_line, sent_command) {
        return true;
    }

    let first_word = normalized_command_first_word(sent_command);
    if first_word.len() < 2 {
        return false;
    }

    let rendered = rendered_line.trim();
    if rendered.is_empty() {
        return false;
    }

    let rendered_lower = rendered.to_ascii_lowercase();
    if !(rendered_lower.starts_with(&first_word) || first_word.starts_with(&rendered_lower)) {
        return false;
    }

    let normalized_rendered = normalize_whitespace(rendered);
    let normalized_sent = normalize_whitespace(sent_command);
    if normalized_sent.starts_with(&normalized_rendered) {
        return true;
    }

    let rendered_echo = normalize_echo_text(rendered);
    let sent_echo = normalize_echo_text(sent_command);
    if rendered_echo.is_empty() || sent_echo.len() < 8 {
        return false;
    }
    if sent_echo.starts_with(&rendered_echo) {
        return true;
    }

    let common = common_subsequence_len(&rendered_echo, &sent_echo);
    common * 100 / rendered_echo.len() >= 70
}

fn raw_line_has_command_redraw_marker(
    raw_line: &str,
    rendered_line: &str,
    sent_command: &str,
) -> bool {
    let first_word = normalized_command_first_word(sent_command);
    if first_word.len() < 2 || !raw_line.contains('$') {
        return false;
    }

    let rendered = rendered_line.trim().to_ascii_lowercase();
    rendered.starts_with(&first_word)
}

#[derive(Debug)]
struct StreamCommandEchoFilter {
    sent_command: String,
    active: bool,
    pending_echo_line: bool,
}

impl StreamCommandEchoFilter {
    fn new(sent_command: &str) -> Self {
        Self {
            sent_command: sent_command.to_string(),
            active: true,
            pending_echo_line: false,
        }
    }

    fn should_drop_line(&mut self, raw_line: &str) -> bool {
        if !self.active {
            return false;
        }

        if self.pending_echo_line {
            self.pending_echo_line = false;
            trace!(
                "Dropping pending command echo line from stream: controls={:?}, raw={raw_line:?}",
                terminal_control_summary(raw_line)
            );
            return true;
        }

        let rendered = terminal_render_line(raw_line);
        if rendered_line_is_command_echo(&rendered, &self.sent_command)
            || raw_line_has_command_redraw_marker(raw_line, &rendered, &self.sent_command)
        {
            trace!(
                "Dropping command echo line from stream: rendered={rendered:?}, controls={:?}, raw={raw_line:?}",
                terminal_control_summary(raw_line)
            );
            return true;
        }

        if !rendered.trim().is_empty() {
            self.active = false;
        }
        false
    }

    fn should_suppress_fragment(&mut self, raw_fragment: &str) -> bool {
        if !self.active {
            return false;
        }

        if self.pending_echo_line {
            return true;
        }

        let rendered = terminal_render_line(raw_fragment);
        let should_suppress = rendered_line_is_command_echo_fragment(&rendered, &self.sent_command)
            || raw_line_has_command_redraw_marker(raw_fragment, &rendered, &self.sent_command);
        if should_suppress {
            self.pending_echo_line = true;
            trace!(
                "Suppressing command echo fragment from stream: rendered={rendered:?}, controls={:?}, raw={raw_fragment:?}",
                terminal_control_summary(raw_fragment)
            );
        }
        should_suppress
    }
}

fn strip_leading_command_echo_lines(output: &str, sent_command: &str) -> String {
    let mut filter = StreamCommandEchoFilter::new(sent_command);
    let mut lines: Vec<&str> = output.split('\n').collect();

    while let Some(first_line) = lines.first() {
        if !filter.should_drop_line(first_line) {
            break;
        }
        lines.remove(0);
    }

    lines.join("\n")
}

fn extract_command_content(all: &str, sent_command: &str, prompt: Option<&str>) -> String {
    let raw_without_echo = strip_leading_command_echo_lines(all, sent_command);
    let normalized_all = normalize_runtime_output(&raw_without_echo);
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
        let mut pending_prompt_lines = Vec::new();
        let mut echo_filter = StreamCommandEchoFilter::new(&sent_command);

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
                        if echo_filter.should_drop_line(trim_start.as_ref()) {
                            continue;
                        }
                        let normalized_fragment = normalize_runtime_output(&trim_start);

                        if terminal_fragment_has_pua(&normalized_fragment)
                            || handler.read_prompt_prefix(&trim_start)
                        {
                            pending_prompt_lines.push(trim_start.to_string());
                            continue;
                        }

                        flush_pending_prompt_lines(
                            handler,
                            &mut pending_prompt_lines,
                            &mut clean_output,
                            &mut is_error,
                        );

                        let trimmed_line = trim_start.trim_end();
                        handler.read(trimmed_line);

                        if handler.error() {
                            is_error = true;
                        }

                        clean_output.push_str(&trim_start);
                    }

                    if !line_buffer.is_empty() && echo_filter.should_suppress_fragment(&line_buffer)
                    {
                        continue;
                    }

                    if let Some(prompt_candidate) =
                        merge_terminal_prompt_fragments(&pending_prompt_lines, Some(&line_buffer))
                        && handler.read_prompt(&prompt_candidate)
                    {
                        handler.read(&prompt_candidate);
                        let matched_prompt = handler
                            .current_prompt()
                            .unwrap_or(&prompt_candidate)
                            .to_string();
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

                    if !pending_prompt_lines.is_empty()
                        && line_buffer.is_empty()
                        && let Some(prompt_candidate) =
                            merge_terminal_prompt_fragments(&pending_prompt_lines, None)
                        && handler.read_prompt(&prompt_candidate)
                    {
                        handler.read(&prompt_candidate);
                        let matched_prompt = handler
                            .current_prompt()
                            .unwrap_or(&prompt_candidate)
                            .to_string();
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

                    if !line_buffer.is_empty() {
                        if handler.read_prompt(&line_buffer) {
                            flush_pending_prompt_lines(
                                handler,
                                &mut pending_prompt_lines,
                                &mut clean_output,
                                &mut is_error,
                            );
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
                            flush_pending_prompt_lines(
                                handler,
                                &mut pending_prompt_lines,
                                &mut clean_output,
                                &mut is_error,
                            );
                            handler.read(&line_buffer);
                            if !is_record {
                                line_buffer.clear();
                            }
                            trace!(
                                "Runtime input required: record_input={}, response_len={}",
                                is_record,
                                c.len()
                            );
                            self.sender.send(c).await?;
                        } else if let Some((c, is_record)) = handler.read_need_write(&line_buffer) {
                            flush_pending_prompt_lines(
                                handler,
                                &mut pending_prompt_lines,
                                &mut clean_output,
                                &mut is_error,
                            );
                            handler.read(&line_buffer);
                            if !is_record {
                                line_buffer.clear();
                            }
                            trace!(
                                "Template input required: record_input={}, response_len={}",
                                is_record,
                                c.len()
                            );
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
                let mut timeout_output = clean_output.clone();
                for pending_line in &pending_prompt_lines {
                    timeout_output.push_str(pending_line);
                }
                timeout_output.push_str(&line_buffer);
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
                        content: timeout_output.clone(),
                        all: timeout_output.clone(),
                    });
                }
                return Err(ConnectError::ExecTimeout(build_exec_timeout_message(
                    &timeout_output,
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
            let state_before_transition = self.handler.current_state().to_string();
            let before_exit_hooks = self
                .handler
                .hooks()
                .before_exit_state(&state_before_transition)
                .to_vec();
            self.run_hook_actions(
                HookTrigger::BeforeExitState(&state_before_transition),
                &before_exit_hooks,
                sys,
            )
            .await?;

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
            if current_state != last_state {
                let after_enter_hooks =
                    self.handler.hooks().after_enter_state(&current_state).to_vec();
                self.run_hook_actions(
                    HookTrigger::AfterEnterState(&current_state),
                    &after_enter_hooks,
                    sys,
                )
                .await?;

                if let Some(recorder) = self.recorder.as_ref() {
                    let _ = recorder.record_event(SessionEvent::StateChanged {
                        state: current_state.clone(),
                    });
                }
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
    fn runtime_command_interaction_matches_shared_sanitized_prompt_with_pua_placeholders() {
        let interaction = RuntimeCommandInteraction::build(&CommandInteraction {
            prompts: vec![PromptResponseRule::new(
                vec![r"^<PUA> adam-work ~ <PUA> \d{1,2}:\d{2}$".to_string()],
                "ok\n".to_string(),
            )],
        })
        .expect("build interaction");

        let prompt = concat!("", " adam-work ~ ", "", " 11:32");

        assert_eq!(
            interaction.read_need_write(prompt),
            Some(("ok\n".to_string(), false))
        );
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
    fn extract_command_content_strips_redrawn_wrapper_echo_line() {
        let sent_command = r#"date; printf '\n__RNETER_EXIT_CODE__:%s:__\n' "$?""#;
        let raw_output = concat!(
            "ddate; printf '\\n__RNETER_EXIT_CODE__:%s:__\\n' \"$?\"dateprintf '\\n__RNETER_EXIT_CODE__:%s:__\\n' \"$?\"\n",
            "2026年 04月 15日 星期三 15:20:10 CST\n",
            "<PUA> adam-work <PUA> ~ <PUA> <PUA> 15:20 <PUA> <PUA>"
        );

        let content = extract_command_content(
            raw_output,
            sent_command,
            Some("<PUA> adam-work <PUA> ~ <PUA> <PUA> 15:20 <PUA> <PUA>"),
        );
        assert_eq!(content, "2026年 04月 15日 星期三 15:20:10 CST");
    }

    #[test]
    fn extract_command_content_strips_wrapped_long_command_echo_noise() {
        let sent_command = "no object-group service OG_SVC_WEB tcp";
        let raw_output = concat!(
            "no object-group service OG_SVC_WEB tcp\n",
            "no object-group ser$p serv            ice OG_SVC_W$SVC_WE            B tcpno object-group se$\n",
            "Removing object-group (OG_SVC_WEB) not allowed, it is being used.\n",
            "ciscoasa-3# "
        );

        let content = extract_command_content(raw_output, sent_command, Some("ciscoasa-3# "));
        assert_eq!(
            content,
            "Removing object-group (OG_SVC_WEB) not allowed, it is being used."
        );
    }

    #[test]
    fn extract_command_content_strips_wrapped_network_object_group_echo_noise() {
        let sent_command = "no object-group network OG_DST_DB";
        let raw_output = concat!(
            "no object-group network OG_DST_DB\n",
            "no object-group net$p netw            ork OG_DST_D$DST_DB            no object-group ne$ \n",
            "Removing object-group (OG_DST_DB) not allowed, it is being used.\n",
            "ciscoasa-3# "
        );

        let content = extract_command_content(raw_output, sent_command, Some("ciscoasa-3# "));
        assert_eq!(
            content,
            "Removing object-group (OG_DST_DB) not allowed, it is being used."
        );
    }

    #[test]
    fn extract_command_content_strips_wrapped_echo_before_full_normalization() {
        let sent_command = "no object-group network OG_DST_DB";
        let raw_output = concat!(
            "no object-group network OG_DST_DB\n",
            "\u{1b}[31mno object-group net$p netw            ork OG_DST_D$DST_DB            no object-group ne$ \u{1b}[0m\n",
            "Removing object-group (OG_DST_DB) not allowed, it is being used.\n",
            "ciscoasa-3# "
        );

        let content = extract_command_content(raw_output, sent_command, Some("ciscoasa-3# "));
        assert_eq!(
            content,
            "Removing object-group (OG_DST_DB) not allowed, it is being used."
        );
    }

    #[test]
    fn terminal_render_line_applies_carriage_return_and_erase_line() {
        let rendered = terminal_render_line("abcdef\r\u{1b}[Kxy");

        assert_eq!(rendered, "xy");
    }

    #[test]
    fn terminal_control_summary_reports_cursor_and_erase_sequences() {
        let controls = terminal_control_summary("abcdef\r\u{1b}[Kxy\u{1b}[3D");

        assert_eq!(controls, vec!["CR", "CSI(K)", "CSI(3D)"]);
    }

    #[test]
    fn terminal_render_line_overwrites_after_carriage_return() {
        let rendered = terminal_render_line("abcdef\rxy");

        assert_eq!(rendered, "xycdef");
    }

    #[test]
    fn terminal_render_line_applies_backspace() {
        let rendered = terminal_render_line("abc\x08D");

        assert_eq!(rendered, "abD");
    }

    #[test]
    fn terminal_render_line_applies_csi_cursor_forward() {
        let rendered = terminal_render_line("ab\u{1b}[3CX");

        assert_eq!(rendered, "ab   X");
    }

    #[test]
    fn terminal_render_line_applies_csi_cursor_backward() {
        let rendered = terminal_render_line("abcdef\u{1b}[3DX");

        assert_eq!(rendered, "abcXef");
    }

    #[test]
    fn terminal_render_line_applies_csi_cursor_horizontal_absolute() {
        let rendered = terminal_render_line("abcdef\u{1b}[2GX");

        assert_eq!(rendered, "aXcdef");
    }

    #[test]
    fn terminal_render_line_applies_csi_erase_to_line_start() {
        let rendered = terminal_render_line("abcdef\u{1b}[1KX");

        assert_eq!(rendered, "      X");
    }

    #[test]
    fn terminal_render_line_applies_csi_erase_entire_line() {
        let rendered = terminal_render_line("abcdef\u{1b}[2Kxy");

        assert_eq!(rendered, "xy");
    }

    #[test]
    fn terminal_render_line_ignores_color_sequences() {
        let rendered = terminal_render_line("ab\u{1b}[31mcd\u{1b}[0mef");

        assert_eq!(rendered, "abcdef");
    }

    #[test]
    fn terminal_render_line_skips_osc_and_dcs_sequences() {
        let rendered = terminal_render_line(concat!(
            "ab",
            "\u{1b}]0;ignored title\u{7}",
            "cd",
            "\u{1b}Pignored payload\u{1b}\\",
            "ef"
        ));

        assert_eq!(rendered, "abcdef");
    }

    #[test]
    fn terminal_render_line_expands_tabs_to_next_four_column_stop() {
        let rendered = terminal_render_line("ab\tX");

        assert_eq!(rendered, "ab  X");
    }

    #[test]
    fn rendered_line_is_command_echo_matches_terminal_redraw_output() {
        let sent_command = "no object-group network OG_DST_DB";
        let rendered = terminal_render_line(
            "\r\u{1b}[Kno object-group net$p netw            ork OG_DST_D$DST_DB            no object-group ne$ ",
        );

        assert!(rendered_line_is_command_echo(&rendered, sent_command));
    }

    #[test]
    fn rendered_line_is_command_echo_fragment_matches_incremental_echo() {
        let sent_command = "no object-group network OG_SRC_APP";

        assert!(rendered_line_is_command_echo_fragment("n", sent_command));
        assert!(rendered_line_is_command_echo_fragment(
            "no object-group net",
            sent_command
        ));
        assert!(rendered_line_is_command_echo_fragment(
            "no object-group net$p netw            ork OG_SRC_A$SRC_AP            P",
            sent_command
        ));
    }

    #[test]
    fn rendered_line_is_command_echo_fragment_matches_service_redraw_echo() {
        let sent_command = "no object-group service OG_SVC_WEB tcp";

        assert!(rendered_line_is_command_echo_fragment(
            "no object-group ser$p serv            ",
            sent_command
        ));
        assert!(rendered_line_is_command_echo_fragment(
            "no object-group ser$p serv            ice OG_SVC_W$SVC_WE            B tcp",
            sent_command
        ));
    }

    #[test]
    fn stream_echo_filter_drops_full_service_redraw_echo_line() {
        let sent_command = "no object-group service OG_SVC_WEB tcp";
        let mut filter = StreamCommandEchoFilter::new(sent_command);

        assert!(filter.should_drop_line(
            "no object-group ser$p serv            ice OG_SVC_W$SVC_WE            B tcpno object-group se$\n"
        ));
    }

    #[test]
    fn rendered_line_is_command_echo_fragment_rejects_real_output() {
        let sent_command = "no object-group network OG_SRC_APP";

        assert!(!rendered_line_is_command_echo_fragment(
            "Removing object-group (OG_SRC_APP) not allowed, it is being used.",
            sent_command
        ));
    }

    #[test]
    fn rendered_line_is_command_echo_rejects_real_output_after_echo_phase() {
        let sent_command = "show running-config object-group service OG_SVC_WEB";
        let rendered = "object-group service OG_SVC_WEB tcp";

        assert!(!rendered_line_is_command_echo(rendered, sent_command));
    }

    #[test]
    fn stream_echo_filter_drops_rendered_command_echo_before_prompt_matching() {
        let sent_command = "no object-group network OG_DST_DB";
        let mut filter = StreamCommandEchoFilter::new(sent_command);

        assert!(filter.should_drop_line(
            "\r\u{1b}[Kno object-group net$p netw            ork OG_DST_D$DST_DB            no object-group ne$ \n"
        ));
        assert!(!filter.should_drop_line(
            "Removing object-group (OG_DST_DB) not allowed, it is being used.\n"
        ));
    }

    #[test]
    fn stream_echo_filter_suppresses_incremental_echo_fragments() {
        let sent_command = "no object-group network OG_SRC_APP";
        let mut filter = StreamCommandEchoFilter::new(sent_command);

        assert!(filter.should_suppress_fragment("no object-group net"));
        assert!(filter.should_suppress_fragment(
            "no object-group net$p netw            ork OG_SRC_A$SRC_AP            P"
        ));
        assert!(filter.should_drop_line(
            "no object-group net$p netw            ork OG_SRC_A$SRC_AP            P\n"
        ));
        assert!(!filter.should_suppress_fragment(
            "Removing object-group (OG_SRC_APP) not allowed, it is being used."
        ));
    }

    #[test]
    fn stream_echo_filter_suppresses_redraw_marker_fragments_during_echo_phase() {
        let sent_command = "no object-group service OG_SVC_WEB tcp extra-wrapper";
        let mut filter = StreamCommandEchoFilter::new(sent_command);

        assert!(
            filter.should_suppress_fragment("no object-group ser$p serv            ice OG_SVC_W")
        );
    }

    #[test]
    fn stream_echo_filter_keeps_suppressing_pending_echo_line_until_newline() {
        let sent_command = "no object-group service OG_SVC_WEB tcp";
        let mut filter = StreamCommandEchoFilter::new(sent_command);

        assert!(filter.should_suppress_fragment("no object-group ser"));
        assert!(filter.should_suppress_fragment("unexpected middle fragment that would not match"));
        assert!(filter.should_drop_line(
            "no object-group ser$p serv            ice OG_SVC_W$SVC_WE            B tcp\n"
        ));
        assert!(!filter.should_suppress_fragment(
            "Removing object-group (OG_SVC_WEB) not allowed, it is being used."
        ));
    }

    #[test]
    fn extract_command_content_keeps_real_output_that_only_shares_command_tokens() {
        let sent_command = "show running-config object-group service OG_SVC_WEB";
        let raw_output = concat!(
            "show running-config object-group service OG_SVC_WEB\n",
            "object-group service OG_SVC_WEB tcp\n",
            " service-object tcp destination eq www\n",
            "ciscoasa-3# "
        );

        let content = extract_command_content(raw_output, sent_command, Some("ciscoasa-3# "));
        assert_eq!(
            content,
            "object-group service OG_SVC_WEB tcp\n service-object tcp destination eq www"
        );
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

    #[test]
    fn normalize_runtime_output_keeps_private_use_placeholders() {
        let raw = concat!("", " adam-work ~ ", "", " 10:38");

        let output = normalize_runtime_output(raw);
        assert_eq!(output, "<PUA> adam-work ~ <PUA> 10:38");
    }

    #[test]
    fn exec_timeout_message_reports_shared_sanitized_output() {
        let raw = concat!("command output\n", "", " adam-work  ~   10:38  ");

        let message = build_exec_timeout_message(raw);
        assert_eq!(message, "command output\n<PUA> adam-work  ~   10:38  ");
    }
}
