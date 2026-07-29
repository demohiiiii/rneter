//! Check Point Security Gateway device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, prompt_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Check Point Security Gateway devices.
pub fn checkpoint_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![prompt_rule(
            "Enable",
            &[r"^(?:\[[^\]\r\n]+\]\s*)?\S+\s*>\s*$"],
        )],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netdriver_context_prompt_variants_are_supported() {
        let mut handler = checkpoint().expect("create Check Point handler");

        for prompt in [
            "hostname>",
            "[WARNING! Local Member] hostname> ",
            "[Global] hostname> ",
            "\r\nhostname> \r\n",
        ] {
            assert!(
                handler.read_prompt(prompt),
                "prompt should match: {prompt:?}"
            );
        }
    }
}
