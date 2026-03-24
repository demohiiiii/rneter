//! Device state machine handler for network devices.
//!
//! This module provides a sophisticated state machine implementation for managing
//! network device interactions through SSH. It handles prompt detection, automatic
//! state transitions, and intelligent command routing based on the current device state.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::{Regex, RegexSet};

mod builder;
mod diagnostics;
mod execution;
mod runtime;
mod transitions;

pub use diagnostics::StateMachineDiagnostics;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandExecutionStrategy {
    PromptDriven,
    ShellExitStatus { marker: String },
}

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

    /// Strategy used to determine command success for this handler.
    command_execution: CommandExecutionStrategy,
}

type ExitPath = Option<(String, Vec<(String, String)>)>;

/// Predefined states that exist in every device handler.
static PRE_STATE: Lazy<Vec<String>> = Lazy::new(|| {
    vec![
        "Output".to_string(),
        "More".to_string(),
        "Error".to_string(),
    ]
});

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
