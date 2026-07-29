//! Fortinet FortiGate device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule, prompt_with_sys_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Fortinet FortiGate devices.
pub fn fortinet_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![prompt_rule("Enable", &[r"^\S+\s*[#$]\s*$"])],
        prompt_with_sys: vec![prompt_with_sys_rule(
            "VDOMEnable",
            "VDOM",
            r"^\S+\s*\((?<VDOM>\S+)\)\s*[#$]\s*$",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netdriver_hash_and_dollar_prompt_variants_are_supported() {
        let mut handler = fortinet().expect("create Fortinet handler");

        for prompt in ["hostname #", "hostname $", "\r\nhostname # \r\n"] {
            handler.read(prompt);
            assert_eq!(handler.current_state(), "enable", "prompt: {prompt:?}");
        }
        for prompt in ["hostname (root) #", "hostname (root) $"] {
            handler.read(prompt);
            assert_eq!(handler.current_state(), "vdomenable", "prompt: {prompt:?}");
            assert_eq!(handler.current_sys(), Some("root"), "prompt: {prompt:?}");
        }
    }
}
