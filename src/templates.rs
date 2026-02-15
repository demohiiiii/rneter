//! Predefined device templates.
//!
//! This module contains factory functions to create `DeviceHandler` instances
//! for common network device types, pre-configured with their prompts,
//! error messages, and state transitions.

use crate::device::{DeviceHandler, StateMachineDiagnostics};
use crate::error::ConnectError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Built-in template names supported by this crate.
pub const BUILTIN_TEMPLATES: &[&str] = &["cisco", "huawei", "h3c", "hillstone", "juniper", "array"];

/// Capability tags used to describe template compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemplateCapability {
    LoginMode,
    EnableMode,
    ConfigMode,
    SysContext,
    InteractiveInput,
}

/// Metadata for a built-in device template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TemplateMetadata {
    pub name: String,
    pub vendor: String,
    pub family: String,
    pub template_version: String,
    pub capabilities: Vec<TemplateCapability>,
}

fn metadata_for(name: &str) -> Option<TemplateMetadata> {
    let meta = match name {
        "cisco" => TemplateMetadata {
            name: "cisco".to_string(),
            vendor: "Cisco".to_string(),
            family: "IOS/IOS-XE".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::LoginMode,
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "huawei" => TemplateMetadata {
            name: "huawei".to_string(),
            vendor: "Huawei".to_string(),
            family: "VRP".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "h3c" => TemplateMetadata {
            name: "h3c".to_string(),
            vendor: "H3C".to_string(),
            family: "Comware".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
            ],
        },
        "hillstone" => TemplateMetadata {
            name: "hillstone".to_string(),
            vendor: "Hillstone".to_string(),
            family: "SG".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "juniper" => TemplateMetadata {
            name: "juniper".to_string(),
            vendor: "Juniper".to_string(),
            family: "JunOS".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "array" => TemplateMetadata {
            name: "array".to_string(),
            vendor: "Array Networks".to_string(),
            family: "APV".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::LoginMode,
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::SysContext,
                TemplateCapability::InteractiveInput,
            ],
        },
        _ => return None,
    };
    Some(meta)
}

/// Returns names of all built-in templates.
pub fn available_templates() -> &'static [&'static str] {
    BUILTIN_TEMPLATES
}

/// Returns metadata for all built-in templates.
pub fn template_catalog() -> Vec<TemplateMetadata> {
    BUILTIN_TEMPLATES
        .iter()
        .filter_map(|name| metadata_for(name))
        .collect()
}

/// Returns metadata for one template by name (case-insensitive).
pub fn template_metadata(name: &str) -> Result<TemplateMetadata, ConnectError> {
    let key = name.to_ascii_lowercase();
    metadata_for(&key).ok_or_else(|| ConnectError::TemplateNotFound(name.to_string()))
}

/// Creates a built-in template by name (case-insensitive).
pub fn by_name(name: &str) -> Result<DeviceHandler, ConnectError> {
    match name.to_ascii_lowercase().as_str() {
        "cisco" => cisco(),
        "huawei" => huawei(),
        "h3c" => h3c(),
        "hillstone" => hillstone(),
        "juniper" => juniper(),
        "array" => array(),
        _ => Err(ConnectError::TemplateNotFound(name.to_string())),
    }
}

/// Builds a template by name and returns its state-machine diagnostics.
pub fn diagnose_template(name: &str) -> Result<StateMachineDiagnostics, ConnectError> {
    let handler = by_name(name)?;
    Ok(handler.diagnose_state_machine())
}

/// Builds a template by name and exports diagnostics as pretty JSON.
pub fn diagnose_template_json(name: &str) -> Result<String, ConnectError> {
    let report = diagnose_template(name)?;
    serde_json::to_string_pretty(&report)
        .map_err(|e| ConnectError::InternalServerError(format!("encode diagnostics json: {e}")))
}

/// Exports diagnostics for all built-in templates as pretty JSON.
pub fn diagnose_all_templates_json() -> Result<String, ConnectError> {
    let mut reports = std::collections::BTreeMap::new();
    for name in BUILTIN_TEMPLATES {
        reports.insert((*name).to_string(), diagnose_template(name)?);
    }
    serde_json::to_string_pretty(&reports)
        .map_err(|e| ConnectError::InternalServerError(format!("encode diagnostics json: {e}")))
}

/// Returns a `DeviceHandler` configured for Cisco IOS/IOS-XE devices.
pub fn cisco() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\S+\(\S+\)#\s*$"]),
            ("Enable".to_string(), vec![r"^[^\s#]+#\s*$"]),
            ("Login".to_string(), vec![r"^[^\s<]+>\s*$"]),
        ],
        // Prompt with sys (empty for cisco in db)
        vec![],
        // Write (interactive inputs)
        vec![(
            "EnablePassword".to_string(),
            (true, "EnablePassword".to_string(), true),
            vec![r"^\x00*\r(Enable )?Password:"],
        )],
        // More regex
        vec![r"\s*<--- More --->\s*"],
        // Error regex
        vec![
            r"% Invalid command at '\^' marker\.",
            r"% Invalid parameter detected at '\^' marker\.",
            r"invalid vlan \(reserved value\) at '\^' marker\.",
            r"ERROR: VLAN \d+ is not a primary vlan",
            r"\^$",
            r"^%.+",
            r"^Command authorization failed.*",
            r"^Command rejected:.*",
            r"ERROR:.+",
            r"Invalid password",
            r"Access denied.",
            r"End address less than start address",
        ],
        // Edges
        vec![
            (
                "Login".to_string(),
                "enable".to_string(),
                "Enable".to_string(),
                false,
                false,
            ),
            (
                "Enable".to_string(),
                "configure terminal".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "exit".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
            (
                "Enable".to_string(),
                "exit".to_string(),
                "Login".to_string(),
                true,
                false,
            ),
        ],
        // Ignore errors
        vec![r"ERROR: object \(.+\) does not exist."],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Huawei VRP devices.
pub fn huawei() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^(HRP_M|HRP_S){0,1}\[.+]+\s*$"]),
            ("Enable".to_string(), vec![r"^(RBM_P|RBM_S)?<.+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "Save".to_string(),
            (false, "y".to_string(), true),
            vec![
                r"Are you sure to continue\?\[Y\/N\]: ",
                r"startup saved-configuration file on peer device\?\[Y\/N\]: ",
                r"Warning: The current configuration will be written to the device. Continue\? \[Y\/N\]: ",
            ],
        )],
        // More regex
        vec![r"\s*---- More ----\s*"],
        // Error regex
        vec![r"Error: .+$", r"\^$"],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "system-view".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "exit".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
        ],
        // Ignore errors
        vec![
            r"Error: Address item conflicts!",
            r"Error: The address item does not exist!",
            r"Error: The delete configuration does not exist.",
            r"Error: The address or address set is not created!",
            r"Error: Cannot add! Service item conflicts or illegal reference!",
            r"Error: The service item does not exist!",
            r"Error: Service item conflicts!",
            r"Error: The service item does not exist!",
            r"Error: The service set is not created(.+)!",
            r"Error: No such a time-range.",
            r"Error: The specified address-group does not exist.",
            r"Error: The specified rule does not exist yet.",
            r"This condition has already been configured",
            r"[a-zA-Z]* (item conflicts|Service item exists\.)",
            r"Error: Worng parameter found at.*",
        ],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for H3C devices.
