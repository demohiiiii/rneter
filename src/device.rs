//! Device state machine handler for network devices.
//!
//! This module provides a sophisticated state machine implementation for managing
//! network device interactions through SSH. It handles prompt detection, automatic
//! state transitions, and intelligent command routing based on the current device state.

use std::collections::{HashMap, HashSet, VecDeque};

use log::trace;
use once_cell::sync::Lazy;
use regex::{Regex, RegexSet};

use crate::error::ConnectError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub struct DeviceHandler {
    /// Index of the current state in the `all_states` vector
    current_state_index: usize,

    /// All possible states the device can be in
    all_states: Vec<String>,

    /// Combined regex set for matching all state patterns
    all_regex: RegexSet,

    /// Maps regex match index to state index
    regex_index_map: HashMap<usize, usize>,

    /// Index range for prompt states in `all_states` (start, end)
    prompt_index: (usize, usize),

    /// Index range for system-specific prompts in `all_states` (start, end)
    sys_prompt_index: (usize, usize),

    /// Maps state to input requirements:
    /// - bool: whether the value is dynamic (from `dyn_param`)
    /// - String: the input value or key in `dyn_param`
    /// - bool: whether to record this input in the output
    input_map: HashMap<String, (bool, String, bool)>,

    /// State transition graph: (from_state, command, to_state, is_exit, needs_format)
    /// Used for pathfinding during active state transitions
    edges: Vec<(String, String, String, bool, bool)>,

    /// Regex patterns for errors that should be ignored
    ignore_errors: Option<RegexSet>,

    /// Dynamic parameters for input substitution (e.g., passwords, system names)
    pub dyn_param: HashMap<String, String>,

    /// Maps state index to (regex, capture_group_name) for extracting values from prompts
    catch_map: HashMap<usize, (Regex, String)>,

    /// Captured system name from the prompt (e.g., hostname)
    sys: Option<String>,

    /// Last prompt text matched by the state machine.
    current_prompt: Option<String>,

    /// Prompt regex patterns grouped by state (for diagnostics).
    prompt_patterns: Vec<(String, String)>,
}

type ExitPath = Option<(String, Vec<(String, String)>)>;

/// Diagnostics summary for a device state machine graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineDiagnostics {
    /// Number of declared states.
    pub total_states: usize,
    /// States participating in transition graph edges.
    pub graph_states: Vec<String>,
    /// Graph entry states (in-degree = 0).
    pub entry_states: Vec<String>,
    /// Edge sources that do not exist in declared states.
    pub missing_edge_sources: Vec<String>,
    /// Edge targets that do not exist in declared states.
    pub missing_edge_targets: Vec<String>,
    /// Graph states unreachable from entry states.
    pub unreachable_states: Vec<String>,
    /// Graph states with no outgoing edges.
    pub dead_end_states: Vec<String>,
    /// Prompt regex patterns shared by multiple states.
    pub duplicate_prompt_patterns: Vec<String>,
    /// States participating in duplicate prompt-pattern groups.
    pub potentially_ambiguous_prompt_states: Vec<String>,
    /// States whose outgoing transitions are only self-loop edges.
    pub self_loop_only_states: Vec<String>,
}

impl StateMachineDiagnostics {
    /// Returns true if diagnostics indicate potential template issues.
    pub fn has_issues(&self) -> bool {
        !self.missing_edge_sources.is_empty()
            || !self.missing_edge_targets.is_empty()
            || !self.unreachable_states.is_empty()
            || !self.dead_end_states.is_empty()
            || !self.duplicate_prompt_patterns.is_empty()
            || !self.self_loop_only_states.is_empty()
    }
}

/// Predefined states that exist in every device handler.
static PRE_STATE: Lazy<Vec<String>> = Lazy::new(|| {
    vec![
        "Output".to_string(),
        "More".to_string(),
        "Error".to_string(),
    ]
});

