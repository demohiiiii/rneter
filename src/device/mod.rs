//! Device state machine handler for network devices.
//!
//! This module provides a sophisticated state machine implementation for managing
//! network device interactions through SSH. It handles prompt detection, automatic
//! state transitions, and intelligent command routing based on the current device state.

use std::collections::HashMap;

use crate::session::SessionHooks;
use once_cell::sync::{Lazy, OnceCell};
use regex::{Regex, RegexSet};

mod builder;
mod config;
mod diagnostics;
mod execution;
mod runtime;
mod transitions;

pub use config::{
    DeviceCommandExecutionConfig, DeviceHandlerConfig, DeviceInputRule, DevicePromptRule,
    DevicePromptWithSysRule, DeviceShellFlavor, DeviceTransitionRule, input_rule, prompt_rule,
    prompt_with_sys_rule, transition_rule,
};
pub use diagnostics::StateMachineDiagnostics;
#[cfg(feature = "testkit")]
pub(crate) use execution::EXIT_STATUS_SUFFIX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandExecutionStrategy {
    PromptDriven,
    ShellExitStatus {
        marker: String,
        shell_flavor: DeviceShellFlavor,
    },
}

#[derive(Clone)]
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

    /// Regex patterns for complete lines that should be joined with a following prompt.
    prompt_prefix_regex: Option<RegexSet>,

    /// Prompt prefix regex patterns kept for handler equivalence checks.
    prompt_prefix_patterns: Vec<String>,

    /// Strategy used to determine command success for this handler.
    command_execution: CommandExecutionStrategy,

    /// Declarative lifecycle hooks retained from configuration for later runtime use.
    hooks: SessionHooks,

    /// Lazily built adjacency list over `edges`, cached because the edge set
    /// is immutable after construction and transitions run on every command.
    /// Values are `(to_state, raw_command, needs_format)`.
    adjacency: OnceCell<AdjacencyList>,
}

type ExitPath = Option<(String, Vec<(String, String)>)>;
type AdjacencyList = HashMap<String, Vec<(String, String, bool)>>;
pub(crate) const PRIVATE_USE_PLACEHOLDER: &str = "<PUA>";

pub(crate) fn is_private_use(ch: char) -> bool {
    matches!(
        ch as u32,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    )
}

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

/// Regex pattern for stripping OSC escape sequences such as xterm title updates.
pub static STRIP_OSC_ESCAPE: Lazy<Regex> =
    Lazy::new(|| match Regex::new(r"\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)") {
        Ok(re) => re,
        Err(err) => panic!("invalid STRIP_OSC_ESCAPE regex: {err}"),
    });

/// Regex pattern for stripping DCS escape sequences such as fish terminal probes.
pub static STRIP_DCS_ESCAPE: Lazy<Regex> = Lazy::new(|| match Regex::new(r"\x1BP(?s:.*?)\x1B\\") {
    Ok(re) => re,
    Err(err) => panic!("invalid STRIP_DCS_ESCAPE regex: {err}"),
});

/// Regex pattern for stripping CSI escape sequences such as `\x1b[?1034h`.
pub static STRIP_CSI_ESCAPE: Lazy<Regex> =
    Lazy::new(|| match Regex::new(r"\x1B\[[0-?]*[ -/]*[@-~]") {
        Ok(re) => re,
        Err(err) => panic!("invalid STRIP_CSI_ESCAPE regex: {err}"),
    });

/// Regex pattern for stripping simple escape sequences such as `\x1b=` and `\x1b>`.
pub static STRIP_SIMPLE_ESCAPE: Lazy<Regex> =
    Lazy::new(|| match Regex::new(r"\x1B(?:[@-Z\\-_]|[=>])") {
        Ok(re) => re,
        Err(err) => panic!("invalid STRIP_SIMPLE_ESCAPE regex: {err}"),
    });

