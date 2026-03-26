use crate::device::{DeviceInputRule, input_rule};
use crate::error::ConnectError;
use crate::session::{
    Command, CommandDynamicParams, DeviceFileTransferDirection, DeviceFileTransferProtocol,
    DeviceFileTransferRequest,
};

const DEFAULT_TRANSFER_TIMEOUT_SECS: u64 = 300;

fn with_newline(value: &str) -> String {
    format!("{value}\n")
}

pub(crate) fn cisco_like_device_transfer_input_rules() -> Vec<DeviceInputRule> {
    vec![
        input_rule(
            "TransferRemoteHost",
            true,
            "TransferRemoteHost",
            true,
            &[r"(?i)^Address or name of remote host.*\?\s*$"],
        ),
        input_rule(
            "TransferSourceUsername",
            true,
            "TransferSourceUsername",
            true,
            &[r"(?i)^Source username.*\?\s*$"],
        ),
        input_rule(
            "TransferDestinationUsername",
            true,
            "TransferDestinationUsername",
            true,
            &[r"(?i)^Destination username.*\?\s*$"],
        ),
        input_rule(
            "TransferSourcePath",
            true,
            "TransferSourcePath",
            true,
            &[r"(?i)^Source (?:file ?name|filename).*\?\s*$"],
        ),
        input_rule(
            "TransferDestinationPath",
            true,
            "TransferDestinationPath",
            true,
            &[r"(?i)^Destination (?:file ?name|filename).*\?\s*$"],
        ),
        input_rule(
            "TransferPassword",
            true,
            "TransferPassword",
            false,
            &[r"(?i)^.*password.*:\s*$"],
        ),
        input_rule(
            "TransferConfirm",
            false,
            "\n",
            false,
            &[r"(?i)^.*\[confirm\]\s*$"],
        ),
        input_rule(
            "TransferOverwrite",
            false,
            "y\n",
            false,
            &[r"(?i)^.*(?:overwrite|over write).*\[(?:y\/n|yes\/no)\].*$"],
        ),
    ]
}

