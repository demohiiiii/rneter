//! Venustech USG device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Venustech USG devices.
pub fn venustech_config() -> DeviceHandlerConfig {
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
            prompt_rule("Enable", &[r"^[^\s#()]+#\s*$"]),
            prompt_rule("Config", &[r"^\S+\(\S+\)#\s*$"]),
        ],
        write,
        more_regex: vec![r"--More-- \(\d+% of \d+ bytes\)".to_string()],
        error_regex: vec![r"^%.+".to_string(), r".+not exist!".to_string()],
        edges: vec![
            transition_rule("Login", "enable", "Enable", false, false),
            transition_rule("Enable", "configure terminal", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
            transition_rule("Enable", "disable", "Login", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Venustech USG devices.
pub fn venustech() -> Result<DeviceHandler, ConnectError> {
    venustech_config().build()
}
