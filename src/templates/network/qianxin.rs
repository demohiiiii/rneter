//! QiAnXin NSG device template.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

/// Returns a `DeviceHandler` configured for QiAnXin NSG devices.
pub fn qianxin() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\S+-config.*]\s*$"]),
            ("Enable".to_string(), vec![r"^\S+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"--More--"],
        // Error regex
        vec![
            r"% Unknown command.",
            r"% Command incomplete.",
            r"%?\s+Invalid parameter.*",
            r"\s+Valid name can.*",
            r"\s+Repetitions with Object.*",
            r".+ exist",
            r"\s+Start larger than end",
            r"\s+Name can not repeat",
            r"Object .+ referenced by other module",
            r"Object service has been referenced",
            r"Object \[.+\] is quoted",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "config terminal".to_string(),
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
