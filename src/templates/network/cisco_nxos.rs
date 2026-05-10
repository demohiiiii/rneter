use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use crate::session::{Command, HookAction, SessionHooks, SessionOperation};
use std::collections::HashMap;

pub fn cisco_nxos_config() -> DeviceHandlerConfig {
    let write = vec![input_rule(
        "EnablePassword",
        true,
        "EnablePassword",
        true,
        &[r"(?i)^(Enable )?Password:"],
    )];

    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Login", &[r"^\r{0,1}[^\s<]+>\s*$"]),
            prompt_rule("Enable", &[r"^\r{0,1}[^\s#]+#\s*$"]),
            prompt_rule("Config", &[r"^\r{0,1}\S+\(\S+\)#\s*$"]),
        ],
        write,
        more_regex: vec![r"\s*--More--\s*".to_string()],
        error_regex: vec![
            r"% Invalid command".to_string(),
            r"% Invalid input".to_string(),
            r"% Ambiguous command".to_string(),
            r"% Incomplete command".to_string(),
            r"^% .+".to_string(),
        ],
        edges: vec![
            transition_rule("Login", "enable", "Enable", false, false),
            transition_rule("Enable", "configure terminal", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
            transition_rule("Enable", "disable", "Login", true, false),
        ],
        dyn_param: HashMap::new(),
        hooks: SessionHooks {
            after_connect: vec![
                HookAction::new(
                    "set-terminal-width",
                    SessionOperation::from(Command {
                        mode: "Enable".to_string(),
                        command: "terminal width 511".to_string(),
                        ..Command::default()
                    }),
                ),
                HookAction::new(
                    "disable-paging",
                    SessionOperation::from(Command {
                        mode: "Enable".to_string(),
                        command: "terminal length 0".to_string(),
                        ..Command::default()
                    }),
                ),
            ],
            ..SessionHooks::default()
        },
        ..Default::default()
    }
}

pub fn cisco_nxos() -> Result<DeviceHandler, ConnectError> {
    cisco_nxos_config().build()
}
