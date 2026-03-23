//! Check Point Security Gateway device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for Check Point Security Gateway devices.
pub fn checkpoint() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt - Check Point only has Enable mode
        vec![("Enable".to_string(), vec![r"^\r{0,1}\S+\s*>\s*$"])],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"-- More --"],
        // Error regex
        vec![r".+Incomplete command\.", r".+Invalid command:.+"],
        // Edges - No mode transitions
        vec![],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}
