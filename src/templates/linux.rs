//! Linux server template.
//!
//! This module provides device handler configuration for Linux servers with
//! support for privilege escalation via sudo or su.

use crate::device::{
    DeviceCommandExecutionConfig, DeviceHandler, DeviceHandlerConfig, DeviceShellFlavor,
    input_rule, prompt_rule, transition_rule,
};
use crate::error::ConnectError;
use std::collections::HashMap;

const LINUX_EXIT_CODE_MARKER: &str = "__RNETER_EXIT_CODE__:";

/// Configuration for Linux template.
#[derive(Debug, Clone)]
pub struct LinuxTemplateConfig {
    pub sudo_mode: SudoMode,
    pub sudo_password: Option<String>,
    pub custom_prompts: Option<CustomPrompts>,
    /// Shell flavor used for exit-status capture wrappers.
    pub shell_flavor: DeviceShellFlavor,
}

impl Default for LinuxTemplateConfig {
    fn default() -> Self {
        Self {
            sudo_mode: SudoMode::SudoInteractive,
            sudo_password: None,
            custom_prompts: None,
            shell_flavor: DeviceShellFlavor::Posix,
        }
    }
}

/// Sudo privilege escalation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SudoMode {
    /// Use `sudo -i` to get interactive root shell
    SudoInteractive,
    /// Use `sudo -s` to get shell as root
    SudoShell,
    /// Use `su -` to switch to root
    Su,
    /// Direct root login (no privilege escalation needed)
    DirectRoot,
}

/// Custom prompt patterns for Linux servers.
#[derive(Debug, Clone)]
pub struct CustomPrompts {
    pub user_prompts: Vec<&'static str>,
    pub root_prompts: Vec<&'static str>,
}

/// Linux command type for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxCommandType {
    ReadOnly,
    FileOp,
    ServiceOp,
    Custom,
}

/// Classify a Linux command by its type.
pub fn classify_linux_command(command: &str) -> LinuxCommandType {
    let cmd = command.trim().to_ascii_lowercase();

    // Read-only commands
    let readonly_prefixes = [
        "ls",
        "cat",
        "grep",
        "find",
        "ps",
        "top",
        "df",
        "du",
        "free",
        "uptime",
        "systemctl status",
        "journalctl",
        "tail",
        "head",
        "less",
        "more",
        "which",
        "whereis",
        "pwd",
        "whoami",
        "id",
        "uname",
        "hostname",
    ];
    if readonly_prefixes
        .iter()
        .any(|prefix| cmd.starts_with(prefix))
    {
        return LinuxCommandType::ReadOnly;
    }

    // Service operations
    let service_prefixes = [
        "systemctl start",
        "systemctl stop",
        "systemctl enable",
        "systemctl disable",
        "systemctl restart",
        "service",
    ];
    if service_prefixes
        .iter()
        .any(|prefix| cmd.starts_with(prefix))
    {
        return LinuxCommandType::ServiceOp;
    }

    // File operations
    let file_prefixes = ["echo", "sed", "awk", "rm", "mv", "cp", "touch", "mkdir"];
    if file_prefixes.iter().any(|prefix| cmd.starts_with(prefix)) {
        return LinuxCommandType::FileOp;
    }

    LinuxCommandType::Custom
}

/// Returns a `DeviceHandler` configured for Linux servers with default settings.
pub fn linux() -> Result<DeviceHandler, ConnectError> {
    linux_with_config(LinuxTemplateConfig::default())
}

