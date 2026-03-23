//! Palo Alto Networks device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for Palo Alto Networks devices.
pub fn paloalto() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\S+@\S+#\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}\S+@\S+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"(--more--)|(lines \d+-\d+ )"],
        // Error regex
        vec![
            r"Unknown command:.*",
            r"Invalid syntax.",
            r"Server error:.*",
            r"Validation Error:.*",
            r"Commit failed",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "configure".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "exit".to_string(),
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
