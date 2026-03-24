use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DeviceHandler;
use crate::error::ConnectError;

/// Public command execution strategy used by handler configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCommandExecutionConfig {
    /// Traditional prompt-driven success detection.
    #[default]
    PromptDriven,
    /// Append a shell marker and parse exit status from output.
    ShellExitStatus {
        marker: String,
        #[serde(default)]
        shell_flavor: DeviceShellFlavor,
    },
}

/// Shell flavor used when composing exit-status capture commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceShellFlavor {
    /// POSIX-compatible shells such as sh/bash/zsh.
    #[default]
    Posix,
    /// Fish shell, which uses `$status` instead of `$?`.
    Fish,
}

/// Prompt-matching rule for one state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevicePromptRule {
    pub state: String,
    pub patterns: Vec<String>,
}

/// Prompt rule that also captures a named group into the FSM sys value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DevicePromptWithSysRule {
    pub state: String,
    pub capture_group: String,
    pub pattern: String,
}

/// Interactive input rule for states such as password prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceInputRule {
    pub state: String,
    pub dynamic: bool,
    pub value: String,
    pub record_input: bool,
    pub patterns: Vec<String>,
}

/// State transition edge used by the FSM path planner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceTransitionRule {
    pub from_state: String,
    pub command: String,
    pub to_state: String,
    pub is_exit: bool,
    pub needs_format: bool,
}

/// Serializable configuration used to build a [`DeviceHandler`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct DeviceHandlerConfig {
    pub prompt: Vec<DevicePromptRule>,
    pub prompt_with_sys: Vec<DevicePromptWithSysRule>,
    pub write: Vec<DeviceInputRule>,
    pub more_regex: Vec<String>,
    pub error_regex: Vec<String>,
    pub edges: Vec<DeviceTransitionRule>,
    #[serde(default)]
    pub ignore_errors: Vec<String>,
    #[serde(default)]
    pub dyn_param: HashMap<String, String>,
    #[serde(default)]
    pub command_execution: DeviceCommandExecutionConfig,
}

impl DeviceHandlerConfig {
    /// Build a [`DeviceHandler`] from this configuration snapshot.
    pub fn build(&self) -> Result<DeviceHandler, ConnectError> {
        DeviceHandler::new(self.clone())
    }
}

impl DeviceHandler {
    /// Build a handler from a public configuration snapshot.
    pub fn from_config(config: &DeviceHandlerConfig) -> Result<Self, ConnectError> {
        Self::new(config.clone())
    }
}

/// Convenience helper for concise template definitions.
pub fn prompt_rule(state: &str, patterns: &[&str]) -> DevicePromptRule {
    DevicePromptRule {
        state: state.to_string(),
        patterns: patterns
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect(),
    }
}

/// Convenience helper for prompt rules that capture a sys value.
pub fn prompt_with_sys_rule(
    state: &str,
    capture_group: &str,
    pattern: &str,
) -> DevicePromptWithSysRule {
    DevicePromptWithSysRule {
        state: state.to_string(),
        capture_group: capture_group.to_string(),
        pattern: pattern.to_string(),
    }
}

/// Convenience helper for interactive input rules.
pub fn input_rule(
    state: &str,
    dynamic: bool,
    value: &str,
    record_input: bool,
    patterns: &[&str],
) -> DeviceInputRule {
    DeviceInputRule {
        state: state.to_string(),
        dynamic,
        value: value.to_string(),
        record_input,
        patterns: patterns
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect(),
    }
}

/// Convenience helper for transition edges.
pub fn transition_rule(
    from_state: &str,
    command: &str,
    to_state: &str,
    is_exit: bool,
    needs_format: bool,
) -> DeviceTransitionRule {
    DeviceTransitionRule {
        from_state: from_state.to_string(),
        command: command.to_string(),
        to_state: to_state.to_string(),
        is_exit,
        needs_format,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    #[test]
    fn config_build_matches_builtin_cisco_template() {
        let handler = templates::cisco().expect("cisco handler");
        let from_config = templates::cisco_config().build().expect("cisco config");

        assert_eq!(handler.states(), from_config.states());
        assert_eq!(handler.edges(), from_config.edges());
        assert!(handler.is_equivalent(&from_config));
    }

    #[test]
    fn config_build_supports_shell_exit_status_strategy() {
        let config = DeviceHandlerConfig {
            prompt: vec![prompt_rule("Root", &[r"^root#\s*$"])],
            prompt_with_sys: Vec::new(),
            write: Vec::new(),
            more_regex: Vec::new(),
            error_regex: Vec::new(),
            edges: Vec::new(),
            ignore_errors: Vec::new(),
            dyn_param: HashMap::new(),
            command_execution: DeviceCommandExecutionConfig::ShellExitStatus {
                marker: "__MARK__".to_string(),
                shell_flavor: DeviceShellFlavor::Posix,
            },
        };

        let handler = config.build().expect("build handler");
        let wrapped = handler.prepare_command_for_execution("echo hi", true);
        assert!(wrapped.contains("__MARK__"));
    }
}
