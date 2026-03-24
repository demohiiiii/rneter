//! Cisco IOS/IOS-XE device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Cisco IOS/IOS-XE devices.
pub fn cisco_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^\S+\(\S+\)#\s*$"]),
            prompt_rule("Enable", &[r"^[^\s#]+#\s*$"]),
            prompt_rule("Login", &[r"^[^\s<]+>\s*$"]),
        ],
        write: vec![input_rule(
            "EnablePassword",
            true,
            "EnablePassword",
            true,
            &[r"^\x00*\r(Enable )?Password:"],
        )],
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
            transition_rule("Enable", "exit", "Login", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Cisco IOS/IOS-XE devices.
pub fn cisco() -> Result<DeviceHandler, ConnectError> {
    cisco_config().build()
}