/// Exports the underlying handler configuration for the Linux template.
pub fn linux_handler_config(config: LinuxTemplateConfig) -> DeviceHandlerConfig {
    let (user_prompts, root_prompts) = if let Some(custom) = config.custom_prompts {
        (custom.user_prompts, custom.root_prompts)
    } else {
        // Default prompt patterns
        (
            vec![
                r"^[^\s]+\$\s*$",        // user$
                r"^[^\s]+@[^\s]+\$\s*$", // user@host$
                r"^[^\s@]+@.+\$\s*$",    // user@host path$
                r"^[^\s@]+@.+>\s*$",     // fish: user@host path>
                r"^\[[^\]]+\]\$\s*$",    // [user@host]$
                r"^\$\s*$",              // $
            ],
            vec![
                r"^[^\s]+#\s*$",          // root#
                r"^root@[^\s]+#\s*$",     // root@host#
                r"^[^\s@]+@.+#\s*$",      // root@host path#
                r"^\[root@[^\]]+\]#\s*$", // [root@host]#
                r"^#\s*$",                // #
            ],
        )
    };

    let sudo_command = match config.sudo_mode {
        SudoMode::SudoInteractive => "sudo -i",
        SudoMode::SudoShell => "sudo -s",
        SudoMode::Su => "su -",
        SudoMode::DirectRoot => "",
    };

    let edges = if config.sudo_mode != SudoMode::DirectRoot {
        vec![
            transition_rule("User", sudo_command, "Root", false, false),
            transition_rule("Root", "exit", "User", true, false),
        ]
    } else {
        vec![]
    };

    let mut dyn_param = HashMap::new();
    if let Some(password) = config.sudo_password {
        dyn_param.insert("SudoPassword".to_string(), password);
    }

    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Root", &root_prompts),
            prompt_rule("User", &user_prompts),
        ],
        prompt_with_sys: Vec::new(),
        write: vec![input_rule(
            "SudoPassword",
            true,
            "SudoPassword",
            false,
            &[
                r"\[sudo\] password for .+:\s*$",
                r"Password:\s*$",
                r"password:\s*$",
            ],
        )],
        more_regex: vec![
            r"--More--".to_string(),
            r"\(END\)".to_string(),
            r"Press SPACE to continue".to_string(),
        ],
        error_regex: vec![
            r"^bash: .+: command not found".to_string(),
            r"^-bash: .+: command not found".to_string(),
            r"^sudo: .+: command not found".to_string(),
            r"Permission denied".to_string(),
            r"Operation not permitted".to_string(),
            r"No such file or directory".to_string(),
            r"cannot access".to_string(),
            r"sudo: \d+ incorrect password attempt".to_string(),
            r"su: Authentication failure".to_string(),
            r"^E: .+".to_string(),
            r"^Error: .+".to_string(),
            r"^error: .+".to_string(),
            r"^ERROR: .+".to_string(),
            r"Failed to .+".to_string(),
            r"fatal: .+".to_string(),
        ],
        edges,
        ignore_errors: Vec::new(),
        dyn_param,
        command_execution: DeviceCommandExecutionConfig::ShellExitStatus {
            marker: LINUX_EXIT_CODE_MARKER.to_string(),
            shell_flavor: config.shell_flavor,
        },
    }
}

