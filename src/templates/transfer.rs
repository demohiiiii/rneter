use crate::error::ConnectError;
use crate::session::{Command, CommandFlow, CommandInteraction, PromptResponseRule};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const DEFAULT_TRANSFER_TIMEOUT_SECS: u64 = 300;

fn default_transfer_mode() -> String {
    "Enable".to_string()
}

/// File transfer protocol executed by the device CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileTransferProtocol {
    Scp,
    Tftp,
}

/// Direction of the transfer from the device's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileTransferDirection {
    /// Pull a file from the external server onto the device.
    ToDevice,
    /// Push a file from the device to the external server.
    FromDevice,
}

/// Template-layer request for building CLI-driven file transfer flows.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileTransferRequest {
    /// Transfer protocol used by the device.
    pub protocol: FileTransferProtocol,
    /// Whether the device is importing or exporting the file.
    pub direction: FileTransferDirection,
    /// Address or DNS name of the external SCP/TFTP server reachable from the device.
    pub server_addr: String,
    /// Path on the external SCP/TFTP server.
    pub remote_path: String,
    /// Path on the device filesystem, e.g. `flash:/image.bin`.
    pub device_path: String,
    /// Optional SCP username.
    #[serde(default)]
    pub username: Option<String>,
    /// Optional SCP password.
    #[serde(default)]
    pub password: Option<String>,
    /// Device mode used to run the transfer command. Defaults to `Enable`.
    #[serde(default = "default_transfer_mode")]
    pub mode: String,
    /// Optional transfer timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl FileTransferRequest {
    /// Build a new CLI transfer request with an `Enable`-mode default.
    pub fn new(
        protocol: FileTransferProtocol,
        direction: FileTransferDirection,
        server_addr: String,
        remote_path: String,
        device_path: String,
    ) -> Self {
        Self {
            protocol,
            direction,
            server_addr,
            remote_path,
            device_path,
            username: None,
            password: None,
            mode: default_transfer_mode(),
            timeout_secs: None,
        }
    }

    /// Attach SCP credentials for the transfer.
    pub fn with_credentials(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    /// Override the device mode used to run the transfer command.
    pub fn with_mode(mut self, mode: String) -> Self {
        self.mode = mode;
        self
    }

    /// Override the transfer timeout in seconds.
    pub fn with_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = Some(timeout_secs);
        self
    }
}

fn with_newline(value: &str) -> String {
    format!("{value}\n")
}

fn prompt_response_rule(
    patterns: &[&str],
    response: String,
    record_input: bool,
) -> PromptResponseRule {
    PromptResponseRule::new(
        patterns
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect(),
        response,
    )
    .with_record_input(record_input)
}

fn validate_transfer_request(
    template: &str,
    request: &FileTransferRequest,
) -> Result<(), ConnectError> {
    if request.server_addr.trim().is_empty() {
        return Err(ConnectError::InvalidTransferRequest(
            "server_addr cannot be empty".to_string(),
        ));
    }
    if request.remote_path.trim().is_empty() {
        return Err(ConnectError::InvalidTransferRequest(
            "remote_path cannot be empty".to_string(),
        ));
    }
    if request.device_path.trim().is_empty() {
        return Err(ConnectError::InvalidTransferRequest(
            "device_path cannot be empty".to_string(),
        ));
    }
    if matches!(request.protocol, FileTransferProtocol::Scp) {
        if request.username.as_deref().unwrap_or("").trim().is_empty() {
            return Err(ConnectError::InvalidTransferRequest(
                "scp transfers require username".to_string(),
            ));
        }
        if request.password.as_deref().unwrap_or("").is_empty() {
            return Err(ConnectError::InvalidTransferRequest(
                "scp transfers require password".to_string(),
            ));
        }
    }

    let template_key = template.to_ascii_lowercase();
    match template_key.as_str() {
        "cisco" | "arista" | "chaitin" | "maipu" | "venustech" => Ok(()),
        _ => Err(ConnectError::TransferNotSupported(format!(
            "template '{template}' does not yet expose a built-in CLI transfer workflow"
        ))),
    }
}

