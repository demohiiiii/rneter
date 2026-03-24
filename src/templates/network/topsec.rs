//! TopSec NGFW device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for TopSec NGFW devices.
pub fn topsec_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![prompt_rule("Enable", &[r"^\r{0,1}\S+[#%]\s*$"])],
        more_regex: vec![r"--More--".to_string()],
        error_regex: vec![r"^error".to_string()],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for TopSec NGFW devices.
pub fn topsec() -> Result<DeviceHandler, ConnectError> {
    topsec_config().build()
}