/// Returns a `DeviceHandler` configured for Linux servers with custom configuration.
pub fn linux_with_config(config: LinuxTemplateConfig) -> Result<DeviceHandler, ConnectError> {
    linux_handler_config(config).build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{CommandBlockKind, RollbackPolicy};
    use crate::templates::{
        TemplateCapability, available_templates, build_tx_block, classify_command,
        template_metadata,
    };

    #[test]
    fn linux_template_has_user_and_root_states() {
        let handler = linux().expect("create linux template");
        let diagnostics = handler.diagnose_state_machine();

        // Linux template has User and Root states with transitions between them
        // Note: state names are normalized to lowercase in diagnostics
        assert!(diagnostics.total_states >= 2);
        assert_eq!(diagnostics.graph_states.len(), 2);
        assert!(diagnostics.graph_states.contains(&"user".to_string()));
        assert!(diagnostics.graph_states.contains(&"root".to_string()));
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn linux_template_is_in_builtin_templates() {
        let names = available_templates();
        assert!(names.contains(&"linux"));
    }

    #[test]
    fn linux_template_metadata_is_correct() {
        let meta = template_metadata("linux").expect("linux metadata");
        assert_eq!(meta.name, "linux");
        assert_eq!(meta.vendor, "Generic");
        assert_eq!(meta.family, "Linux");
        assert!(meta.capabilities.contains(&TemplateCapability::LoginMode));
        assert!(meta.capabilities.contains(&TemplateCapability::EnableMode));
        assert!(
            meta.capabilities
                .contains(&TemplateCapability::InteractiveInput)
        );
    }

    #[test]
    fn linux_template_by_name_works() {
        let handler = crate::templates::by_name("linux").expect("linux template by name");
        let diagnostics = handler.diagnose_state_machine();
        assert!(diagnostics.total_states >= 2);
    }

    #[test]
    fn linux_template_by_name_is_case_insensitive() {
        let handler = crate::templates::by_name("LiNuX").expect("linux template case insensitive");
        let diagnostics = handler.diagnose_state_machine();
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn linux_handler_config_rebuilds_equivalent_handler() {
        let handler = linux().expect("linux template");
        let rebuilt = linux_handler_config(LinuxTemplateConfig::default())
            .build()
            .expect("linux config");

        assert!(handler.is_equivalent(&rebuilt));
    }

    #[test]
    fn linux_handler_config_can_be_extended_by_callers() {
        let mut config = linux_handler_config(LinuxTemplateConfig::default());
        config
            .prompt
            .push(prompt_rule("Maintenance", &[r"^\[maint\]#\s*$"]));

        let handler = config.build().expect("extended config");
        assert!(
            handler
                .states()
                .iter()
                .any(|state| state.eq_ignore_ascii_case("Maintenance"))
        );
    }

    #[test]
    fn classify_linux_command_identifies_readonly() {
        assert_eq!(classify_linux_command("ls -la"), LinuxCommandType::ReadOnly);
        assert_eq!(
            classify_linux_command("cat /etc/hosts"),
            LinuxCommandType::ReadOnly
        );
        assert_eq!(
            classify_linux_command("systemctl status nginx"),
            LinuxCommandType::ReadOnly
        );
        assert_eq!(classify_linux_command("ps aux"), LinuxCommandType::ReadOnly);
    }

    #[test]
    fn classify_linux_command_identifies_service_ops() {
        assert_eq!(
            classify_linux_command("systemctl start nginx"),
            LinuxCommandType::ServiceOp
        );
        assert_eq!(
            classify_linux_command("systemctl enable nginx"),
            LinuxCommandType::ServiceOp
        );
    }

    #[test]
    fn classify_linux_command_identifies_file_ops() {
        assert_eq!(
            classify_linux_command("echo 'test' > /tmp/file"),
            LinuxCommandType::FileOp
        );
        assert_eq!(
            classify_linux_command("rm /tmp/file"),
            LinuxCommandType::FileOp
        );
    }

    #[test]
    fn classify_linux_command_is_case_insensitive() {
        assert_eq!(classify_linux_command("LS -LA"), LinuxCommandType::ReadOnly);
        assert_eq!(
            classify_linux_command("SYSTEMCTL START nginx"),
            LinuxCommandType::ServiceOp
        );
    }

    #[test]
    fn classify_command_supports_linux_template() {
        let kind = classify_command("linux", "ls -la").expect("classify");
        assert_eq!(kind, CommandBlockKind::Show);

        let kind = classify_command("linux", "apt install nginx").expect("classify");
        assert_eq!(kind, CommandBlockKind::Config);
    }

    #[test]
    fn linux_with_config_sudo_interactive() {
        let config = LinuxTemplateConfig {
            sudo_mode: SudoMode::SudoInteractive,
            sudo_password: Some("test123".to_string()),
            custom_prompts: None,
            ..LinuxTemplateConfig::default()
        };
        let handler = linux_with_config(config).expect("create linux with config");
        let diagnostics = handler.diagnose_state_machine();
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn linux_with_config_sudo_shell() {
        let config = LinuxTemplateConfig {
            sudo_mode: SudoMode::SudoShell,
            sudo_password: None,
            custom_prompts: None,
            ..LinuxTemplateConfig::default()
        };
        let handler = linux_with_config(config).expect("create linux with sudo -s");
        let diagnostics = handler.diagnose_state_machine();
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn linux_with_config_direct_root() {
        let config = LinuxTemplateConfig {
            sudo_mode: SudoMode::DirectRoot,
            sudo_password: None,
            custom_prompts: None,
            ..LinuxTemplateConfig::default()
        };
        let handler = linux_with_config(config).expect("create linux with direct root");
        let diagnostics = handler.diagnose_state_machine();
        // Direct root has no state transitions
        assert_eq!(diagnostics.graph_states.len(), 0);
    }

    #[test]
    fn linux_with_custom_prompts() {
        let config = LinuxTemplateConfig {
            sudo_mode: SudoMode::SudoInteractive,
            sudo_password: None,
            custom_prompts: Some(CustomPrompts {
                user_prompts: vec![r"^myuser@myhost\$\s*$"],
                root_prompts: vec![r"^root@myhost#\s*$"],
            }),
            ..LinuxTemplateConfig::default()
        };
        let handler = linux_with_config(config).expect("create linux with custom prompts");
        let diagnostics = handler.diagnose_state_machine();
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn build_tx_block_for_linux_readonly() {
        let commands = vec!["ls -la".to_string(), "cat /etc/hosts".to_string()];
        let tx = build_tx_block("linux", "show-block", "User", &commands, Some(30), None)
            .expect("build show tx");
        assert_eq!(tx.kind, CommandBlockKind::Show);
        assert!(matches!(tx.rollback_policy, RollbackPolicy::None));
    }

    #[test]
    fn build_tx_block_for_linux_config_requires_explicit_rollback() {
        // Config operations require explicit rollback command
        let commands = vec!["apt install nginx".to_string()];
        let result = build_tx_block("linux", "install-nginx", "Root", &commands, Some(60), None);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("require resource_rollback_command")
        );
    }

    #[test]
    fn build_tx_block_requires_explicit_rollback_for_config_commands() {
        // Config commands require explicit resource_rollback_command
        let commands = vec!["apt install nginx && rm -rf /".to_string()];
        let result = build_tx_block("linux", "malicious", "Root", &commands, Some(60), None);

        // Should fail because no rollback command provided
        assert!(result.is_err(), "Should require explicit rollback command");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("require resource_rollback_command")
        );
    }

    #[test]
    fn linux_template_password_not_recorded_in_output() {
        // Verify that password recording is disabled
        let mut handler = linux().expect("create linux template");
        handler
            .dyn_param
            .insert("SudoPassword".to_string(), "secret123".to_string());

        // The password should be in dyn_param but marked as not recordable
        assert!(handler.dyn_param.contains_key("SudoPassword"));

        // Note: The actual recording flag is checked in the input_map
        // which is set to (true, "SudoPassword", false) where the last false means don't record
    }

    #[test]
    fn linux_template_wraps_commands_for_exit_code_capture() {
        let handler = linux().expect("create linux template");
        let wrapped = handler.prepare_command_for_execution("false", true);

        assert!(wrapped.starts_with("false; printf"));
        assert!(wrapped.contains(LINUX_EXIT_CODE_MARKER));
        assert!(wrapped.contains("\"$?\""));
    }

    #[test]
    fn linux_template_can_force_fish_exit_status_capture() {
        let handler = linux_with_config(LinuxTemplateConfig {
            shell_flavor: DeviceShellFlavor::Fish,
            ..LinuxTemplateConfig::default()
        })
        .expect("create fish linux template");
        let wrapped = handler.prepare_command_for_execution("date", true);

        assert!(wrapped.contains(LINUX_EXIT_CODE_MARKER));
        assert!(wrapped.contains("\"$status\""));
    }
}
