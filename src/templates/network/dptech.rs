//! DPTech Firewall device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for DPTech devices.
pub fn dptech_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^\r{0,1}\[.+\]\s*$"]),
            prompt_rule("Enable", &[r"^\r{0,1}<.+>\s*$"]),
        ],
        more_regex: vec![r" --More\(CTRL\+C break\)-- ".to_string()],
        error_regex: vec![
            r"% Unknown command.*".to_string(),
            r"Can't find the .+ object".to_string(),
            r".*not exist.*".to_string(),
            r".*item is longer.*".to_string(),
            r"Failed.*".to_string(),
            r"Undefined error.*".to_string(),
            r"% Command can not contain:.+".to_string(),
            r"Invalid parameter.*".to_string(),
            r"% Ambiguous command.".to_string(),
        ],
        edges: vec![
            transition_rule("Enable", "conf-mode", "Config", false, false),
            transition_rule("Config", "end", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for DPTech devices.
pub fn dptech() -> Result<DeviceHandler, ConnectError> {
    dptech_config().build()
}