fn build_cisco_like_transfer_interaction(request: &FileTransferRequest) -> CommandInteraction {
    let mut prompts = vec![prompt_response_rule(
        &[r"(?i)^Address or name of remote host.*\?\s*$"],
        with_newline(&request.server_addr),
        true,
    )];

    match request.direction {
        FileTransferDirection::ToDevice => {
            prompts.push(prompt_response_rule(
                &[r"(?i)^Source (?:file ?name|filename).*\?\s*$"],
                with_newline(&request.remote_path),
                true,
            ));
            prompts.push(prompt_response_rule(
                &[r"(?i)^Destination (?:file ?name|filename).*\?\s*$"],
                "\n".to_string(),
                true,
            ));
        }
        FileTransferDirection::FromDevice => {
            prompts.push(prompt_response_rule(
                &[r"(?i)^Destination (?:file ?name|filename).*\?\s*$"],
                with_newline(&request.remote_path),
                true,
            ));
        }
    }

    if matches!(request.protocol, FileTransferProtocol::Scp) {
        let username = request.username.as_deref().unwrap_or_default();
        let password = request.password.as_deref().unwrap_or_default();

        match request.direction {
            FileTransferDirection::ToDevice => prompts.push(prompt_response_rule(
                &[r"(?i)^Source username.*\?\s*$"],
                with_newline(username),
                true,
            )),
            FileTransferDirection::FromDevice => prompts.push(prompt_response_rule(
                &[r"(?i)^Destination username.*\?\s*$"],
                with_newline(username),
                true,
            )),
        }

        prompts.push(prompt_response_rule(
            &[r"(?i)^.*password.*:\s*$"],
            with_newline(password),
            false,
        ));
    }

    prompts.push(prompt_response_rule(
        &[r"(?i)^.*\[confirm\]\s*$"],
        "\n".to_string(),
        false,
    ));
    prompts.push(prompt_response_rule(
        &[r"(?i)^.*(?:overwrite|over write).*\[(?:y\/n|yes\/no)\].*$"],
        "y\n".to_string(),
        false,
    ));

    CommandInteraction { prompts }
}

/// Build a CLI command for device-side SCP/TFTP transfer on supported built-in templates.
pub fn build_file_transfer_command(
    template: &str,
    request: &FileTransferRequest,
) -> Result<Command, ConnectError> {
    let mut flow = build_file_transfer_flow(template, request)?;
    match flow.steps.len() {
        1 => Ok(flow.steps.remove(0)),
        step_count => Err(ConnectError::InvalidTransferRequest(format!(
            "template '{template}' produced {step_count} commands; use build_file_transfer_flow instead"
        ))),
    }
}

