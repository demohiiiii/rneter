//! Cisco IOS/IOS-XE device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use crate::session::{Command, HookAction, SessionHooks, SessionOperation};
use std::collections::HashMap;

/// Exports the underlying handler configuration for Cisco IOS/IOS-XE devices.
pub fn cisco_config() -> DeviceHandlerConfig {
    let write = vec![input_rule(
        "EnablePassword",
        true,
        "EnablePassword",
        true,
        &[r"(Enable )?Password:"],
    )];

    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Login", &[r"^[^\s<]+>\s*$"]),
            // Parenthesized prompts belong to Config and must not also match
            // the broader privileged-mode prompt.
            prompt_rule("Enable", &[r"^[^\s#()]+#\s*$"]),
            prompt_rule("Config", &[r"^\S+\(\S+\)#\s*$"]),
        ],
        write,
        more_regex: vec![r"\s*<--- More --->\s*".to_string()],
        error_regex: vec![
            r"% Invalid command at '\^' marker\.".to_string(),
            r"% Invalid parameter detected at '\^' marker\.".to_string(),
            r"invalid vlan \(reserved value\) at '\^' marker\.".to_string(),
            r"ERROR: VLAN \d+ is not a primary vlan".to_string(),
            r"\^$".to_string(),
            r"^%.+".to_string(),
            r"^Command authorization failed.*".to_string(),
            r"^Command rejected:.*".to_string(),
            r"ERROR:.+".to_string(),
            r"Invalid password".to_string(),
            r"Access denied.".to_string(),
            r"End address less than start address".to_string(),
        ],
        edges: vec![
            transition_rule("Login", "enable", "Enable", false, false),
            transition_rule("Enable", "configure terminal", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
            transition_rule("Enable", "disable", "Login", true, false),
        ],
        dyn_param: HashMap::new(),
        hooks: SessionHooks {
            after_connect: vec![HookAction::new(
                "disable-paging",
                SessionOperation::from(Command {
                    mode: "Enable".to_string(),
                    command: "terminal pager 0".to_string(),
                    ..Command::default()
                }),
            )],
            ..SessionHooks::default()
        },
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Cisco IOS/IOS-XE devices.
pub fn cisco() -> Result<DeviceHandler, ConnectError> {
    cisco_config().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_password_prompt_matches_with_or_without_carriage_return() {
        for prompt in ["Password: ", "\rPassword: "] {
            let mut handler = cisco().expect("create cisco device handler");
            handler
                .dyn_param
                .insert("EnablePassword".to_string(), "secret\n".to_string());

            assert_eq!(
                handler.read_need_write(prompt),
                Some(("secret\n".to_string(), true)),
                "prompt should match: {prompt:?}"
            );
        }
    }

    #[test]
    fn prompt_states_distinguish_config_from_enable() {
        for (prompt, expected_state) in [
            ("cisco-catalyst>", "login"),
            ("cisco-catalyst#", "enable"),
            ("cisco-catalyst(config)#", "config"),
            ("cisco-catalyst(config-if)#", "config"),
        ] {
            let mut handler = cisco().expect("create cisco device handler");
            handler.read(prompt);
            assert_eq!(
                handler.current_state(),
                expected_state,
                "prompt should resolve to the expected state: {prompt:?}"
            );
        }
    }

    #[test]
    fn enable_to_login_transition_uses_disable() {
        let handler = cisco().expect("create cisco device handler");
        assert!(handler.edges().contains(&(
            "enable".to_string(),
            "disable".to_string(),
            "login".to_string(),
            true,
            false,
        )));
    }

    #[test]
    fn cisco_template_disables_paging_after_connect() {
        let config = cisco_config();
        assert_eq!(config.hooks.after_connect.len(), 1);
        assert_eq!(config.hooks.after_connect[0].name, "disable-paging");
    }
}
