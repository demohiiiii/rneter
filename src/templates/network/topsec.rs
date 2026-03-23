//! TopSec NGFW device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for TopSec NGFW devices.
pub fn topsec() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt - TopSec only has Enable mode
        vec![("Enable".to_string(), vec![r"^\r{0,1}\S+[#%]\s*$"])],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"--More--"],
        // Error regex
        vec![r"^error"],
        // Edges - No mode transitions
        vec![],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}
