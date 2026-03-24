//! QiAnXin NSG device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for QiAnXin NSG devices.
pub fn qianxin_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^\S+-config.*]\s*$"]),
            prompt_rule("Enable", &[r"^\S+>\s*$"]),
        ],
        more_regex: vec![r"--More--".to_string()],
        error_regex: vec![
            r"% Unknown command.".to_string(),
            r"% Command incomplete.".to_string(),
            r"%?\s+Invalid parameter.*".to_string(),
            r"\s+Valid name can.*".to_string(),
            r"\s+Repetitions with Object.*".to_string(),
            r".+ exist".to_string(),
            r"\s+Start larger than end".to_string(),
            r"\s+Name can not repeat".to_string(),
            r"Object .+ referenced by other module".to_string(),
            r"Object service has been referenced".to_string(),
            r"Object \[.+\] is quoted".to_string(),
        ],
        edges: vec![
            transition_rule("Enable", "config terminal", "Config", false, false),
            transition_rule("Config", "end", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for QiAnXin NSG devices.
pub fn qianxin() -> Result<DeviceHandler, ConnectError> {
    qianxin_config().build()
}
