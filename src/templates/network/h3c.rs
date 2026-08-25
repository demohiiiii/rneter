//! H3C Comware device template.

use crate::device::{DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, transition_rule};
use crate::error::ConnectError;
use crate::session::{Command, HookAction, SessionHooks, SessionOperation};
use std::collections::HashMap;

/// Exports the underlying handler configuration for H3C devices.
pub fn h3c_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Enable", &[r"^(RBM_P|RBM_S)?<.+>\s*$"]),
            prompt_rule(
                "Config",
                &[
                    r"^(RBM_P|RBM_S)?\[(?:[^Yy][^\]]*|[Yy]|[Yy][^/][^\]]*|[Yy]/|[Yy]/[^Nn][^\]]*|[Yy]/[Nn][^\]]+)\]\s*$",
                ],
            ),
        ],
        write: vec![
            input_rule(
                "SaveConfirm",
                false,
                "Y",
                true,
                &[
                    r"The current configuration will be written to the device\. Are you sure\? \[Y/N\]:?\s*$",
                    r"flash:/startup\.cfg exists, overwrite\? \[Y/N\]:?\s*$",
                    r"Are you sure you want to continue the save operation\? \[Y/N\]:?\s*$",
                ],
            ),
            input_rule(
                "KeepFilename",
                false,
                "\n",
                true,
                &[r"\(To leave the existing filename unchanged, press the enter key\):?\s*$"],
            ),
            input_rule(
                "PasswordExpiry",
                false,
                "N",
                true,
                &[r"Your password will expire in \d+ days\. Do you want to change it\?\s*$"],
            ),
        ],
        more_regex: vec![r"\s*---- More ----\s*".to_string()],
        error_regex: vec![
            r".+\^.+".to_string(),
            r".+%.+".to_string(),
            r".+doesn't exist.+".to_string(),
            r".+does not exist.+".to_string(),
            r"Object group with given name exists with different type.".to_string(),
            r"Permission denied\.".to_string(),
            r"Failed to apply .+".to_string(),
        ],
        edges: vec![
            transition_rule("Enable", "system-view", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        hooks: SessionHooks {
            after_connect: vec![HookAction::new(
                "disable-paging",
                SessionOperation::from(Command {
                    mode: "Enable".to_string(),
                    command: "screen-length disable".to_string(),
                    ..Command::default()
                }),
            )],
            ..SessionHooks::default()
        },
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for H3C devices.
pub fn h3c() -> Result<DeviceHandler, ConnectError> {
    h3c_config().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netdriver_prompt_variants_are_supported_without_treating_confirmations_as_prompts() {
        let mut handler = h3c().expect("create H3C handler");

        for prompt in ["\0<hostname>", "RBM_P<hostname>", "RBM_S[hostname]"] {
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
    fn netdriver_interactive_prompts_are_supported() {
        let mut handler = h3c().expect("create H3C handler");

        for prompt in [
            "The current configuration will be written to the device. Are you sure? [Y/N]:",
            "flash:/startup.cfg exists, overwrite? [Y/N]:",
            "Are you sure you want to continue the save operation? [Y/N]:",
        ] {
            assert_eq!(
                handler.read_need_write(prompt),
                Some(("Y".to_string(), true)),
                "confirmation should match: {prompt:?}"
            );
        }
        assert_eq!(
            handler.read_need_write(
                "(To leave the existing filename unchanged, press the enter key):"
            ),
            Some(("\n".to_string(), true))
        );
        assert_eq!(
            handler
                .read_need_write("Your password will expire in 30 days. Do you want to change it?"),
            Some(("N".to_string(), true))
        );
    }
}
