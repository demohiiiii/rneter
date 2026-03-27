use once_cell::sync::Lazy;

use super::command_flow_template::{
    CommandFlowTemplate, CommandFlowTemplatePrompt, CommandFlowTemplateStep,
    CommandFlowTemplateText, CommandFlowTemplateVar, CommandFlowTemplateVarKind,
};

const DEFAULT_TRANSFER_TIMEOUT_SECS: u64 = 300;

static CISCO_LIKE_COMMAND_FLOW_TEMPLATE: Lazy<CommandFlowTemplate> = Lazy::new(|| {
    CommandFlowTemplate::new(
        "cisco_like_copy",
        vec![
            CommandFlowTemplateStep::new(CommandFlowTemplateText::if_equals(
                "direction",
                "to_device",
                CommandFlowTemplateText::concat(vec![
                    CommandFlowTemplateText::literal("copy "),
                    CommandFlowTemplateText::var("protocol"),
                    CommandFlowTemplateText::literal(": "),
                    CommandFlowTemplateText::var("device_path"),
                ]),
                Some(CommandFlowTemplateText::concat(vec![
                    CommandFlowTemplateText::literal("copy "),
                    CommandFlowTemplateText::var("device_path"),
                    CommandFlowTemplateText::literal(" "),
                    CommandFlowTemplateText::var("protocol"),
                    CommandFlowTemplateText::literal(":"),
                ])),
            ))
            .with_timeout_secs(DEFAULT_TRANSFER_TIMEOUT_SECS)
            .with_prompts(vec![
                CommandFlowTemplatePrompt::new(
                    vec![r"(?i)^Address or name of remote host.*\?\s*$".to_string()],
                    CommandFlowTemplateText::var("server_addr"),
                )
                .with_append_newline(true)
                .with_record_input(true),
                CommandFlowTemplatePrompt::new(
                    vec![r"(?i)^Source (?:file ?name|filename).*\?\s*$".to_string()],
                    CommandFlowTemplateText::if_equals(
                        "direction",
                        "to_device",
                        CommandFlowTemplateText::var("remote_path"),
                        None,
                    ),
                )
                .with_append_newline(true)
                .with_record_input(true),
                CommandFlowTemplatePrompt::new(
                    vec![r"(?i)^Destination (?:file ?name|filename).*\?\s*$".to_string()],
                    CommandFlowTemplateText::if_equals(
                        "direction",
                        "from_device",
                        CommandFlowTemplateText::var("remote_path"),
                        None,
                    ),
                )
                .with_append_newline(true)
                .with_record_input(true),
                CommandFlowTemplatePrompt::new(
                    vec![
                        r"(?i)^Source username.*\?\s*$".to_string(),
                        r"(?i)^Destination username.*\?\s*$".to_string(),
                    ],
                    CommandFlowTemplateText::var("transfer_username"),
                )
                .with_append_newline(true)
                .with_record_input(true),
                CommandFlowTemplatePrompt::new(
                    vec![r"(?i)^.*password.*:\s*$".to_string()],
                    CommandFlowTemplateText::var("transfer_password"),
                )
                .with_append_newline(true),
                CommandFlowTemplatePrompt::new(
                    vec![r"(?i)^.*\[confirm\]\s*$".to_string()],
                    CommandFlowTemplateText::literal(""),
                )
                .with_append_newline(true),
                CommandFlowTemplatePrompt::new(
                    vec![r"(?i)^.*(?:overwrite|over write).*\[(?:y\/n|yes\/no)\].*$".to_string()],
                    CommandFlowTemplateText::literal("y"),
                )
                .with_append_newline(true),
            ]),
        ],
    )
    .with_description("Generic interactive SCP/TFTP copy flow for Cisco-like CLIs.")
    .with_default_mode("Enable")
    .with_vars(vec![
        CommandFlowTemplateVar::new("protocol")
            .with_label("Transfer Protocol")
            .with_description("Transfer protocol used by the device-side copy workflow.")
            .with_required(true)
            .with_options(["scp", "tftp"]),
        CommandFlowTemplateVar::new("direction")
            .with_label("Transfer Direction")
            .with_description(
                "Choose whether the device pulls a file from the server or pushes a file to it.",
            )
            .with_required(true)
            .with_options(["to_device", "from_device"]),
        CommandFlowTemplateVar::new("server_addr")
            .with_label("Server Address")
            .with_description("SCP/TFTP server reachable from the target device.")
            .with_required(true)
            .with_placeholder("192.0.2.10"),
        CommandFlowTemplateVar::new("remote_path")
            .with_label("Remote Path")
            .with_description("Remote filename or path used on the transfer server.")
            .with_required(true)
            .with_placeholder("/images/image.bin"),
        CommandFlowTemplateVar::new("device_path")
            .with_label("Device Path")
            .with_description("Destination or source path on the target device.")
            .with_required(true)
            .with_placeholder("flash:/image.bin"),
        CommandFlowTemplateVar::new("transfer_username")
            .with_label("Transfer Username")
            .with_description("Required when the protocol is SCP.")
            .with_placeholder("backup"),
        CommandFlowTemplateVar::new("transfer_password")
            .with_label("Transfer Password")
            .with_description("Required when the protocol is SCP.")
            .with_kind(CommandFlowTemplateVarKind::Secret),
    ])
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
        assert_eq!(template.vars.len(), 7);
    }

    #[test]
    fn cisco_like_copy_template_renders_scp_to_device_flow() {
        let template = cisco_like_copy_template();
        let flow = template
            .to_command_flow(
                &CommandFlowTemplateRuntime::new()
                    .with_default_mode("Enable")
                    .with_vars(json!({
                        "protocol": "scp",
                        "direction": "to_device",
                        "server_addr": "192.0.2.10",
                        "remote_path": "/pub/image.bin",
                        "device_path": "flash:/image.bin",
                        "transfer_username": "deploy",
                        "transfer_password": "secret",
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
        assert_eq!(command.interaction.prompts[2].response, "\n");
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
                        "protocol": "tftp",
                        "direction": "from_device",
                        "server_addr": "198.51.100.20",
                        "remote_path": "configs/r1.cfg",
                        "device_path": "startup-config",
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
        assert_eq!(command.interaction.prompts[1].response, "\n");
        assert_eq!(command.interaction.prompts[2].response, "configs/r1.cfg\n");
    }

    #[test]
    fn cisco_like_copy_template_renders_empty_optional_scp_credentials() {
        let template = cisco_like_copy_template();
        let flow = template
            .to_command_flow(&CommandFlowTemplateRuntime::new().with_vars(json!({
                "protocol": "scp",
                "direction": "from_device",
                "server_addr": "198.51.100.20",
                "remote_path": "configs/r1.cfg",
                "device_path": "startup-config",
            })))
            .expect("render flow");

        assert_eq!(flow.steps.len(), 1);
        assert_eq!(flow.steps[0].interaction.prompts[3].response, "\n");
        assert_eq!(flow.steps[0].interaction.prompts[4].response, "\n");
    }
}
