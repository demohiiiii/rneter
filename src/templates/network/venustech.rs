//! Venustech USG device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for Venustech USG devices.
pub fn venustech() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\S+\(\S+\)#\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}[^\s#]+#\s*$"]),
            ("Login".to_string(), vec![r"^\r{0,1}[^\s<]+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "EnablePassword".to_string(),
            (true, "EnablePassword".to_string(), true),
            vec![r"(Enable )?Password:"],
        )],
        // More regex
        vec![r"--More-- \(\d+% of \d+ bytes\)"],
        // Error regex
        vec![r"^%.+", r".+not exist!"],
        // Edges
        vec![
            (
                "Login".to_string(),
                "enable".to_string(),
                "Enable".to_string(),
                false,
                false,
            ),
            (
                "Enable".to_string(),
                "configure terminal".to_string(),
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
            (
                "Enable".to_string(),
                "exit".to_string(),
                "Login".to_string(),
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
