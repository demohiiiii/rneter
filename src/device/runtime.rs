use log::trace;

use super::{DeviceHandler, latest_terminal_fragment, sanitize_terminal_text};

impl DeviceHandler {
    /// Converts a line of output to a state.
    ///
    /// Matches the line against all known regex patterns and returns the corresponding state.
    /// If no match is found, defaults to the "Output" state.
    fn line2state(&self, line: &str, need_catch: bool) -> (usize, &str, Option<String>) {
        let matches: Vec<_> = self.all_regex.matches(line).into_iter().collect();
        if matches.is_empty() {
            let state = self
                .all_states
                .first()
                .map(|s| s.as_str())
                .unwrap_or("output");
            return (0, state, None);
        }

        let mut current_state_catch = None;
        let index = match matches.first() {
            Some(v) => *v,
            None => {
                let state = self
                    .all_states
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or("output");
                return (0, state, None);
            }
        };

        if need_catch
            && let Some((regex, catch)) = self.catch_map.get(&index)
            && let Some(caps) = regex.captures(line)
        {
            current_state_catch = caps.name(catch).map(|s| s.as_str().to_string());
        }

        let state_index = self.regex_index_map.get(&index).copied().unwrap_or(0);
        let state = self
            .all_states
            .get(state_index)
            .map(|s| s.as_str())
            .unwrap_or("output");
        (state_index, state, current_state_catch)
    }

    /// Reads a line of output and updates the current state.
    pub fn read(&mut self, line: &str) {
        let sanitized_line = sanitize_terminal_text(line);
        let prompt_line = latest_terminal_fragment(&sanitized_line);
        trace!("Read line: '{:?}'", prompt_line);
        let (state_index, state, catch) = self.line2state(prompt_line, true);
        trace!("Converted to state: '{:?}'", state);
        if self.ignore_error(prompt_line) {
            trace!("Ignoring error state");
            self.current_state_index = 0;
        } else {
            if self.match_prompt(state_index) {
                trace!("State captured value: '{:?}'", catch);
                self.sys = catch;
                self.current_prompt = Some(prompt_line.to_string());
            }

            self.current_state_index = state_index;
        }
    }

    fn ignore_error(&self, line: &str) -> bool {
        self.ignore_errors
            .as_ref()
            .map(|set| set.is_match(line))
            .unwrap_or(false)
    }

    fn match_prompt(&self, index: usize) -> bool {
        let (start, end) = self.prompt_index;
        index >= start && index <= end
    }

    pub(super) fn match_sys_prompt(&self, index: usize) -> bool {
        let (start, end) = self.sys_prompt_index;
        index >= start && index <= end
    }

    /// Checks if a line matches a prompt pattern.
    pub fn read_prompt(&mut self, line: &str) -> bool {
        let sanitized_line = sanitize_terminal_text(line);
        let prompt_line = latest_terminal_fragment(&sanitized_line);
        trace!("Checking if line is a prompt: '{:?}'", prompt_line);
        let (index, _, _) = self.line2state(prompt_line, false);
        self.match_prompt(index)
    }

    /// Checks if a complete line should be held and matched with a following prompt.
    pub fn read_prompt_prefix(&self, line: &str) -> bool {
        let Some(prompt_prefix_regex) = self.prompt_prefix_regex.as_ref() else {
            return false;
        };

        let sanitized_line = sanitize_terminal_text(line);
        let prompt_line = latest_terminal_fragment(&sanitized_line).trim_end();
        trace!("Checking if line is a prompt prefix: '{:?}'", prompt_line);
        prompt_prefix_regex.is_match(prompt_line)
    }

    /// Checks if a line matches a system-specific prompt pattern.
    pub fn read_sys_prompt(&mut self, line: &str) -> bool {
        let sanitized_line = sanitize_terminal_text(line);
        let prompt_line = latest_terminal_fragment(&sanitized_line);
        trace!("Checking if line is a system prompt: '{:?}'", prompt_line);
        let (index, _, _) = self.line2state(prompt_line, false);
        self.match_sys_prompt(index)
    }

    /// Checks if a line requires input and returns the input to send.
    pub fn read_need_write(&mut self, line: &str) -> Option<(String, bool)> {
        let sanitized_line = sanitize_terminal_text(line);
        let prompt_line = latest_terminal_fragment(&sanitized_line);
        trace!("Checking if input is required: '{:?}'", prompt_line);
        let (_, input, _) = self.line2state(prompt_line, false);
        if let Some((is_dyn, s, is_record)) = self.input_map.get(input) {
            if *is_dyn {
                if let Some(cmd) = self.dyn_param.get(s) {
                    trace!(
                        "Input rule matched dynamic response: state='{}', key='{}', record_input={}, response_len={}",
                        input,
                        s,
                        is_record,
                        cmd.len()
                    );
                    return Some((cmd.clone(), *is_record));
                }

                let available_keys: Vec<_> = self.dyn_param.keys().cloned().collect();
                trace!(
                    "Input rule matched but dynamic response is missing: state='{}', key='{}', available_dyn_keys={:?}",
                    input, s, available_keys
                );
                return None;
            }
            trace!(
                "Input rule matched static response: state='{}', record_input={}, response_len={}",
                input,
                is_record,
                s.len()
            );
            return Some((s.clone(), *is_record));
        }
        trace!(
            "No input rule matched: derived_state='{}', prompt_fragment={:?}",
            input, prompt_line
        );
        None
    }

