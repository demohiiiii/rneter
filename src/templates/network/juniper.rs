//! Juniper JunOS device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use crate::session::{Command, HookAction, SessionHooks, SessionOperation};
use std::collections::HashMap;

/// Exports the underlying handler configuration for Juniper JunOS devices.
pub fn juniper_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Enable", &[r"^\S+@\S+>\s*$"]),
            prompt_rule("Config", &[r"^(?:\[edit\]\s+)?\S+@\S+#\s*$"]),
        ],
        prompt_prefix: vec![r"^\[edit\]\s*$".to_string()],
        write: vec![input_rule(
            "Save",
            false,
            "yes",
            true,
            &[r"Exit with uncommitted changes\? \[yes,no\] \(yes\) "],
        )],
        more_regex: vec![r"---\(more.*\)---".to_string()],
        error_regex: vec![
            r".*unknown command.*".to_string(),
            r"syntax error.*".to_string(),
            r"error:.+".to_string(),
            r".+not found.*".to_string(),
            r"invalid value .+".to_string(),
            r"invalid ip address .+".to_string(),
            r".*invalid prefix length .+".to_string(),
            r"prefix length \S+ is larger than \d+ .+".to_string(),
            r"number: \S+: Value must be a number from 0 to 255 at \S+".to_string(),
            r"\s+\^$".to_string(),
        ],
        edges: vec![
            transition_rule("Enable", "configure", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        hooks: SessionHooks {
            after_connect: vec![HookAction::new(
                "disable-paging",
                SessionOperation::from(Command {
                    mode: "Enable".to_string(),
                    command: "set cli screen-length 0".to_string(),
                    ..Command::default()
                }),
            )],
            ..SessionHooks::default()
        },
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Juniper JunOS devices.
pub fn juniper() -> Result<DeviceHandler, ConnectError> {
    juniper_config().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_to_config_transition_uses_configure() {
        let handler = juniper().expect("create juniper device handler");
        assert!(handler.edges().contains(&(
            "enable".to_string(),
            "configure".to_string(),
            "config".to_string(),
            false,
            false,
        )));
    }

    #[test]
    fn config_prompt_matches_edit_context_line_with_prompt() {
        let mut handler = juniper().expect("create juniper device handler");

        assert!(handler.read_prompt("[edit] admin@dyadd-srx# "));
    }

    #[test]
    fn edit_context_line_is_held_as_prompt_prefix() {
        let handler = juniper().expect("create juniper device handler");

        assert!(handler.read_prompt_prefix("[edit]\n"));
    }

    #[test]
    fn juniper_template_declares_screen_length_hook() {
        let config = juniper_config();
        assert_eq!(config.hooks.after_connect.len(), 1);
        assert_eq!(config.hooks.after_connect[0].name, "disable-paging");
    }
}
