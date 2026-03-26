//! Arista EOS device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use crate::templates::transfer::cisco_like_device_transfer_input_rules;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Arista EOS devices.
pub fn arista_config() -> DeviceHandlerConfig {
    let mut write = vec![input_rule(
        "EnablePassword",
        true,
        "EnablePassword",
        true,
        &[r"Password:"],
    )];
    write.extend(cisco_like_device_transfer_input_rules());

    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^\r{0,1}\S+\(\S+\)#\s*$"]),
            prompt_rule("Enable", &[r"^\r{0,1}[^\s#]+#\s*$"]),
            prompt_rule("Login", &[r"^\r{0,1}[^\s<]+>\s*$"]),
        ],
        write,
        more_regex: vec![r" --More-- ".to_string()],
        error_regex: vec![
            r"% Invalid input".to_string(),
            r"% Ambiguous command".to_string(),
            r"% Bad secret".to_string(),
            r"% Unrecognized command".to_string(),
            r"% Incomplete command".to_string(),
            r"% Invalid port range .+".to_string(),
            r"! Access VLAN does not exist. Creating vlan .+".to_string(),
            r"% Address \S+ is already assigned to interface .+".to_string(),
            r"% Removal of physical interfaces is not permitted".to_string(),
            r"^% .+".to_string(),
        ],
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

/// Returns a `DeviceHandler` configured for Arista EOS devices.
pub fn arista() -> Result<DeviceHandler, ConnectError> {
    arista_config().build()
}
