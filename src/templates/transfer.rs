use once_cell::sync::Lazy;

use super::command_flow_template::{
    CommandFlowTemplate, CommandFlowTemplatePrompt, CommandFlowTemplateStep,
};

const DEFAULT_TRANSFER_TIMEOUT_SECS: u64 = 300;

static CISCO_LIKE_COMMAND_FLOW_TEMPLATE: Lazy<CommandFlowTemplate> = Lazy::new(|| {
    CommandFlowTemplate::new(
        "cisco_like_copy",
        vec![
            CommandFlowTemplateStep::from_template("{{command}}")
                .with_timeout_secs(DEFAULT_TRANSFER_TIMEOUT_SECS)
                .with_prompts(vec![
                    CommandFlowTemplatePrompt::from_template(
                        vec![r"(?i)^Address or name of remote host.*\?\s*$".to_string()],
                        "{{server_addr}}",
                    )
                    .with_append_newline(true)
                    .with_record_input(true),
                    CommandFlowTemplatePrompt::new(
                        vec![r"(?i)^Source (?:file ?name|filename).*\?\s*$".to_string()],
                        "{{remote_path}}",
                    )
                    .with_append_newline(true)
                    .with_record_input(true),
                    CommandFlowTemplatePrompt::new(
                        vec![r"(?i)^Destination (?:file ?name|filename).*\?\s*$".to_string()],
                        "{{remote_path}}",
                    )
                    .with_append_newline(true)
                    .with_record_input(true),
                    CommandFlowTemplatePrompt::from_template(
                        vec![
                            r"(?i)^Source username.*\?\s*$".to_string(),
                            r"(?i)^Destination username.*\?\s*$".to_string(),
                        ],
                        "{{transfer_username}}",
                    )
                    .with_append_newline(true)
                    .with_record_input(true),
                    CommandFlowTemplatePrompt::from_template(
                        vec![r"(?i)^.*password.*:\s*$".to_string()],
                        "{{transfer_password}}",
                    )
                    .with_append_newline(true),
                    CommandFlowTemplatePrompt::new(vec![r"(?i)^.*\[confirm\]\s*$".to_string()], "")
                        .with_append_newline(true),
                    CommandFlowTemplatePrompt::from_template(
                        vec![
                            r"(?i)^.*(?:overwrite|over write).*\[(?:y\/n|yes\/no)\].*$".to_string(),
                        ],
                        "{{overwrite_answer}}",
                    )
                    .with_append_newline(true),
                ]),
        ],
    )
    .with_default_mode("Enable")
});

/// Built-in copy workflow template for Cisco-like CLIs.
///
/// This template is intended for devices whose interactive copy prompts follow
/// Cisco-style wording, including built-in `cisco`, `arista`, `chaitin`,
/// `maipu`, and `venustech` handler profiles.
pub fn cisco_like_copy_template() -> CommandFlowTemplate {
    CISCO_LIKE_COMMAND_FLOW_TEMPLATE.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::CommandFlowTemplateRuntime;
    use serde_json::json;

    #[test]
    fn cisco_like_copy_template_exposes_expected_metadata() {
        let template = cisco_like_copy_template();

        assert_eq!(template.name, "cisco_like_copy");
        assert_eq!(template.default_mode.as_deref(), Some("Enable"));
        assert_eq!(template.steps.len(), 1);
    }

    #[test]
    fn cisco_like_copy_template_renders_scp_to_device_flow() {
        let template = cisco_like_copy_template();
        let flow = template
            .to_command_flow(
                &CommandFlowTemplateRuntime::new()
                    .with_default_mode("Enable")
                    .with_vars(json!({
                        "command": "copy scp: flash:/image.bin",
                        "server_addr": "192.0.2.10",
                        "remote_path": "/pub/image.bin",
                        "transfer_username": "deploy",
                        "transfer_password": "secret",
                        "overwrite_answer": "y",
                    })),
            )
            .expect("render flow");

        assert!(flow.stop_on_error);
        assert_eq!(flow.steps.len(), 1);

        let command = &flow.steps[0];
        assert_eq!(command.mode, "Enable");
        assert_eq!(command.command, "copy scp: flash:/image.bin");
        assert_eq!(command.timeout, Some(DEFAULT_TRANSFER_TIMEOUT_SECS));
        assert!(command.dyn_params.is_empty());
        assert_eq!(command.interaction.prompts.len(), 7);
        assert_eq!(command.interaction.prompts[0].response, "192.0.2.10\n");
        assert_eq!(command.interaction.prompts[1].response, "/pub/image.bin\n");
        assert_eq!(command.interaction.prompts[2].response, "/pub/image.bin\n");
        assert_eq!(command.interaction.prompts[3].response, "deploy\n");
        assert_eq!(command.interaction.prompts[4].response, "secret\n");
        assert_eq!(command.interaction.prompts[5].response, "\n");
        assert_eq!(command.interaction.prompts[6].response, "y\n");
    }

    #[test]
    fn cisco_like_copy_template_renders_tftp_from_device_flow() {
        let template = cisco_like_copy_template();
        let flow = template
            .to_command_flow(
                &CommandFlowTemplateRuntime::new()
                    .with_default_mode("Config")
                    .with_vars(json!({
                        "command": "copy startup-config tftp:",
                        "server_addr": "198.51.100.20",
                        "remote_path": "configs/r1.cfg",
                        "transfer_username": "",
                        "transfer_password": "",
                        "overwrite_answer": "y",
                    })),
            )
            .expect("render flow");

        let command = &flow.steps[0];

        assert_eq!(command.command, "copy startup-config tftp:");
        assert_eq!(command.mode, "Config");
        assert_eq!(command.timeout, Some(DEFAULT_TRANSFER_TIMEOUT_SECS));
        assert!(command.dyn_params.is_empty());
        assert_eq!(command.interaction.prompts.len(), 7);
        assert_eq!(command.interaction.prompts[0].response, "198.51.100.20\n");
        assert_eq!(command.interaction.prompts[1].response, "configs/r1.cfg\n");
        assert_eq!(command.interaction.prompts[2].response, "configs/r1.cfg\n");
        assert_eq!(command.interaction.prompts[6].response, "y\n");
    }

    #[test]
    fn cisco_like_copy_template_renders_empty_optional_scp_credentials() {
        let template = cisco_like_copy_template();
        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({
                "command": "copy startup-config scp:",
                "server_addr": "198.51.100.20",
                "remote_path": "configs/r1.cfg",
                "transfer_username": "",
                "transfer_password": "",
                "overwrite_answer": "y",
            })))
            .expect("render flow");

        assert_eq!(flow.steps.len(), 1);
        assert_eq!(flow.steps[0].interaction.prompts[3].response, "\n");
        assert_eq!(flow.steps[0].interaction.prompts[4].response, "\n");
        assert_eq!(flow.steps[0].interaction.prompts[6].response, "y\n");
    }

    #[test]
    fn cisco_like_copy_template_renders_custom_overwrite_answer() {
        let template = cisco_like_copy_template();
        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({
                "command": "copy scp: flash:/image.bin",
                "server_addr": "198.51.100.20",
                "remote_path": "/pub/image.bin",
                "transfer_username": "",
                "transfer_password": "",
                "overwrite_answer": "n",
            })))
            .expect("render flow");

        assert_eq!(flow.steps[0].interaction.prompts[6].response, "n\n");
    }
}
