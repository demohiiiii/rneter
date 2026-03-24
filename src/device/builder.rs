use std::collections::HashMap;

use regex::{Regex, RegexSet};

use super::{CommandExecutionStrategy, DeviceHandler, PRE_STATE};
use crate::error::ConnectError;

impl DeviceHandler {
    /// Checks if two DeviceHandlers are equivalent (used for connection parameter comparison).
    pub fn is_equivalent(&self, other: &DeviceHandler) -> bool {
        if self.all_states != other.all_states {
            return false;
        }

        if self.edges != other.edges {
            return false;
        }

        if self.input_map != other.input_map {
            return false;
        }

        if self.prompt_index != other.prompt_index {
            return false;
        }

        if self.sys_prompt_index != other.sys_prompt_index {
            return false;
        }

        if self.catch_map.len() != other.catch_map.len() {
            return false;
        }

        for (key, (_, group_name)) in &self.catch_map {
            if let Some((_, other_group_name)) = other.catch_map.get(key) {
                if group_name != other_group_name {
                    return false;
                }
            } else {
                return false;
            }
        }

        if self.regex_index_map != other.regex_index_map {
            return false;
        }

        if self.command_execution != other.command_execution {
            return false;
        }

        true
    }

    /// Creates a new `DeviceHandler` with the specified state machine configuration.
    ///
    /// # Arguments
    ///
    /// * `prompt` - List of (state_name, regex_patterns) for basic prompts
    /// * `prompt_with_sys` - List of (state_name, regex_pattern, capture_group) for prompts with system names
    /// * `write` - List of (state_name, input_config, regex_patterns) for states requiring input
    /// * `more_regex` - Regex patterns that match pagination prompts (e.g., "--More--")
    /// * `error_regex` - Regex patterns that match error messages
    /// * `edges` - State transition graph: (from, command, to, is_exit, needs_format)
    /// * `ignore_errors` - Regex patterns for errors that should be ignored
    /// * `dyn_param` - Dynamic parameters for command/input substitution
    #[allow(clippy::too_many_arguments)]
    pub fn new<I, S>(
        prompt: Vec<(String, I)>,
        prompt_with_sys: Vec<(String, S, String)>,
        write: Vec<(String, (bool, String, bool), I)>,
        more_regex: I,
        error_regex: I,
        edges: Vec<(String, String, String, bool, bool)>,
        ignore_errors: I,
        dyn_param: HashMap<String, String>,
    ) -> Result<DeviceHandler, ConnectError>
    where
        S: AsRef<str> + Clone,
        I: IntoIterator<Item = S>,
    {
        let mut all_states: Vec<String> = PRE_STATE
            .iter()
            .map(|s| s.to_string().to_ascii_lowercase())
            .collect();

        let mut regexs: Vec<String> = Vec::new();
        let mut regex_index_map = HashMap::new();

        let start_offset = regexs.len();
        regexs.extend(more_regex.into_iter().map(|s| s.as_ref().to_string()));
        for i in start_offset..regexs.len() {
            regex_index_map.insert(i, 1);
        }

        let start_offset = regexs.len();
        regexs.extend(error_regex.into_iter().map(|s| s.as_ref().to_string()));
        for i in start_offset..regexs.len() {
            regex_index_map.insert(i, 2);
        }

        let mut prompt_patterns: Vec<(String, String)> = Vec::new();

        for (state, regex_iter) in prompt {
            let normalized_state = state.to_ascii_lowercase();
            let state_index = all_states.len();
            all_states.push(normalized_state.clone());

            let start_offset = regexs.len();
            let modified_regexs = regex_iter
                .into_iter()
                .map(|s| format!(r"^\x00*\r{{0,1}}{}", s.as_ref().trim_start_matches('^')))
                .collect::<Vec<_>>();

            for pattern in &modified_regexs {
                prompt_patterns.push((normalized_state.clone(), pattern.clone()));
            }
            regexs.extend(modified_regexs);

            for i in start_offset..regexs.len() {
                regex_index_map.insert(i, state_index);
            }
        }

        let mut catch_map = HashMap::new();
        let sys_prompt_state_index = all_states.len();

        for (state, regex, catch) in prompt_with_sys {
            let normalized_state = state.to_ascii_lowercase();
            let state_index = all_states.len();
            all_states.push(normalized_state.clone());

            let start_offset = regexs.len();
            let modified_regex =
                format!(r"^\x00*\r{{0,1}}{}", regex.as_ref().trim_start_matches('^'));

            let regex = Regex::new(&modified_regex).map_err(|err| {
                ConnectError::InvalidDeviceHandlerConfig(format!(
                    "invalid prompt_with_sys regex for state '{}': {}",
                    state, err
                ))
            })?;
            catch_map.insert(start_offset, (regex, catch));

            prompt_patterns.push((normalized_state, modified_regex.clone()));
            regexs.push(modified_regex);

            regex_index_map.insert(start_offset, state_index);
        }

        let sys_prompt_index = (sys_prompt_state_index, all_states.len() - 1);
        let prompt_index = (3, all_states.len() - 1);

        let mut input_map = HashMap::new();
        for (state, cmd, regex_iter) in write {
            let state_index = all_states.len();
            all_states.push(state.to_ascii_lowercase());

            let start_offset = regexs.len();
            regexs.extend(regex_iter.into_iter().map(|s| s.as_ref().to_string()));

            input_map.insert(state.to_ascii_lowercase(), cmd);

            for i in start_offset..regexs.len() {
                regex_index_map.insert(i, state_index);
            }
        }

        input_map.insert("more".to_string(), (false, " ".to_string(), false));

        let all_regex = RegexSet::new(&regexs).map_err(|err| {
            ConnectError::InvalidDeviceHandlerConfig(format!(
                "failed to build state regex set: {}",
                err
            ))
        })?;

        let mut ignore_iter = ignore_errors.into_iter().peekable();
        let ignore_errors = if ignore_iter.peek().is_none() {
            None
        } else {
            Some(RegexSet::new(ignore_iter).map_err(|err| {
                ConnectError::InvalidDeviceHandlerConfig(format!(
                    "invalid ignore_errors regex set: {}",
                    err
                ))
            })?)
        };

        let edges = edges
            .iter()
            .map(|(start, cmd, end, exit, format)| {
                (
                    start.to_ascii_lowercase(),
                    cmd.clone(),
                    end.to_ascii_lowercase(),
                    *exit,
                    *format,
                )
            })
            .collect();

        Ok(Self {
            current_state_index: 0,
            prompt_index,
            sys_prompt_index,
            all_states,
            all_regex,
            regex_index_map,
            input_map,
            edges,
            ignore_errors,
            dyn_param,
            catch_map,
            sys: None,
            current_prompt: None,
            prompt_patterns,
            command_execution: CommandExecutionStrategy::PromptDriven,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::DeviceHandler;
    use crate::error::ConnectError;

    #[test]
    fn invalid_handler_regex_returns_config_error() {
        let err = match DeviceHandler::new(
            vec![("Login".to_string(), vec![r"["])],
            vec![],
            vec![],
            vec![r"^--More--$"],
            vec![r"^ERROR: .+$"],
            vec![],
            vec![],
            HashMap::new(),
        ) {
            Ok(_) => panic!("invalid regex should fail handler construction"),
            Err(err) => err,
        };

        match err {
            ConnectError::InvalidDeviceHandlerConfig(msg) => {
                assert!(msg.contains("failed to build state regex set"));
            }
            other => panic!("unexpected error type: {other}"),
        }
    }
}
