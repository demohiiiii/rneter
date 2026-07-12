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

/// Returns a `DeviceHandler` configured for Linux servers with default settings.
pub fn linux() -> Result<DeviceHandler, ConnectError> {
    linux_handler_config().build()
}

/// Exports the underlying handler configuration for the Linux template.
pub fn linux_handler_config() -> DeviceHandlerConfig {
    let user_prompts = [
        r"^[^\s]+\$\s*$",          // user$
        r"^[^\s]+@[^\s]+\$\s*$",   // user@host$
        r"^[^\s@]+@.+\$\s*$",      // user@host path$
        r"^[^\s@]+@.+>\s*$",       // fish: user@host path>
        r"^\[[^\]]+\]\$\s*$",      // [user@host]$
        r"^\[[^\]]+\]\s+.+\$\s*$", // [host] path$
        r"^\[[^\]]+\]\s+.+>\s*$",  // fish: [host] path>
        r"^\$\s*$",                // $
    ];
    let root_prompts = [
        r"^[^\s]+#\s*$",          // root#
        r"^root@[^\s]+#\s*$",     // root@host#
        r"^[^\s@]+@.+#\s*$",      // root@host path#
        r"^\[root@[^\]]+\]#\s*$", // [root@host]#
        r"^\[[^\]]+\]\s+.+#\s*$", // fish: [host] path#
        r"^\[[^\]]+\]#\s*$",      // [host]#
        r"^#\s*$",                // #
    ];

    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("User", &user_prompts),
            prompt_rule("Root", &root_prompts),
        ],
        prompt_with_sys: Vec::new(),
        prompt_prefix: Vec::new(),
        write: vec![input_rule(
            "EnablePassword",
            true,
            "EnablePassword",
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
        edges: vec![
            transition_rule("User", "sudo -i", "Root", false, false),
            transition_rule("Root", "exit", "User", true, false),
        ],
        ignore_errors: Vec::new(),
        dyn_param: HashMap::new(),
        hooks: Default::default(),
        command_execution: DeviceCommandExecutionConfig::ShellExitStatus {
            marker: LINUX_EXIT_CODE_MARKER.to_string(),
            shell_flavor: DeviceShellFlavor::Posix,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{TemplateCapability, available_templates, template_metadata};

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
        let rebuilt = linux_handler_config().build().expect("linux config");

        assert!(handler.is_equivalent(&rebuilt));
    }

    #[test]
    fn linux_template_prompts_follow_default_privilege_order() {
        let config = linux_handler_config();
        let prompt_states = config
            .prompt
            .iter()
            .map(|rule| rule.state.as_str())
            .collect::<Vec<_>>();

        assert_eq!(prompt_states, vec!["User", "Root"]);
    }

    #[test]
    fn linux_handler_config_can_be_extended_by_callers() {
        let mut config = linux_handler_config();
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
    fn linux_template_uses_default_sudo_edge() {
        let config = linux_handler_config();

        assert!(
            config
                .edges
                .contains(&transition_rule("User", "sudo -i", "Root", false, false))
        );
        assert!(
            config
                .edges
                .contains(&transition_rule("Root", "exit", "User", true, false))
        );
    }

    #[test]
    fn linux_sudo_prompt_uses_enable_password_dynamic_param() {
        let config = linux_handler_config();
        let rule = config.write.first().expect("sudo password rule");

        assert_eq!(rule.state, "EnablePassword");
        assert_eq!(rule.value, "EnablePassword");
        assert!(rule.dynamic);
        assert!(!rule.record_input);
    }

    #[test]
    fn linux_privilege_edge_can_be_replaced_by_callers() {
        let mut config = linux_handler_config();
        config.edges = vec![
            transition_rule("User", "sudo -s", "Root", false, false),
            transition_rule("Root", "exit", "User", true, false),
        ];

        let handler = config.build().expect("create linux with custom edges");
        let diagnostics = handler.diagnose_state_machine();
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn linux_prompt_rules_can_be_replaced_by_callers() {
        let mut config = linux_handler_config();
        config.prompt = vec![
            prompt_rule("User", &[r"^myuser@myhost\$\s*$"]),
            prompt_rule("Root", &[r"^root@myhost#\s*$"]),
        ];
        let handler = config.build().expect("create linux with custom prompts");
        let diagnostics = handler.diagnose_state_machine();
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn linux_template_password_not_recorded_in_output() {
        let config = linux_handler_config();
        let rule = config.write.first().expect("sudo password rule");

        assert!(!rule.record_input);
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
        let mut config = linux_handler_config();
        config.command_execution = DeviceCommandExecutionConfig::ShellExitStatus {
            marker: LINUX_EXIT_CODE_MARKER.to_string(),
            shell_flavor: DeviceShellFlavor::Fish,
        };
        let handler = config.build().expect("create fish linux template");
        let wrapped = handler.prepare_command_for_execution("date", true);

        assert!(wrapped.contains(LINUX_EXIT_CODE_MARKER));
        assert!(wrapped.contains("\"$status\""));
    }
}
