//! Fortinet FortiGate device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for Fortinet FortiGate devices.
pub fn fortinet() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt - Fortinet only has Enable mode
        vec![("Enable".to_string(), vec![r"^\r{0,1}\S+\s*#\s*$"])],
        // Prompt with sys (VDOM support)
        vec![(
            "VDOMEnable".to_string(),
            "VDOM",
            r"^\r{0,1}\S+\s*\((?<VDOM>\S+)\)\s*#\s*$".to_string(),
        )],
        // Write
        vec![],
        // More regex
        vec![r"--More--"],
        // Error regex
        vec![r"Unknown action.*", r"Command fail.*"],
        // Edges - Fortinet has no mode transitions
        vec![],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}