/// Build a CLI command flow for device-side SCP/TFTP transfer on supported built-in templates.
pub fn build_file_transfer_flow(
    template: &str,
    request: &FileTransferRequest,
) -> Result<CommandFlow, ConnectError> {
    validate_transfer_request(template, request)?;

    let protocol = match request.protocol {
        FileTransferProtocol::Scp => "scp",
        FileTransferProtocol::Tftp => "tftp",
    };

    let command = match request.direction {
        FileTransferDirection::ToDevice => {
            format!("copy {protocol}: {}", request.device_path)
        }
        FileTransferDirection::FromDevice => {
            format!("copy {} {protocol}:", request.device_path)
        }
    };

    Ok(CommandFlow::new(vec![Command {
        mode: request.mode.clone(),
        command,
        timeout: Some(
            request
                .timeout_secs
                .unwrap_or(DEFAULT_TRANSFER_TIMEOUT_SECS),
        ),
        interaction: build_cisco_like_transfer_interaction(request),
        ..Command::default()
    }]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_transfer_request_builder_overrides_defaults() {
        let request = FileTransferRequest::new(
            FileTransferProtocol::Scp,
            FileTransferDirection::ToDevice,
            "192.0.2.10".to_string(),
            "/images/new.bin".to_string(),
            "flash:/new.bin".to_string(),
        )
        .with_credentials("backup".to_string(), "secret".to_string())
        .with_mode("Config".to_string())
        .with_timeout_secs(300);

        assert_eq!(request.protocol, FileTransferProtocol::Scp);
        assert_eq!(request.direction, FileTransferDirection::ToDevice);
        assert_eq!(request.server_addr, "192.0.2.10");
        assert_eq!(request.remote_path, "/images/new.bin");
        assert_eq!(request.device_path, "flash:/new.bin");
        assert_eq!(request.username.as_deref(), Some("backup"));
        assert_eq!(request.password.as_deref(), Some("secret"));
        assert_eq!(request.mode, "Config");
        assert_eq!(request.timeout_secs, Some(300));
    }

    #[test]
    fn build_cisco_scp_to_device_command_sets_expected_interaction() {
        let request = FileTransferRequest::new(
            FileTransferProtocol::Scp,
            FileTransferDirection::ToDevice,
            "192.0.2.10".to_string(),
            "/pub/image.bin".to_string(),
            "flash:/image.bin".to_string(),
        )
        .with_credentials("deploy".to_string(), "secret".to_string());

        let command = build_file_transfer_command("cisco", &request).expect("build transfer");

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
    fn build_tftp_from_device_command_sets_runtime_interaction_only() {
        let request = FileTransferRequest::new(
            FileTransferProtocol::Tftp,
            FileTransferDirection::FromDevice,
            "198.51.100.20".to_string(),
            "configs/r1.cfg".to_string(),
            "startup-config".to_string(),
        )
        .with_timeout_secs(900);

        let command = build_file_transfer_command("arista", &request).expect("build transfer");

        assert_eq!(command.command, "copy startup-config tftp:");
        assert_eq!(command.timeout, Some(900));
        assert!(command.dyn_params.is_empty());
        assert_eq!(command.interaction.prompts.len(), 4);
        assert_eq!(command.interaction.prompts[0].response, "198.51.100.20\n");
        assert_eq!(command.interaction.prompts[1].response, "configs/r1.cfg\n");
    }

    #[test]
    fn build_transfer_flow_wraps_single_command_step() {
        let request = FileTransferRequest::new(
            FileTransferProtocol::Tftp,
            FileTransferDirection::ToDevice,
            "198.51.100.20".to_string(),
            "images/fw.bin".to_string(),
            "flash:/fw.bin".to_string(),
        );

        let flow = build_file_transfer_flow("cisco", &request).expect("build flow");

        assert!(flow.stop_on_error);
        assert_eq!(flow.steps.len(), 1);
        assert_eq!(flow.steps[0].command, "copy tftp: flash:/fw.bin");
    }

    #[test]
    fn scp_builder_requires_credentials() {
        let request = FileTransferRequest::new(
            FileTransferProtocol::Scp,
            FileTransferDirection::FromDevice,
            "198.51.100.20".to_string(),
            "configs/r1.cfg".to_string(),
            "startup-config".to_string(),
        );

        let err = build_file_transfer_command("cisco", &request).expect_err("should fail");
        assert!(matches!(err, ConnectError::InvalidTransferRequest(_)));
    }

    #[test]
    fn unsupported_template_returns_transfer_not_supported() {
        let request = FileTransferRequest::new(
            FileTransferProtocol::Tftp,
            FileTransferDirection::ToDevice,
            "198.51.100.20".to_string(),
            "images/fw.bin".to_string(),
            "flash:/fw.bin".to_string(),
        );

        let err = build_file_transfer_command("huawei", &request).expect_err("should fail");
        assert!(matches!(err, ConnectError::TransferNotSupported(_)));
    }
}