pub fn h3c() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^(RBM_P|RBM_S)?\[.+\]\s*$"]),
            ("Enable".to_string(), vec![r"^(RBM_P|RBM_S)?<.+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"\s*---- More ----\s*"],
        // Error regex
        vec![
            r".+\^.+",
            r".+%.+",
            r".+doesn't exist.+",
            r".+does not exist.+",
            r"Object group with given name exists with different type.",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "system-view".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "exit".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
        ],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Hillstone devices.
pub fn hillstone() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Enable".to_string(), vec![r"^.+#\s\r{0,1}$"]),
            (
                "Config".to_string(),
                vec![r"^.+\(config.*\)\s*#\s\r{0,1}$"],
                // Note: adjusted regex slightly from raw string to be valid rust string if needed,
                // raw string `^\\x00*\\r{0,1}.+\\(config.*\\)#\\s\\r{0,1}$` -> `r"^\x00*\r{0,1}.+\(config.*\)\#\s\r{0,1}$"`
                // Actually the db has `(config.*)` so we need `\(config.*\)` in regex.
            ),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "Save".to_string(),
            (false, "y".to_string(), true),
            vec![
                r"Save configuration, are you sure\? \[y\]\/n: ",
                r"Save configuration for all VSYS, are you sure\? \[y\]\/n: ",
                r"Backup start configuration file, are you sure\? y\/\[n\]: ",
                r"Backup all start configuration files, are you sure\? y\/\[n\]: ",
                r"保存配置，请确认 \[y\]\/n: ",
                r"备份启动配置文件，请确认 y\/\[n\]: ",
                r"保存所有VSYS的配置，请确认 \[y\]\/n: ",
                r"备份所有启动配置文件，请确认 y\/\[n\]: ",
            ],
        )],
        // More regex
        vec![r"\s*--More--\s*"],
        // Error regex
        vec![
            r".+\^.+",
            r".+%.+",
            r".+doesn't exist.+",
            r".+does not exist.+",
            r"Object group with given name exists with different type.",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "config".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "exit".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
        ],
        // Ignore errors
        vec![
            r"Error: Schedule entity (.+) is not found",
            r"错误：没有找到时间表(.+)",
            r"Error: Failed to find this service",
            r"错误: 无法找到服务",
            r"Error: Rule (\d+) is not found$",
            r"错误：规则(\d+)不存在",
            r"Error: This service already exists",
            r"错误：该服务已经添加",
            r"Error: Rule is already configured with schedule (.+)",
            r#"错误：此规则已经配置了时间表"(.+)""#,
            r"Error: Rule is not configured with schedule (.+)",
            r#"错误：此规则没有配置了时间表"(.+)""#,
            r"Error: This entity is already added",
            r"错误：该项已经添加",
            r"Error: This entity already exists",
            r"错误: 该成员已经存在",
            r"Error: Cannot find this service entity",
            r"错误：查找该服务条目失败!",
            r"Error: Address entry (.+) has no member (.+)",
            r"错误：地址条目(.+)没有成员(.+)",
            r"Error: Address (.+) is not found",
            r"错误：地址簿(.+)没有找到",
            r"Error: Deleting a service not configured",
            r"错误：尝试删除一个没有配置的服务",
        ],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Juniper JunOS devices.
