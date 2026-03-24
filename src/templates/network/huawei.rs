//! Huawei VRP device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Huawei VRP devices.
pub fn huawei_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Config", &[r"^(HRP_M|HRP_S){0,1}\[.+]+\s*$"]),
            prompt_rule("Enable", &[r"^(HRP_M|HRP_S){0,1}<.+>\s*$"]),
        ],
        write: vec![input_rule(
            "Save",
            false,
            "y",
            true,
            &[
                r"Are you sure to continue\?\[Y\/N\]: ",
                r"startup saved-configuration file on peer device\?\[Y\/N\]: ",
                r"Warning: The current configuration will be written to the device. Continue\? \[Y\/N\]: ",
            ],
        )],
        more_regex: vec![r"\s*---- More ----\s*".to_string()],
        error_regex: vec![r"Error: .+$".to_string(), r"\^$".to_string()],
        edges: vec![
            transition_rule("Enable", "system-view", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Huawei VRP devices.
pub fn huawei() -> Result<DeviceHandler, ConnectError> {
    huawei_config().build()
}
