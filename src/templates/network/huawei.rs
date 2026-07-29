//! Huawei VRP device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Huawei VRP devices.
pub fn huawei_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Enable", &[r"^(HRP_M|HRP_S){0,1}<.+>\s*$"]),
            prompt_rule(
                "Config",
                &[
                    r"^(HRP_M|HRP_S)?\[(?:[^Yy][^\]]*|[Yy]|[Yy][^/][^\]]*|[Yy]/|[Yy]/[^Nn][^\]]*|[Yy]/[Nn][^\]]+)\]\s*$",
                ],
            ),
        ],
        write: vec![
            input_rule(
                "Save",
                false,
                "y",
                true,
                &[
                    r"Are you sure to continue\?\[Y/N\]:?\s*$",
                    r"startup saved-configuration file on peer device\?\[Y/N\]:?\s*$",
                    r"Warning: The current configuration will be written to the device\. Continue\? \[Y/N\]:?\s*$",
                    r"Warning: This command will invalidate the rule\. Continue\?\[Y/N\]:?\s*$",
                ],
            ),
            input_rule(
                "PasswordChange",
                false,
                "n",
                true,
                &[
                    r"The password needs to be changed, Continue\? \[Y/N\]:?\s*$",
                    r"The password needs to be changed\. Change now\? \[Y/N\]:?\s*$",
                ],
            ),
        ],
        more_regex: vec![r"\s*---- More ----\s*".to_string()],
        error_regex: vec![r"Error: .+$".to_string(), r"\^$".to_string()],
        edges: vec![
            transition_rule("Enable", "system-view", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Huawei VRP devices.
pub fn huawei() -> Result<DeviceHandler, ConnectError> {
    huawei_config().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netdriver_prompt_variants_are_supported_without_treating_confirmations_as_prompts() {
        let mut handler = huawei().expect("create Huawei handler");

        for prompt in [
            "HRP_M<USG6000v>",
            "HRP_S[USG6000V2-object-address-set-test obj]",
            "[USG6000V2-GigabitEthernet0/0/1]",
        ] {
            assert!(
                handler.read_prompt(prompt),
                "prompt should match: {prompt:?}"
            );
        }
        for prompt in ["[Y/N]", "[y/n]"] {
            assert!(
                !handler.read_prompt(prompt),
                "confirmation should not match a config prompt: {prompt:?}"
            );
        }
    }

    #[test]
    fn netdriver_confirmation_and_password_change_prompts_are_supported() {
        let mut handler = huawei().expect("create Huawei handler");

        assert_eq!(
            handler
                .read_need_write("Warning: This command will invalidate the rule. Continue?[Y/N]"),
            Some(("y".to_string(), true))
        );
        for prompt in [
            "The password needs to be changed, Continue? [Y/N]",
            "The password needs to be changed. Change now? [Y/N]:",
        ] {
            assert_eq!(
                handler.read_need_write(prompt),
                Some(("n".to_string(), true)),
                "password-change prompt should match: {prompt:?}"
            );
        }
    }
}
