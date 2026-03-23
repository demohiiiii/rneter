//! DPTech Firewall device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for DPTech devices.
pub fn dptech() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\[.+\]\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}<.+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r" --More\(CTRL\+C break\)-- "],
        // Error regex
        vec![
            r"% Unknown command.*",
            r"Can't find the .+ object",
            r".*not exist.*",
            r".*item is longer.*",
            r"Failed.*",
            r"Undefined error.*",
            r"% Command can not contain:.+",
            r"Invalid parameter.*",
            r"% Ambiguous command.",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "conf-mode".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "end".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
        ],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}
