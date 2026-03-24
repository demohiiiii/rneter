//! Array Networks APV device template.

use crate::device::{
    DeviceHandler, DeviceHandlerConfig, input_rule, prompt_rule, prompt_with_sys_rule,
    transition_rule,
};
use crate::error::ConnectError;
use std::collections::HashMap;

/// Exports the underlying handler configuration for Array Networks devices.
pub fn array_config() -> DeviceHandlerConfig {
    DeviceHandlerConfig {
        prompt: vec![
            prompt_rule("Login", &[r"^[^\s<]+>\s*$"]),
            prompt_rule("Enable", &[r"^[^\s#]+#\s*$"]),
            prompt_rule("Config", &[r"^\S+\(\S+\)#\s*$"]),
        ],
        prompt_with_sys: vec![
            prompt_with_sys_rule("VSiteConfig", "VS", r"^(?<VS>\S+)\(\S+\)\$\s*$"),
            prompt_with_sys_rule("VSiteEnable", "VS", r"^(?<VS>\S+)\$\s*$"),
        ],
        write: vec![input_rule(
            "EnablePassword",
            true,
            "EnablePassword",
            true,
            &[r"^\x00*\rEnable password:"],
        )],
        more_regex: vec![r"\s*--More--\s*".to_string()],
        error_regex: vec![
            r"Virtual site .+ is not configured".to_string(),
            r"Access denied!".to_string(),
            r"Cannot find the group name '.+'\.".to_string(),
            r#"No such group map configured: ".+" to ".+"\."#.to_string(),
            r#"Internal group ".+" not found, please configure the group at localdb\."#.to_string(),
            r#"Already has a group map for external group ".+"\."#.to_string(),
            r#"role ".+" doesn't exist"#.to_string(),
            r#"qualification ".+" doesn't exist"#.to_string(),
            r#"the condition "GROUPNAME IS '.+'" doesn't exist in qualification ".+", role ".+""#.to_string(),
            r#"The resource ".+" has not been assigned to this role"#.to_string(),
            r"Netpool .+ does not exist".to_string(),
            r"Resource group .+ does not exist".to_string(),
            r#"The resource ".+" has not been assigned to this role"#.to_string(),
            r"Cannot find the resource group '.+'\.".to_string(),
            r"This resource group name has been used, please give another one\.".to_string(),
            r"This resource .+ doesn't exist or hasn't assigned to target .+".to_string(),
            r"Parse network resource failed: Invalid port format\.".to_string(),
            r"Parse network resource failed: Invalid ACL format\.".to_string(),
            r"Parse network resource failed: ICMP protocol resources MUST NOT with port information\.".to_string(),
            r"Cannot find the resource group '.+'\.".to_string(),
            r#"The resource ".+" does not exsit under resource group ".+""#.to_string(),
            r"\^$".to_string(),
        ],
        edges: vec![
            transition_rule("Login", "enable", "Enable", false, false),
            transition_rule("Enable", "configure terminal", "Config", false, false),
            transition_rule("Config", "exit", "Enable", true, false),
            transition_rule("Enable", "exit", "Login", true, false),
            transition_rule("Enable", "switch {}", "VSiteEnable", false, true),
            transition_rule("VSiteEnable", "configure terminal", "VSiteConfig", false, false),
            transition_rule("VSiteConfig", "exit", "VSiteEnable", true, false),
            transition_rule("VSiteEnable", "exit", "Enable", true, false),
        ],
        dyn_param: HashMap::new(),
        ..Default::default()
    }
}

/// Returns a `DeviceHandler` configured for Array Networks devices.
pub fn array() -> Result<DeviceHandler, ConnectError> {
    array_config().build()
}