pub fn juniper() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\S+@\S+#\s*$"]),
            ("Enable".to_string(), vec![r"^\S+@\S+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "Save".to_string(),
            (false, "yes".to_string(), true),
            vec![r"Exit with uncommitted changes\? \[yes,no\] \(yes\) "],
        )],
        // More regex
        vec![r"---\(more.*\)---"],
        // Error regex
        vec![
            r".*unknown command.*",
            r"syntax error.*",
            r"error:.+",
            r".+not found.*",
            r"invalid value .+",
            r"invalid ip address .+",
            r".*invalid prefix length .+",
            r"prefix length \S+ is larger than \d+ .+",
            r"number: \S+: Value must be a number from 0 to 255 at \S+",
            r"\s+\^$",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "system-view".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "exit".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
        ],
        // Ignore errors
        vec![
            r"warning: statement not found",
            r"warning: element \S+ not found",
        ],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Array Networks devices.
pub fn array() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Login".to_string(), vec![r"^[^\s<]+>\s*$"]),
            ("Enable".to_string(), vec![r"^[^\s#]+#\s*$"]),
            ("Config".to_string(), vec![r"^\S+\(\S+\)#\s*$"]),
        ],
        // Prompt with sys
        vec![
            (
                "VSiteConfig".to_string(),
                "VS",
                r"^(?<VS>\S+)\(\S+\)\$\s*$".to_string(),
            ),
            (
                "VSiteEnable".to_string(),
                "VS",
                r"^(?<VS>\S+)\$\s*$".to_string(),
            ),
        ],
        // Write
        vec![(
            "EnablePassword".to_string(),
            (true, "EnablePassword".to_string(), true),
            vec![r"^\x00*\rEnable password:"],
        )],
        // More regex
        vec![r"\s*--More--\s*"],
        // Error regex
        vec![
            r"Virtual site .+ is not configured",
            r"Access denied!",
            r"Cannot find the group name '.+'\.",
            r#"No such group map configured: ".+" to ".+"\."#,
            r#"Internal group ".+" not found, please configure the group at localdb\."#,
            r#"Already has a group map for external group ".+"\."#,
            r#"role ".+" doesn't exist"#,
            r#"qualification ".+" doesn't exist"#,
            r#"the condition "GROUPNAME IS '.+'" doesn't exist in qualification ".+", role ".+""#,
            r#"The resource ".+" has not been assigned to this role"#,
            r"Netpool .+ does not exist",
            r"Resource group .+ does not exist",
            r#"The resource ".+" has not been assigned to this role"#,
            r"Cannot find the resource group '.+'\.",
            r"This resource group name has been used, please give another one\.",
            r"This resource .+ doesn't exist or hasn't assigned to target .+",
            r"Parse network resource failed: Invalid port format\.",
            r"Parse network resource failed: Invalid ACL format\.",
            r"Parse network resource failed: ICMP protocol resources MUST NOT with port information\.",
            r"Cannot find the resource group '.+'\.",
            r#"The resource ".+" does not exsit under resource group ".+""#,
            r"\^$",
        ],
        // Edges
        vec![
            (
                "Login".to_string(),
                "enable".to_string(),
                "Enable".to_string(),
                false,
                false,
            ),
            (
                "Enable".to_string(),
                "configure terminal".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "exit".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
            (
                "Enable".to_string(),
                "exit".to_string(),
                "Login".to_string(),
                true,
                false,
            ),
            (
                "Enable".to_string(),
                "switch {}".to_string(),
                "VSiteEnable".to_string(),
                false,
                true,
            ),
            (
                "VSiteEnable".to_string(),
                "configure terminal".to_string(),
                "VSiteConfig".to_string(),
                false,
                false,
            ),
            (
                "VSiteConfig".to_string(),
                "exit".to_string(),
                "VSiteEnable".to_string(),
                true,
                false,
            ),
            (
                "VSiteEnable".to_string(),
                "exit".to_string(),
                "Enable".to_string(),
                true,
                false,
            ),
        ],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_templates_contains_expected_names() {
        let names = available_templates();
        assert!(names.contains(&"cisco"));
        assert!(names.contains(&"juniper"));
        assert!(names.contains(&"array"));
    }

    #[test]
    fn by_name_is_case_insensitive() {
        let handler = by_name("CiScO").expect("cisco template should load");
        let diagnostics = handler.diagnose_state_machine();
        assert!(diagnostics.missing_edge_sources.is_empty());
        assert!(diagnostics.missing_edge_targets.is_empty());
    }

    #[test]
    fn by_name_returns_template_not_found_for_unknown_name() {
        let err = match by_name("unknown-vendor") {
            Ok(_) => panic!("unknown template should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, ConnectError::TemplateNotFound(_)));
    }

    #[test]
    fn diagnose_template_returns_report() {
        let report = diagnose_template("huawei").expect("diagnostics should succeed");
        assert!(report.total_states > 0);
    }

    #[test]
    fn template_catalog_has_metadata_for_all_builtin_templates() {
        let catalog = template_catalog();
        assert_eq!(catalog.len(), BUILTIN_TEMPLATES.len());
        assert!(catalog.iter().any(|m| m.name == "cisco"));
        assert!(catalog.iter().any(|m| m.name == "array"));
    }

    #[test]
    fn template_metadata_is_case_insensitive() {
        let meta = template_metadata("JuNiPeR").expect("metadata should resolve");
        assert_eq!(meta.name, "juniper");
        assert_eq!(meta.vendor, "Juniper");
    }

    #[test]
    fn diagnose_template_json_returns_valid_json() {
        let json = diagnose_template_json("cisco").expect("json diagnostics");
        let report: StateMachineDiagnostics =
            serde_json::from_str(&json).expect("parse diagnostics json");
        assert!(report.total_states > 0);
    }

    #[test]
    fn diagnose_all_templates_json_includes_builtin_template_keys() {
        let json = diagnose_all_templates_json().expect("all diagnostics json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse json");
        for name in BUILTIN_TEMPLATES {
            assert!(value.get(*name).is_some(), "missing template key: {name}");
        }
    }
}
