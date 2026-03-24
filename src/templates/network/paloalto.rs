//! Palo Alto Networks device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Palo Alto Networks devices.
pub fn paloalto_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^\r{0,1}\S+@\S+#\s*$"]),
            prompt_rule("Enable", &[r"^\r{0,1}\S+@\S+>\s*$"]),
        ],
        more_regex: vec![r"(--more--)|(lines \d+-\d+ )".to_string()],
        error_regex: vec![
            r"Unknown command:.*".to_string(),
            r"Invalid syntax.".to_string(),
            r"Server error:.*".to_string(),
            r"Validation Error:.*".to_string(),
            r"Commit failed".to_string(),
        ],
        edges: vec![
            transition_rule("Enable", "configure", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Palo Alto Networks devices.
pub fn paloalto() -> Result<DeviceHandler, ConnectError> {
    paloalto_config().build()
}
