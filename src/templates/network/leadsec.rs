//! LeadSec PowerV device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for LeadSec PowerV devices.
pub fn leadsec_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![prompt_rule("Login", &[r"^[A-Za-z0-9]+>\s*$"])],
        error_regex: vec![
            r"^\^\s.*".to_string(),
            r"^错误[：:]?\s?.*".to_string(),
            r"unknown keyword".to_string(),
            r"\S*存在".to_string(),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for LeadSec PowerV devices.
pub fn leadsec() -> Result<DeviceHandler, ConnectError> {
    leadsec_config().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_v_prompt_matches_reference_driver_constraints() {
        let mut handler = leadsec().expect("create LeadSec handler");

        for prompt in ["PowerV>", "PowerV123> ", "\r\nPowerV>\r\n"] {
            assert!(
                handler.read_prompt(prompt),
                "prompt should match: {prompt:?}"
            );
        }
        for prompt in ["Power-V>", "Power_V>", "PowerV#"] {
            assert!(
                !handler.read_prompt(prompt),
                "prompt should not match: {prompt:?}"
            );
        }
    }

    #[test]
    fn power_v_error_variants_are_detected() {
        for error in [
            "^ invalid command",
            "错误：参数无效",
            "unknown keyword",
            "对象已存在",
        ] {
            let mut handler = leadsec().expect("create LeadSec handler");
            handler.read(error);
            assert_eq!(
                handler.current_state(),
                "error",
                "error should match: {error:?}"
            );
        }
    }
}
