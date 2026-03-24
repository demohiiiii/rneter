//! Check Point Security Gateway device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Check Point Security Gateway devices.
pub fn checkpoint_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![prompt_rule("Enable", &[r"^\r{0,1}\S+\s*>\s*$"])],
        more_regex: vec![r"-- More --".to_string()],
        error_regex: vec![
            r".+Incomplete command\.".to_string(),
            r".+Invalid command:.+".to_string(),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Check Point Security Gateway devices.
pub fn checkpoint() -> Result<DeviceHandler, ConnectError> {
    checkpoint_config().build()
}
