use log::trace;

use super::{
    DeviceHandler, STRIP_CSI_ESCAPE, STRIP_DCS_ESCAPE, STRIP_OSC_ESCAPE, STRIP_SIMPLE_ESCAPE,
};

fn sanitize_terminal_line(line: &str) -> String {
    let without_osc = STRIP_OSC_ESCAPE.replace_all(line, "");
    let without_dcs = STRIP_DCS_ESCAPE.replace_all(without_osc.as_ref(), "");
    let without_csi = STRIP_CSI_ESCAPE.replace_all(without_dcs.as_ref(), "");
    let without_simple = STRIP_SIMPLE_ESCAPE.replace_all(without_csi.as_ref(), "");
    without_simple
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
        .collect()
}

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
        let sanitized_line = sanitize_terminal_line(line);
        trace!("Read line: '{:?}'", sanitized_line);
        let (state_index, state, catch) = self.line2state(&sanitized_line, true);
        trace!("Converted to state: '{:?}'", state);
        if self.ignore_error(&sanitized_line) {
            trace!("Ignoring error state");
            self.current_state_index = 0;
        } else {
            if self.match_prompt(state_index) {
                trace!("State captured value: '{:?}'", catch);
                self.sys = catch;
                self.current_prompt = Some(sanitized_line);
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
        let sanitized_line = sanitize_terminal_line(line);
        trace!("Checking if line is a prompt: '{:?}'", sanitized_line);
        let (index, _, _) = self.line2state(&sanitized_line, false);
        self.match_prompt(index)
    }

    /// Checks if a line matches a system-specific prompt pattern.
    pub fn read_sys_prompt(&mut self, line: &str) -> bool {
        let sanitized_line = sanitize_terminal_line(line);
        trace!(
            "Checking if line is a system prompt: '{:?}'",
            sanitized_line
        );
        let (index, _, _) = self.line2state(&sanitized_line, false);
        self.match_sys_prompt(index)
    }

    /// Checks if a line requires input and returns the input to send.
    pub fn read_need_write(&mut self, line: &str) -> Option<(String, bool)> {
        let sanitized_line = sanitize_terminal_line(line);
        trace!("Checking if input is required: '{:?}'", sanitized_line);
        let (_, input, _) = self.line2state(&sanitized_line, false);
        if let Some((is_dyn, s, is_record)) = self.input_map.get(input) {
            if *is_dyn {
                return self.dyn_param.get(s).map(|cmd| (cmd.clone(), *is_record));
            }
            return Some((s.clone(), *is_record));
        }
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

    /// Checks if the current state is an error state.
    pub fn error(&self) -> bool {
        self.current_state().eq("error")
    }
}

#[cfg(test)]
mod tests {
    use super::super::build_test_handler;
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
}
