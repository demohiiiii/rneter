//! Hillstone SG device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Hillstone devices.
pub fn hillstone_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Enable", &[r"^.+#\s\r{0,1}$"]),
            prompt_rule("Config", &[r"^.+\(config.*\)\s*#\s\r{0,1}$"]),
        ],
        write: vec![input_rule(
            "Save",
            false,
            "y",
            true,
            &[
                r"Save configuration, are you sure\? \[y\]\/n: ",
                r"Save configuration for all VSYS, are you sure\? \[y\]\/n: ",
                r"Backup start configuration file, are you sure\? y\/\[n\]: ",
                r"Backup all start configuration files, are you sure\? y\/\[n\]: ",
                r"保存配置，请确认 \[y\]\/n: ",
                r"备份启动配置文件，请确认 y\/\[n\]: ",
                r"保存所有VSYS的配置，请确认 \[y\]\/n: ",
                r"备份所有启动配置文件，请确认 y\/\[n\]: ",
            ],
        )],
        more_regex: vec![r"\s*--More--\s*".to_string()],
        error_regex: vec![
            r".+\^.+".to_string(),
            r".+%.+".to_string(),
            r".+doesn't exist.+".to_string(),
            r".+does not exist.+".to_string(),
            r"Object group with given name exists with different type.".to_string(),
        ],
        edges: vec![
            transition_rule("Enable", "config", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Hillstone devices.
pub fn hillstone() -> Result<DeviceHandler, ConnectError> {
    hillstone_config().build()
}