fn validate_transfer_request(
    template: &str,
    request: &DeviceFileTransferRequest,
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
    if matches!(request.protocol, DeviceFileTransferProtocol::Scp) {
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

/// Build a CLI command for device-side SCP/TFTP transfer on supported built-in templates.
pub fn build_file_transfer_command(
    template: &str,
    request: &DeviceFileTransferRequest,
) -> Result<Command, ConnectError> {
    validate_transfer_request(template, request)?;

    let protocol = match request.protocol {
        DeviceFileTransferProtocol::Scp => "scp",
        DeviceFileTransferProtocol::Tftp => "tftp",
    };

    let command = match request.direction {
        DeviceFileTransferDirection::ToDevice => {
            format!("copy {protocol}: {}", request.device_path)
        }
        DeviceFileTransferDirection::FromDevice => {
            format!("copy {} {protocol}:", request.device_path)
        }
    };

    let mut dyn_params = CommandDynamicParams {
        transfer_remote_host: Some(with_newline(&request.server_addr)),
        ..CommandDynamicParams::default()
    };

    match request.direction {
        DeviceFileTransferDirection::ToDevice => {
            dyn_params.transfer_source_path = Some(with_newline(&request.remote_path));
            dyn_params.transfer_destination_path = Some("\n".to_string());
        }
        DeviceFileTransferDirection::FromDevice => {
            dyn_params.transfer_destination_path = Some(with_newline(&request.remote_path));
        }
    }

    if matches!(request.protocol, DeviceFileTransferProtocol::Scp) {
        let username = request.username.as_deref().unwrap_or_default();
        let password = request.password.as_deref().unwrap_or_default();
        match request.direction {
            DeviceFileTransferDirection::ToDevice => {
                dyn_params.transfer_source_username = Some(with_newline(username));
            }
            DeviceFileTransferDirection::FromDevice => {
                dyn_params.transfer_destination_username = Some(with_newline(username));
            }
        }
        dyn_params.transfer_password = Some(with_newline(password));
    }

    Ok(Command {
        mode: request.mode.clone(),
        command,
        timeout: Some(
            request
                .timeout_secs
                .unwrap_or(DEFAULT_TRANSFER_TIMEOUT_SECS),
        ),
        dyn_params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::cisco;

    #[test]
    fn build_cisco_scp_to_device_command_sets_expected_dyn_params() {
        let request = DeviceFileTransferRequest::new(
            DeviceFileTransferProtocol::Scp,
            DeviceFileTransferDirection::ToDevice,
            "192.0.2.10".to_string(),
            "/pub/image.bin".to_string(),
            "flash:/image.bin".to_string(),
        )
        .with_credentials("deploy".to_string(), "secret".to_string());

        let command = build_file_transfer_command("cisco", &request).expect("build transfer");

        assert_eq!(command.mode, "Enable");
        assert_eq!(command.command, "copy scp: flash:/image.bin");
        assert_eq!(command.timeout, Some(DEFAULT_TRANSFER_TIMEOUT_SECS));
        assert_eq!(
            command.dyn_params.transfer_remote_host.as_deref(),
            Some("192.0.2.10\n")
        );
        assert_eq!(
            command.dyn_params.transfer_source_username.as_deref(),
            Some("deploy\n")
        );
        assert_eq!(
            command.dyn_params.transfer_source_path.as_deref(),
            Some("/pub/image.bin\n")
        );
        assert_eq!(
            command.dyn_params.transfer_destination_path.as_deref(),
            Some("\n")
        );
        assert_eq!(
            command.dyn_params.transfer_password.as_deref(),
            Some("secret\n")
        );
    }

    #[test]
    fn build_tftp_from_device_command_omits_scp_credentials() {
        let request = DeviceFileTransferRequest::new(
            DeviceFileTransferProtocol::Tftp,
            DeviceFileTransferDirection::FromDevice,
            "198.51.100.20".to_string(),
            "configs/r1.cfg".to_string(),
            "startup-config".to_string(),
        )
        .with_timeout_secs(900);

        let command = build_file_transfer_command("arista", &request).expect("build transfer");

        assert_eq!(command.command, "copy startup-config tftp:");
        assert_eq!(command.timeout, Some(900));
        assert!(command.dyn_params.transfer_password.is_none());
        assert_eq!(
            command.dyn_params.transfer_destination_path.as_deref(),
            Some("configs/r1.cfg\n")
        );
    }

    #[test]
    fn scp_builder_requires_credentials() {
        let request = DeviceFileTransferRequest::new(
            DeviceFileTransferProtocol::Scp,
            DeviceFileTransferDirection::FromDevice,
            "198.51.100.20".to_string(),
            "configs/r1.cfg".to_string(),
            "startup-config".to_string(),
        );

        let err = build_file_transfer_command("cisco", &request).expect_err("should fail");
        assert!(matches!(err, ConnectError::InvalidTransferRequest(_)));
    }

    #[test]
    fn unsupported_template_returns_transfer_not_supported() {
        let request = DeviceFileTransferRequest::new(
            DeviceFileTransferProtocol::Tftp,
            DeviceFileTransferDirection::ToDevice,
            "198.51.100.20".to_string(),
            "images/fw.bin".to_string(),
            "flash:/fw.bin".to_string(),
        );

        let err = build_file_transfer_command("huawei", &request).expect_err("should fail");
        assert!(matches!(err, ConnectError::TransferNotSupported(_)));
    }

    #[test]
    fn cisco_template_recognizes_transfer_prompts() {
        let mut handler = cisco().expect("cisco handler");
        handler.dyn_param.insert(
            "TransferRemoteHost".to_string(),
            "198.51.100.20\n".to_string(),
        );
        handler
            .dyn_param
            .insert("TransferSourceUsername".to_string(), "deploy\n".to_string());
        handler
            .dyn_param
            .insert("TransferDestinationPath".to_string(), "\n".to_string());
        handler
            .dyn_param
            .insert("TransferPassword".to_string(), "secret\n".to_string());

        assert_eq!(
            handler.read_need_write("Address or name of remote host []?"),
            Some(("198.51.100.20\n".to_string(), true))
        );
        assert_eq!(
            handler.read_need_write("Source username []?"),
            Some(("deploy\n".to_string(), true))
        );
        assert_eq!(
            handler.read_need_write("Password:"),
            Some(("secret\n".to_string(), false))
        );
        assert_eq!(
            handler.read_need_write("Destination filename [image.bin]?"),
            Some(("\n".to_string(), true))
        );
    }
}