pub(crate) fn sanitize_terminal_text(line: &str) -> String {
    let without_osc = STRIP_OSC_ESCAPE.replace_all(line, "");
    let without_dcs = STRIP_DCS_ESCAPE.replace_all(without_osc.as_ref(), "");
    let without_csi = STRIP_CSI_ESCAPE.replace_all(without_dcs.as_ref(), "");
    let without_simple = STRIP_SIMPLE_ESCAPE.replace_all(without_csi.as_ref(), "");

    let mut sanitized = String::with_capacity(without_simple.len());
    let mut in_pua_run = false;
    for ch in without_simple.chars() {
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            continue;
        }
        if is_private_use(ch) {
            if !in_pua_run {
                sanitized.push_str(PRIVATE_USE_PLACEHOLDER);
                in_pua_run = true;
            }
            continue;
        }
        in_pua_run = false;
        sanitized.push(ch);
    }

    sanitized
}

pub(crate) fn latest_terminal_fragment(line: &str) -> &str {
    line.rsplit(['\n', '\r'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(line)
}

pub(crate) fn normalize_terminal_output(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());

    for chunk in text.split_inclusive('\n') {
        let has_newline = chunk.ends_with('\n');
        let body = if has_newline {
            &chunk[..chunk.len().saturating_sub(1)]
        } else {
            chunk
        };
        let sanitized = sanitize_terminal_text(body);
        let visible = latest_terminal_fragment(&sanitized).trim_end_matches('\r');
        normalized.push_str(visible);
        if has_newline {
            normalized.push('\n');
        }
    }

    normalized
}

pub(crate) fn normalize_terminal_fragment(line: &str) -> String {
    let sanitized = sanitize_terminal_text(line);
    latest_terminal_fragment(&sanitized)
        .trim_end_matches('\r')
        .to_string()
}

pub(crate) fn terminal_fragment_has_pua(line: &str) -> bool {
    normalize_terminal_fragment(line).contains(PRIVATE_USE_PLACEHOLDER)
}

pub(crate) fn merge_terminal_prompt_fragments(
    lines: &[String],
    tail: Option<&str>,
) -> Option<String> {
    let mut fragments = Vec::with_capacity(lines.len() + usize::from(tail.is_some()));

    for line in lines {
        let fragment = normalize_terminal_fragment(line);
        let trimmed = fragment.trim();
        if !trimmed.is_empty() {
            fragments.push(trimmed.to_string());
        }
    }

    if let Some(tail) = tail {
        let fragment = normalize_terminal_fragment(tail);
        let trimmed = fragment.trim();
        if !trimmed.is_empty() {
            fragments.push(trimmed.to_string());
        }
    }

    if fragments.is_empty() {
        None
    } else {
        Some(fragments.join(" "))
    }
}

#[cfg(test)]
fn build_test_handler() -> DeviceHandler {
    let mut dyn_param = HashMap::new();
    dyn_param.insert("EnablePassword".to_string(), "secret\n".to_string());

    DeviceHandler::new(DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Login", &[r"^dev>\s*$"]),
            prompt_rule("Enable", &[r"^dev#\s*$"]),
            prompt_rule("Config", &[r"^dev\(cfg\)#\s*$"]),
        ],
        write: vec![
            input_rule(
                "EnablePassword",
                true,
                "EnablePassword",
                true,
                &[r"^Password:\s*$"],
            ),
            input_rule("Confirm", false, "y", false, &[r"^\[y\/n\]\?\s*$"]),
        ],
        more_regex: vec![r"^--More--$".to_string()],
        error_regex: vec![r"^ERROR: .+$".to_string()],
        edges: vec![
            transition_rule("Login", "enable", "Enable", false, false),
            transition_rule("Enable", "configure terminal", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
            transition_rule("Enable", "exit", "Login", true, false),
        ],
        ignore_errors: vec![r"^ERROR: benign$".to_string()],
        dyn_param,
        ..Default::default()
    })
    .expect("test handler config should be valid")
}
