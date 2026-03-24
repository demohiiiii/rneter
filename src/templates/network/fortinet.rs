//! Fortinet FortiGate device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule, prompt_with_sys_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Fortinet FortiGate devices.
pub fn fortinet_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![prompt_rule("Enable", &[r"^\r{0,1}\S+\s*#\s*$"])],
        prompt_with_sys: vec![prompt_with_sys_rule(
            "VDOMEnable",
            "VDOM",
            r"^\r{0,1}\S+\s*\((?<VDOM>\S+)\)\s*#\s*$",
        )],
        more_regex: vec![r"--More--".to_string()],
        error_regex: vec![
            r"Unknown action.*".to_string(),
            r"Command fail.*".to_string(),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Fortinet FortiGate devices.
pub fn fortinet() -> Result<DeviceHandler, ConnectError> {
    fortinet_config().build()
}
