//! Arista EOS device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for Arista EOS devices.
pub fn arista() -> Result<DeviceHandler, ConnectError> {
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
            vec![r"Password:"],
        )],
        // More regex
        vec![r" --More-- "],
        // Error regex
        vec![
            r"% Invalid input",
            r"% Ambiguous command",
            r"% Bad secret",
            r"% Unrecognized command",
            r"% Incomplete command",
            r"% Invalid port range .+",
            r"! Access VLAN does not exist. Creating vlan .+",
            r"% Address \S+ is already assigned to interface .+",
            r"% Removal of physical interfaces is not permitted",
            r"^% .+",
        ],
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
