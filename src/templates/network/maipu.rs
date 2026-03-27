//! Maipu Network device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Maipu devices.
pub fn maipu_config() -> DeviceHandlerConfig {
    let write = vec![input_rule(
        "EnablePassword",
        true,
        "EnablePassword",
        true,
        &[r"password:"],
    )];

    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^\r{0,1}\S+\(\S+\)#\s*$"]),
            prompt_rule("Enable", &[r"^\r{0,1}[^\s#]+#\s*$"]),
            prompt_rule("Login", &[r"^\r{0,1}[^\s<]+>\s*$"]),
        ],
        write,
        error_regex: vec![r"% Invalid input".to_string()],
        edges: vec![
            transition_rule("Login", "enable", "Enable", false, false),
            transition_rule("Enable", "configure terminal", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
            transition_rule("Enable", "exit", "Login", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Maipu devices.
pub fn maipu() -> Result<DeviceHandler, ConnectError> {
    maipu_config().build()
}
