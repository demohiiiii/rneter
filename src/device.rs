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

/// A state machine handler for managing device interactions.
///
/// `DeviceHandler` implements a finite state machine that tracks the current state
/// of a network device (e.g., user mode, enable mode, config mode) and handles
/// automatic transitions between states. It uses regex patterns to detect prompts
/// and state changes, and maintains a graph of possible state transitions.
///
/// # State Machine Concepts
///
/// - **States**: Different modes the device can be in (e.g., "UserMode", "EnableMode")
/// - **Prompts**: Regex patterns that identify when the device is in a particular state
/// - **Transitions**: Commands that move the device from one state to another
/// - **Edges**: The graph of allowed state transitions with associated commands
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
}

/// Predefined states that exist in every device handler.
///
/// These states are always present and have special meanings:
/// - `Output`: Default state for regular command output
/// - `More`: State for paginated output (e.g., "--More--" prompts)
/// - `Error`: State indicating an error occurred
static PRE_STATE: Lazy<Vec<String>> = Lazy::new(|| {
    vec![
        "Output".to_string(),
        "More".to_string(),
        "Error".to_string(),
    ]
});

impl DeviceHandler {
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
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use rneter::device::DeviceHandler;
    /// use std::collections::HashMap;
    ///
    /// let handler = DeviceHandler::new(
    ///     vec![("UserMode".to_string(), vec![r">"])],
    ///     vec![],
    ///     vec![],
    ///     vec![r"--More--"],
    ///     vec![r"% Invalid"],
    ///     vec![],
    ///     vec![],
    ///     HashMap::new(),
    /// );
    /// ```
    pub fn new<I, S>(
        prompt: Vec<(String, I)>,
        prompt_with_sys: Vec<(String, S, String)>,
        write: Vec<(String, (bool, String, bool), I)>,
        more_regex: I,
        error_regex: I,
        edges: Vec<(String, String, String, bool, bool)>,
        ignore_errors: I,
        dyn_param: HashMap<String, String>,
    ) -> DeviceHandler
    where
        S: AsRef<str> + Clone,
        I: IntoIterator<Item = S>,
    {
        let mut all_states: Vec<String> = PRE_STATE.iter().map(|s| s.to_string()).collect();

        let mut regexs: Vec<S> = Vec::new();
        let mut regex_index_map = HashMap::new();

        let start_offset = regexs.len();
        regexs.extend(more_regex);
        for i in start_offset..regexs.len() {
            regex_index_map.insert(i, 1);
        }

        let start_offset = regexs.len();
        regexs.extend(error_regex);
        for i in start_offset..regexs.len() {
            regex_index_map.insert(i, 2);
        }

        for (state, regex_iter) in prompt {
            // The new state's index is the current length of all_states
            let state_index = all_states.len();
            all_states.push(state);

            let start_offset = regexs.len();
            regexs.extend(regex_iter);

            for i in start_offset..regexs.len() {
                regex_index_map.insert(i, state_index);
            }
        }

        let mut catch_map = HashMap::new();

        let sys_prompt_state_index = all_states.len();

        for (state, regex, catch) in prompt_with_sys {
            // The new state's index is the current length of all_states
            let state_index = all_states.len();
            all_states.push(state.clone());

            let start_offset = regexs.len();

            catch_map.insert(start_offset, (Regex::new(&regex.as_ref()).unwrap(), catch));

            regexs.push(regex);

            regex_index_map.insert(start_offset, state_index);
        }

        let sys_prompt_index = (sys_prompt_state_index, all_states.len() - 1);

        let prompt_index = (3, all_states.len() - 1);

        let mut input_map = HashMap::new();

        for (state, cmd, regex_iter) in write {
            // The new state's index is the current length of all_states
            let state_index = all_states.len();
            all_states.push(state.clone());

            let start_offset = regexs.len();
            regexs.extend(regex_iter);

            input_map.insert(state, cmd);

            for i in start_offset..regexs.len() {
                regex_index_map.insert(i, state_index);
            }
        }

        input_map.insert("More".to_string(), (false, " ".to_string(), false));

        let all_regex = RegexSet::new(&regexs).unwrap();

        let mut ignore_iter = ignore_errors.into_iter().peekable();
        let ignore_errors = if ignore_iter.peek().is_none() {
            None
        } else {
            Some(RegexSet::new(ignore_iter).unwrap())
        };

        Self {
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
        }
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
            return (0, self.all_states.get(0).unwrap(), None);
        }
        let mut current_state_catch = None;
        let index = matches.get(0).unwrap();
        if need_catch {
            if let Some((regex, catch)) = self.catch_map.get(index) {
                if let Some(caps) = regex.captures(line) {
                    current_state_catch = caps.name(catch).map(|s| s.as_str().to_string());
                }
            }
        }
        let state_index = *self.regex_index_map.get(index).unwrap();
        (
            state_index,
            self.all_states.get(state_index).unwrap(),
            current_state_catch,
        )
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
            }

            self.current_state_index = state_index;
        }
    }

    /// Checks if a line matches an error pattern that should be ignored.
    fn ignore_error(&self, line: &str) -> bool {
        if self.ignore_errors.is_none() {
            return false;
        }

        self.ignore_errors.as_ref().unwrap().is_match(line)
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
        self.all_states.get(self.current_state_index).unwrap()
    }

    /// Checks if the current state is an error state.
    ///
    /// # Returns
    ///
    /// `true` if the device is currently in an error state, `false` otherwise
    pub fn error(&self) -> bool {
        self.current_state().eq("Error")
    }

    /// Finds the path to exit from system-specific prompts.
    ///
    /// When in a system-specific state (e.g., a specific configuration context),
    /// this method finds the sequence of commands needed to exit to a non-system state.
    fn exit_until_no_sys(
        &self,
        sys: Option<&String>,
    ) -> Result<Option<(&str, Vec<(String, String)>)>, ConnectError> {
        if !self.match_sys_prompt(self.current_state_index) {
            return Ok(None);
        }
        let mut exit_edges = self.edges.iter().filter(|(_, _, _, exit, _)| *exit);
        let mut edge_map = HashMap::new();
        while let Some((start, cmd, end, _, format)) = exit_edges.next() {
            edge_map.insert(start, (cmd, end, format));
        }
        let mut path = Vec::new();
        let mut current = &self.current_state().to_string();
        loop {
            if let Some((cmd, end, format)) = edge_map.get(current) {
                path.push((Self::format_cmd(**format, *cmd, sys), (*end).to_string()));
                if let Some(index) = self.all_states.iter().position(|v| v.eq(*end)) {
                    if !self.match_sys_prompt(index) {
                        return Ok(Some((*end, path)));
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
    fn format_cmd(format: bool, cmd: &String, sys: Option<&String>) -> String {
        if format {
            if sys.is_some() {
                cmd.replace("{}", sys.as_ref().unwrap())
            } else {
                String::new()
            }
        } else {
            cmd.clone()
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
        let mut start_node = self.current_state();

        let end_node = state;

        let mut switch_path = Vec::new();

        if self.sys.is_some() && sys.is_some() {
            if self.sys.as_ref().unwrap() != sys.unwrap() {
                trace!(
                    "Need to switch system: {} to {}",
                    self.sys.as_ref().unwrap(),
                    sys.unwrap()
                );
                if let Some((node, exit_path)) = self.exit_until_no_sys(sys)? {
                    start_node = node;
                    switch_path.extend(exit_path);
                }
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
            adj_list
                .entry(from.clone())
                .or_default()
                .push((to.clone(), Self::format_cmd(*format, label, sys)));
        }

        // --- 2. Initialize BFS data structures ---
        // Queue for storing nodes to visit
        let mut queue = VecDeque::new();
        queue.push_back(start_node.to_string());

        // Set of visited nodes to prevent revisits and infinite loops
        let mut visited = HashSet::new();
        visited.insert(start_node.to_string());

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
        while &current != start_node {
            if let Some((parent, edge_label)) = predecessors.get(&current) {
                path.push((edge_label.clone(), current.clone()));
                current = parent.clone();
            } else {
                // Theoretically, this shouldn't happen if the destination is reachable
                unimplemented!();
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
    Lazy::new(|| Regex::new(r"^(\r+(\s+\r+)*)|(\u{8}+(\s+\u{8}+)*)").unwrap());