    /// Returns the current state name.
    pub fn current_state(&self) -> &str {
        self.all_states
            .get(self.current_state_index)
            .map(|s| s.as_str())
            .unwrap_or("output")
    }

    /// Returns the currently captured system name, if available.
    pub fn current_sys(&self) -> Option<&str> {
        self.sys.as_deref()
    }

    /// Returns last prompt text matched by the state machine.
    pub fn current_prompt(&self) -> Option<&str> {
        self.current_prompt.as_deref()
    }

    /// Returns all declared state names.
    pub fn states(&self) -> Vec<String> {
        self.all_states.clone()
    }

    /// Returns all configured transition edges.
    pub fn edges(&self) -> Vec<(String, String, String, bool, bool)> {
        self.edges.clone()
    }

    /// Returns the normalized hook configuration retained on this handler.
    pub fn hooks(&self) -> &crate::session::SessionHooks {
        &self.hooks
    }

    /// Checks if the current state is an error state.
    pub fn error(&self) -> bool {
        self.current_state().eq("error")
    }
}

#[cfg(test)]
mod tests {
    use super::super::build_test_handler;
    use crate::device::normalize_terminal_output;
    use crate::templates;

    #[test]
    fn error_state_is_detected_after_error_line() {
        let mut handler = build_test_handler();

        assert!(!handler.error());
        handler.read("ERROR: invalid command");
        assert!(handler.error());
    }

    #[test]
    fn ignore_error_pattern_resets_to_output_state() {
        let mut handler = build_test_handler();

        handler.read("ERROR: benign");
        assert_eq!(handler.current_state(), "output");
        assert!(!handler.error());
    }

    #[test]
    fn current_prompt_is_updated_when_prompt_line_is_read() {
        let mut handler = build_test_handler();
        assert_eq!(handler.current_prompt(), None);

        handler.read("dev#");
        assert_eq!(handler.current_prompt(), Some("dev#"));
    }

    #[test]
    fn read_need_write_supports_dynamic_and_static_inputs() {
        let mut handler = build_test_handler();

        assert_eq!(
            handler.read_need_write("Password:"),
            Some(("secret\n".to_string(), true))
        );
        assert_eq!(
            handler.read_need_write("[y/n]?"),
            Some(("y".to_string(), false))
        );
        assert_eq!(handler.read_need_write("no input"), None);
    }

    #[test]
    fn linux_prompt_matches_after_stripping_ansi_sequences() {
        let mut handler = templates::linux().expect("create linux template");
        let raw_prompt = "\u{1b}]0;root@test-65:~\u{7}\u{1b}[?1034h[root@test-65 ~]# ";

        assert!(handler.read_prompt(raw_prompt));
        handler.read(raw_prompt);
        assert_eq!(handler.current_state(), "root");
        assert_eq!(handler.current_prompt(), Some("[root@test-65 ~]# "));
    }

    #[test]
    fn fish_prompt_matches_after_stripping_terminal_probe_sequences() {
        let mut handler = templates::linux().expect("create linux template");
        let raw_prompt = "\u{1b}[?u\u{1b}[>0q\u{1b}[?1049h\u{1b}P+q696e646e\u{1b}\\\u{1b}[?1049l\u{1b}[0c\u{1b}]133;A;click_events=1\u{7}\u{1b}[92mroot\u{1b}[m@\u{1b}[33m192-168-30-92\u{1b}[m \u{1b}[31m~\u{1b}[m# ";

        assert!(handler.read_prompt(raw_prompt));
        handler.read(raw_prompt);
        assert_eq!(handler.current_state(), "root");
        assert_eq!(handler.current_prompt(), Some("root@192-168-30-92 ~# "));
    }

    #[test]
    fn fish_prompt_matches_last_carriage_return_fragment() {
        let mut handler = templates::linux().expect("create linux template");
        let raw_prompt =
            "noise-before\r\u{1b}>\u{1b}[92m[192-168-30]\u{1b}[m \u{1b}[31m~\u{1b}[m# ";

        assert!(handler.read_prompt(raw_prompt));
        handler.read(raw_prompt);
        assert_eq!(handler.current_state(), "root");
        assert_eq!(handler.current_prompt(), Some("[192-168-30] ~# "));
    }

    #[test]
    fn prompt_rules_can_match_pua_placeholders_after_shared_sanitization() {
        let mut handler = crate::device::DeviceHandler::new(crate::device::DeviceHandlerConfig {
            prompt: vec![crate::device::prompt_rule(
                "User",
                &[r"^<PUA>\s+adam-work\s+<PUA>\s+~\s+<PUA>\s+\d{1,2}:\d{2}\s+<PUA>\s+<PUA>$"],
            )],
            ..Default::default()
        })
        .expect("build handler");

        let raw_prompt = concat!(
            "",
            " adam-work ",
            "",
            " ~ ",
            "",
            "",
            " 11:32 ",
            "",
            " "
        );

        assert!(handler.read_prompt(raw_prompt));
        handler.read(raw_prompt);
        assert_eq!(handler.current_state(), "user");
    }

    #[test]
    fn shared_normalization_replaces_private_use_with_placeholder() {
        let normalized = normalize_terminal_output(concat!("󰌽", " adam-work ", ""));
        assert_eq!(normalized, "<PUA> adam-work <PUA>");
    }
}
