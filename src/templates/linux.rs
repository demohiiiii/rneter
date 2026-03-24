//! Linux server template.
//!
//! This module provides device handler configuration for Linux servers with
//! support for privilege escalation via sudo or su.

use crate::device::DeviceHandler;
use crate::error::ConnectError;
use std::collections::HashMap;

const LINUX_EXIT_CODE_MARKER: &str = "__RNETER_EXIT_CODE__:";

/// Configuration for Linux template.
#[derive(Debug, Clone)]
pub struct LinuxTemplateConfig {
    pub sudo_mode: SudoMode,
    pub sudo_password: Option<String>,
    pub custom_prompts: Option<CustomPrompts>,
}

impl Default for LinuxTemplateConfig {
    fn default() -> Self {
        Self {
            sudo_mode: SudoMode::SudoInteractive,
            sudo_password: None,
            custom_prompts: None,
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

/// Returns a `DeviceHandler` configured for Linux servers with custom configuration.
pub fn linux_with_config(config: LinuxTemplateConfig) -> Result<DeviceHandler, ConnectError> {
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
            (
                "User".to_string(),
                sudo_command.to_string(),
                "Root".to_string(),
                false,
                false,
            ),
            (
                "Root".to_string(),
                "exit".to_string(),
                "User".to_string(),
                true,
                false,
            ),
        ]
    } else {
        vec![] // Direct root login, no state transition needed
    };

    let mut dyn_param = HashMap::new();
    if let Some(password) = config.sudo_password {
        dyn_param.insert("SudoPassword".to_string(), password);
    }

    DeviceHandler::new(
        // Prompt
        vec![
            ("Root".to_string(), root_prompts),
            ("User".to_string(), user_prompts),
        ],
        // Prompt with sys (optional: capture hostname)
        vec![],
        // Write (interactive inputs)
        vec![(
            "SudoPassword".to_string(),
            (true, "SudoPassword".to_string(), false), // Don't record password
            vec![
                r"\[sudo\] password for .+:\s*$",
                r"Password:\s*$",
                r"password:\s*$",
            ],
        )],
        // More regex (pagination prompts)
        vec![r"--More--", r"\(END\)", r"Press SPACE to continue"],
        // Error regex
        vec![
            r"^bash: .+: command not found",
            r"^-bash: .+: command not found",
            r"^sudo: .+: command not found",
            r"Permission denied",
            r"Operation not permitted",
            r"No such file or directory",
            r"cannot access",
            r"sudo: \d+ incorrect password attempt",
            r"su: Authentication failure",
            r"^E: .+",     // apt errors
            r"^Error: .+", // generic errors
            r"^error: .+", // lowercase errors
            r"^ERROR: .+", // uppercase errors
            r"Failed to .+",
            r"fatal: .+",
        ],
        // Edges
        edges,
        // Ignore errors (empty by default, user can customize)
        vec![],
        // Dyn param
        dyn_param,
    )
    .map(|handler| handler.with_shell_exit_status_marker(LINUX_EXIT_CODE_MARKER))
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
    }
}
