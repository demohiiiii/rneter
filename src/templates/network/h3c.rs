//! H3C Comware device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for H3C devices.
pub fn h3c_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^(RBM_P|RBM_S)?\[.+\]\s*$"]),
            prompt_rule("Enable", &[r"^(RBM_P|RBM_S)?<.+>\s*$"]),
        ],
        more_regex: vec![r"\s*---- More ----\s*".to_string()],
        error_regex: vec![
            r".+\^.+".to_string(),
            r".+%.+".to_string(),
            r".+doesn't exist.+".to_string(),
            r".+does not exist.+".to_string(),
            r"Object group with given name exists with different type.".to_string(),
        ],
        edges: vec![
            transition_rule("Enable", "system-view", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for H3C devices.
pub fn h3c() -> Result<DeviceHandler, ConnectError> {
    h3c_config().build()
}