impl DeviceHandler {
    /// Checks if two DeviceHandlers are equivalent (used for connection parameter comparison).
    pub fn is_equivalent(&self, other: &DeviceHandler) -> bool {
        // Compare core state configurations
        if self.all_states != other.all_states {
            return false;
        }

        // Compare edge configurations (state transition graph)
        if self.edges != other.edges {
            return false;
        }

        // Compare input map
        if self.input_map != other.input_map {
            return false;
        }

        // Compare prompt index range
        if self.prompt_index != other.prompt_index {
            return false;
        }

        if self.sys_prompt_index != other.sys_prompt_index {
            return false;
        }

        // Compare catch map (compare keys and capture group names only)
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

        // Compare regex index map
        if self.regex_index_map != other.regex_index_map {
            return false;
        }

        // Note: We do not compare current_state_index, dyn_param, sys, etc., as these
        // are runtime states that change during the connection.
        // We also don't compare regex content directly because RegexSet doesn't support comparison.

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

        // Change from Vec<S> to Vec<String> to support modification
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
            // The new state's index is the current length of all_states
            let normalized_state = state.to_ascii_lowercase();
            let state_index = all_states.len();
            all_states.push(normalized_state.clone());

            let start_offset = regexs.len();
            // Automatically prepend the common prefix to prompt regexes
            // We strip any existing leading '^' to avoid duplication
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
            // The new state's index is the current length of all_states
            let normalized_state = state.to_ascii_lowercase();
            let state_index = all_states.len();
            all_states.push(normalized_state.clone());

            let start_offset = regexs.len();

            // Prepend prefix to prompt_with_sys regex as well
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
            // The new state's index is the current length of all_states
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
        })
    }

    /// Converts a line of output to a state.
    ///
    /// Matches the line against all known regex patterns and returns the corresponding state.
    /// If no match is found, defaults to the "Output" state.
    ///
    /// # Arguments
    ///
    /// * `line` - The line to match
    /// * `need_catch` - Whether to capture values from the line (e.g., hostname)
    ///
    /// # Returns
    ///
    /// A tuple of (state_index, state_name, captured_value)
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
    ///
    /// This method should be called for each line of output received from the device.
    /// It automatically updates the internal state and captures values from prompts
    /// when configured to do so.
    ///
    /// # Arguments
    ///
    /// * `line` - A line of output from the device
    pub fn read(&mut self, line: &str) {
        trace!("Read line: '{:?}'", line);
        let (state_index, state, catch) = self.line2state(line, true);
        trace!("Converted to state: '{:?}'", state);
        if self.ignore_error(line) {
            trace!("Ignoring error state");
            self.current_state_index = 0;
        } else {
            if self.match_prompt(state_index) {
                trace!("State captured value: '{:?}'", catch);
                self.sys = catch;
                self.current_prompt = Some(line.to_string());
            }

            self.current_state_index = state_index;
        }
    }

    /// Checks if a line matches an error pattern that should be ignored.
    fn ignore_error(&self, line: &str) -> bool {
        self.ignore_errors
            .as_ref()
            .map(|set| set.is_match(line))
            .unwrap_or(false)
    }

    /// Checks if a state index corresponds to a prompt state.
    fn match_prompt(&self, index: usize) -> bool {
        let (start, end) = self.prompt_index;
        index >= start && index <= end
    }

    /// Checks if a state index corresponds to a system-specific prompt state.
    fn match_sys_prompt(&self, index: usize) -> bool {
        let (start, end) = self.sys_prompt_index;
        index >= start && index <= end
    }

    /// Checks if a line matches a prompt pattern.
    ///
    /// This is useful for detecting when the device is ready to accept the next command.
    ///
    /// # Arguments
    ///
    /// * `line` - The line to check
    ///
    /// # Returns
    ///
    /// `true` if the line matches any prompt pattern, `false` otherwise
    pub fn read_prompt(&mut self, line: &str) -> bool {
        trace!("Checking if line is a prompt: '{:?}'", line);
        let (index, _, _) = self.line2state(line, false);
        self.match_prompt(index)
    }

    /// Checks if a line matches a system-specific prompt pattern.
    ///
    /// # Arguments
    ///
    /// * `line` - The line to check
    ///
    /// # Returns
    ///
    /// `true` if the line matches a system-specific prompt, `false` otherwise
    pub fn read_sys_prompt(&mut self, line: &str) -> bool {
        trace!("Checking if line is a system prompt: '{:?}'", line);
        let (index, _, _) = self.line2state(line, false);
        self.match_sys_prompt(index)
    }

    /// Checks if a line requires input and returns the input to send.
    ///
    /// Some device states require automatic input (e.g., password prompts, confirmation).
    /// This method checks if the current line triggers such a state and returns the
    /// appropriate input.
    ///
    /// # Arguments
    ///
    /// * `line` - The line to check
    ///
    /// # Returns
    ///
    /// `Some((input, should_record))` if input is required, where:
    /// - `input` is the string to send
    /// - `should_record` indicates whether this input should be included in the output
    ///
    /// `None` if no input is required
    pub fn read_need_write(&mut self, line: &str) -> Option<(String, bool)> {
        trace!("Checking if input is required: '{:?}'", line);
        let (_, input, _) = self.line2state(line, false);
        if let Some((is_dyn, s, is_record)) = self.input_map.get(input) {
            if *is_dyn {
                return self.dyn_param.get(s).map(|cmd| (cmd.clone(), *is_record));
            }
            return Some((s.clone(), *is_record));
        }
        None
    }

    /// Returns the current state name.
    ///
    /// # Returns
    ///
    /// The name of the current state
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

    /// Analyze the state transition graph for common template issues.
    pub fn diagnose_state_machine(&self) -> StateMachineDiagnostics {
        let all_states_set: HashSet<String> = self.all_states.iter().cloned().collect();
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut out_degree: HashMap<String, usize> = HashMap::new();
        let mut graph_states_set: HashSet<String> = HashSet::new();
        let mut missing_edge_sources = HashSet::new();
        let mut missing_edge_targets = HashSet::new();

        for (from, _cmd, to, _is_exit, _needs_format) in &self.edges {
            if !all_states_set.contains(from) {
                missing_edge_sources.insert(from.clone());
                continue;
            }
            if !all_states_set.contains(to) {
                missing_edge_targets.insert(to.clone());
                continue;
            }

            graph_states_set.insert(from.clone());
            graph_states_set.insert(to.clone());

            adjacency.entry(from.clone()).or_default().push(to.clone());
            *out_degree.entry(from.clone()).or_insert(0) += 1;
            *in_degree.entry(to.clone()).or_insert(0) += 1;
            in_degree.entry(from.clone()).or_insert(0);
            out_degree.entry(to.clone()).or_insert(0);
        }

        let mut graph_states = graph_states_set.into_iter().collect::<Vec<_>>();
        graph_states.sort();

        let mut entry_states = graph_states
            .iter()
            .filter(|state| in_degree.get(*state).copied().unwrap_or(0) == 0)
            .cloned()
            .collect::<Vec<_>>();
        entry_states.sort();

        // Fallback: if no root-like node exists (fully cyclic graph), pick stable seed.
        let seeds = if entry_states.is_empty() {
            graph_states
                .first()
                .cloned()
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            entry_states.clone()
        };

        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        for seed in seeds {
            if reachable.insert(seed.clone()) {
                queue.push_back(seed);
            }
        }
        while let Some(node) = queue.pop_front() {
            if let Some(neighbors) = adjacency.get(&node) {
                for next in neighbors {
                    if reachable.insert(next.clone()) {
                        queue.push_back(next.clone());
                    }
                }
            }
        }

        let mut unreachable_states = graph_states
            .iter()
            .filter(|state| !reachable.contains(*state))
            .cloned()
            .collect::<Vec<_>>();
        unreachable_states.sort();

        let mut dead_end_states = graph_states
            .iter()
            .filter(|state| out_degree.get(*state).copied().unwrap_or(0) == 0)
            .cloned()
            .collect::<Vec<_>>();
        dead_end_states.sort();

        let mut missing_edge_sources = missing_edge_sources.into_iter().collect::<Vec<_>>();
        missing_edge_sources.sort();
        let mut missing_edge_targets = missing_edge_targets.into_iter().collect::<Vec<_>>();
        missing_edge_targets.sort();

        let mut duplicate_prompt_patterns = Vec::new();
        let mut ambiguous_states = HashSet::new();
        let mut pattern_states: HashMap<String, HashSet<String>> = HashMap::new();
        for (state, pattern) in &self.prompt_patterns {
            pattern_states
                .entry(pattern.clone())
                .or_default()
                .insert(state.clone());
        }
        for (pattern, states) in pattern_states {
            if states.len() > 1 {
                let mut states_vec = states.into_iter().collect::<Vec<_>>();
                states_vec.sort();
                for state in &states_vec {
                    ambiguous_states.insert(state.clone());
                }
                duplicate_prompt_patterns.push(format!("{pattern} => {}", states_vec.join(",")));
            }
        }
        duplicate_prompt_patterns.sort();
        let mut potentially_ambiguous_prompt_states =
            ambiguous_states.into_iter().collect::<Vec<_>>();
        potentially_ambiguous_prompt_states.sort();

        let mut self_loop_only_states = graph_states
            .iter()
            .filter(|state| {
                let outs = adjacency.get(*state);
                out_degree.get(*state).copied().unwrap_or(0) > 0
                    && outs
                        .map(|targets| targets.iter().all(|target| target == *state))
                        .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();
        self_loop_only_states.sort();

        StateMachineDiagnostics {
            total_states: self.all_states.len(),
            graph_states,
            entry_states,
            missing_edge_sources,
            missing_edge_targets,
            unreachable_states,
            dead_end_states,
            duplicate_prompt_patterns,
            potentially_ambiguous_prompt_states,
            self_loop_only_states,
        }
    }

    /// Checks if the current state is an error state.
    ///
    /// # Returns
    ///
    /// `true` if the device is currently in an error state, `false` otherwise
    pub fn error(&self) -> bool {
        // All states are normalized to lowercase during handler construction.
        self.current_state().eq("error")
    }

    /// Finds the path to exit from system-specific prompts.
    ///
    /// When in a system-specific state (e.g., a specific configuration context),
    /// this method finds the sequence of commands needed to exit to a non-system state.
    fn exit_until_no_sys(&self, sys: Option<&String>) -> Result<ExitPath, ConnectError> {
        if !self.match_sys_prompt(self.current_state_index) {
            return Ok(None);
        }
        let exit_edges = self.edges.iter().filter(|(_, _, _, exit, _)| *exit);
        let mut edge_map = HashMap::new();
        for (start, cmd, end, _, format) in exit_edges {
            edge_map.insert(start, (cmd, end, format));
        }
        let mut path = Vec::new();
        let mut current = &self.current_state().to_string();
        loop {
            if let Some((cmd, end, format)) = edge_map.get(current) {
                path.push((
                    Self::format_cmd(**format, cmd, sys.map(|s| s.as_str())),
                    (*end).to_string(),
                ));
                if let Some(index) = self.all_states.iter().position(|v| v.eq(*end)) {
                    if !self.match_sys_prompt(index) {
                        return Ok(Some(((*end).to_string(), path)));
                    }
                    current = *end;
                } else {
                    return Err(ConnectError::TargetStateNotExistError);
                }
            } else {
                return Err(ConnectError::NoExitCommandError(current.clone()));
            }
        }
    }

    /// Formats a command string with system name substitution.
    ///
    /// If `format` is true, replaces "{}" in the command with the system name.
    fn format_cmd(format: bool, cmd: &str, sys: Option<&str>) -> String {
        if format {
            if let Some(s) = sys {
                cmd.replace("{}", s)
            } else {
                String::new()
            }
        } else {
            cmd.to_string()
        }
    }

    /// Calculates the commands needed to transition to a target state.
    ///
    /// This method uses breadth-first search (BFS) to find the shortest path from  
    /// the current state to the target state in the state transition graph.
    ///
    /// # Arguments
    ///
    /// * `state` - The target state name
    /// * `sys` - Optional system name for state-specific transitions
    ///
    /// # Returns
    ///
    /// A vector of (command, target_state) pairs representing the path to take,
    /// or an error if the target state is unreachable.
    ///
    /// # Errors
    ///
    /// Returns `ConnectError::UnreachableState` if there's no path to the target state.
    pub fn trans_state_write(
        &self,
        state: &str,
        sys: Option<&String>,
    ) -> Result<Vec<(String, String)>, ConnectError> {
        let mut start_node = self.current_state().to_string();

        let end_node = state;

        let mut switch_path = Vec::new();

        if let (Some(current_sys), Some(target_sys)) = (&self.sys, sys)
            && current_sys != target_sys
        {
            trace!("Need to switch system: {} to {}", current_sys, target_sys);
            if let Some((node, exit_path)) = self.exit_until_no_sys(sys)? {
                start_node = node;
                switch_path.extend(exit_path);
            }
        }

        // If start and end are the same, the path is empty
        if start_node == end_node {
            return Ok(switch_path);
        }

        // --- 1. Build adjacency list ---
        // Use HashMap to represent the graph, where keys are nodes and values are
        // lists of all outgoing edges `(neighbor_node, edge_label)`.
        let mut adj_list: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (from, label, to, _, format) in &self.edges {
            adj_list.entry(from.clone()).or_default().push((
                to.clone(),
                Self::format_cmd(*format, label, sys.map(|s| s.as_str())),
            ));
        }

        // --- 2. Initialize BFS data structures ---
        // Queue for storing nodes to visit
        let mut queue = VecDeque::new();
        queue.push_back(start_node.clone());

        // Set of visited nodes to prevent revisits and infinite loops
        let mut visited = HashSet::new();
        visited.insert(start_node.clone());

        // Predecessor map for backtracking the path.
        // Key: child node, Value: (parent node, edge label connecting them)
        let mut predecessors: HashMap<String, (String, String)> = HashMap::new();

        // --- 3. Start BFS loop ---
        while let Some(current_node) = queue.pop_front() {
            trace!("Current node: '{:?}'", current_node);
            // If current node is the destination, we've found the shortest path, stop searching
            if current_node == end_node {
                break;
            }

            // Iterate through all neighbors of the current node
            if let Some(neighbors) = adj_list.get(&current_node) {
                for (neighbor_node, edge_label) in neighbors {
                    // If the neighbor node hasn't been visited
                    if !visited.contains(neighbor_node) {
                        // Mark as visited
                        visited.insert(neighbor_node.clone());
                        // Record its predecessor node and the edge that led to this discovery
                        predecessors.insert(
                            neighbor_node.clone(),
                            (current_node.clone(), edge_label.clone()),
                        );
                        // Add the neighbor node to the queue for later processing
                        queue.push_back(neighbor_node.clone());
                    }
                }
            }
        }

        // --- 4. Backtrack and build the path ---
        // Check if the destination exists in the predecessor map; if not, it's unreachable
        if !predecessors.contains_key(end_node) {
            return Err(ConnectError::UnreachableState(end_node.to_string()));
        }

        let mut current = end_node.to_string();

        let mut path = Vec::new();

        // Backtrack from destination to start using the predecessor map
        while current != start_node {
            if let Some((parent, edge_label)) = predecessors.get(&current) {
                path.push((edge_label.clone(), current.clone()));
                current = parent.clone();
            } else {
                return Err(ConnectError::InternalServerError(format!(
                    "failed to backtrack path from '{}' to '{}'",
                    end_node, start_node
                )));
            }
        }

        // Reverse to get the correct order (we built it backwards)
        path.reverse();
        switch_path.extend(path);
        trace!("Command path: '{:?}'", switch_path);
        Ok(switch_path)
    }
}

/// Regex pattern for matching and removing control characters at the start of lines.
///
/// This pattern matches carriage returns and backspace characters that may appear
/// at the beginning of terminal output, which can interfere with proper line parsing.
pub static IGNORE_START_LINE: Lazy<Regex> =
    Lazy::new(
        || match Regex::new(r"^(\r+(\s+\r+)*)|(\u{8}+(\s+\u{8}+)*)") {
            Ok(re) => re,
            Err(err) => panic!("invalid IGNORE_START_LINE regex: {err}"),
        },
    );

#[cfg(test)]
mod tests {
    use super::DeviceHandler;
    use crate::error::ConnectError;
    use std::collections::HashMap;

    fn build_test_handler() -> DeviceHandler {
        let mut dyn_param = HashMap::new();
        dyn_param.insert("EnablePassword".to_string(), "secret\n".to_string());

        DeviceHandler::new(
            vec![
                ("Login".to_string(), vec![r"^dev>\s*$"]),
                ("Enable".to_string(), vec![r"^dev#\s*$"]),
                ("Config".to_string(), vec![r"^dev\(cfg\)#\s*$"]),
            ],
            vec![],
            vec![
                (
                    "EnablePassword".to_string(),
                    (true, "EnablePassword".to_string(), true),
                    vec![r"^Password:\s*$"],
                ),
                (
                    "Confirm".to_string(),
                    (false, "y".to_string(), false),
                    vec![r"^\[y\/n\]\?\s*$"],
                ),
            ],
            vec![r"^--More--$"],
            vec![r"^ERROR: .+$"],
            vec![
                (
                    "Login".to_string(),
                    "enable".to_string(),
                    "Enable".to_string(),
                    false,
                    false,
                ),
                (
                    "Enable".to_string(),
                    "configure terminal".to_string(),
                    "Config".to_string(),
                    false,
                    false,
                ),
                (
                    "Config".to_string(),
                    "exit".to_string(),
                    "Enable".to_string(),
                    true,
                    false,
                ),
                (
                    "Enable".to_string(),
                    "exit".to_string(),
                    "Login".to_string(),
                    true,
                    false,
                ),
            ],
            vec![r"^ERROR: benign$"],
            dyn_param,
        )
        .expect("test handler config should be valid")
    }

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
    fn transition_path_is_found_for_reachable_state() {
        let mut handler = build_test_handler();
        handler.read("dev>");

        let path = handler
            .trans_state_write("config", None)
            .expect("reachable path should be found");

        assert_eq!(
            path,
            vec![
                ("enable".to_string(), "enable".to_string()),
                ("configure terminal".to_string(), "config".to_string()),
            ]
        );
    }

    #[test]
    fn transition_to_unknown_state_returns_error() {
        let mut handler = build_test_handler();
        handler.read("dev>");

        let err = handler
            .trans_state_write("does-not-exist", None)
            .expect_err("unknown target state should return error");
        match err {
            ConnectError::UnreachableState(s) => assert_eq!(s, "does-not-exist"),
            other => panic!("unexpected error type: {other}"),
        }
    }

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

    #[test]
    fn state_machine_diagnostics_are_clean_for_valid_template() {
        let handler = build_test_handler();
        let report = handler.diagnose_state_machine();

        assert!(!report.has_issues());
        assert!(report.missing_edge_sources.is_empty());
        assert!(report.missing_edge_targets.is_empty());
        assert!(report.unreachable_states.is_empty());
        assert!(report.dead_end_states.is_empty());
        assert!(report.duplicate_prompt_patterns.is_empty());
        assert!(report.potentially_ambiguous_prompt_states.is_empty());
        assert!(report.self_loop_only_states.is_empty());
    }

    #[test]
    fn state_machine_diagnostics_detect_invalid_edges_and_dead_ends() {
        let handler = DeviceHandler::new(
            vec![
                ("Login".to_string(), vec![r"^dev>\s*$"]),
                ("Enable".to_string(), vec![r"^dev#\s*$"]),
            ],
            vec![],
            vec![],
            vec![r"^--More--$"],
            vec![r"^ERROR: .+$"],
            vec![
                (
                    "Login".to_string(),
                    "enable".to_string(),
                    "Enable".to_string(),
                    false,
                    false,
                ),
                (
                    "Enable".to_string(),
                    "to-ghost".to_string(),
                    "Ghost".to_string(),
                    false,
                    false,
                ),
            ],
            vec![],
            HashMap::new(),
        )
        .expect("handler should build");

        let report = handler.diagnose_state_machine();
        assert!(report.has_issues());
        assert_eq!(report.missing_edge_targets, vec!["ghost".to_string()]);
        assert_eq!(report.dead_end_states, vec!["enable".to_string()]);
    }

    #[test]
    fn state_machine_diagnostics_detect_duplicate_prompt_patterns() {
        let handler = DeviceHandler::new(
            vec![
                ("Login".to_string(), vec![r"^dup>\s*$"]),
                ("Enable".to_string(), vec![r"^dup>\s*$"]),
            ],
            vec![],
            vec![],
            vec![r"^--More--$"],
            vec![r"^ERROR: .+$"],
            vec![(
                "Login".to_string(),
                "noop".to_string(),
                "Enable".to_string(),
                false,
                false,
            )],
            vec![],
            HashMap::new(),
        )
        .expect("handler should build");

        let report = handler.diagnose_state_machine();
        assert!(report.has_issues());
        assert!(!report.duplicate_prompt_patterns.is_empty());
        assert!(
            report
                .potentially_ambiguous_prompt_states
                .contains(&"enable".to_string())
        );
        assert!(
            report
                .potentially_ambiguous_prompt_states
                .contains(&"login".to_string())
        );
    }

    #[test]
    fn state_machine_diagnostics_detect_self_loop_only_states() {
        let handler = DeviceHandler::new(
            vec![
                ("Login".to_string(), vec![r"^dev>\s*$"]),
                ("Enable".to_string(), vec![r"^dev#\s*$"]),
            ],
            vec![],
            vec![],
            vec![r"^--More--$"],
            vec![r"^ERROR: .+$"],
            vec![(
                "Enable".to_string(),
                "noop".to_string(),
                "Enable".to_string(),
                false,
                false,
            )],
            vec![],
            HashMap::new(),
        )
        .expect("handler should build");

        let report = handler.diagnose_state_machine();
        assert!(report.self_loop_only_states.contains(&"enable".to_string()));
    }
}
