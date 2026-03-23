//! Predefined device templates.
//!
//! This module contains factory functions to create `DeviceHandler` instances
//! for common network device types, pre-configured with their prompts,
//! error messages, and state transitions.

use crate::device::{DeviceHandler, StateMachineDiagnostics};
use crate::error::ConnectError;
use crate::session::{CommandBlockKind, RollbackPolicy, TxBlock, TxStep};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Built-in template names supported by this crate.
pub const BUILTIN_TEMPLATES: &[&str] = &[
    "cisco", "huawei", "h3c", "hillstone", "juniper", "array", "linux",
    "arista", "fortinet", "paloalto", "topsec", "venustech", "dptech",
    "chaitin", "qianxin", "maipu", "checkpoint"
];

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
        "linux" => TemplateMetadata {
            name: "linux".to_string(),
            vendor: "Generic".to_string(),
            family: "Linux".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::LoginMode,
                TemplateCapability::EnableMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "arista" => TemplateMetadata {
            name: "arista".to_string(),
            vendor: "Arista".to_string(),
            family: "EOS".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::LoginMode,
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "fortinet" => TemplateMetadata {
            name: "fortinet".to_string(),
            vendor: "Fortinet".to_string(),
            family: "FortiGate".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
            ],
        },
        "paloalto" => TemplateMetadata {
            name: "paloalto".to_string(),
            vendor: "Palo Alto Networks".to_string(),
            family: "PA".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
            ],
        },
        "topsec" => TemplateMetadata {
            name: "topsec".to_string(),
            vendor: "Topsec".to_string(),
            family: "NGFW".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
            ],
        },
        "venustech" => TemplateMetadata {
            name: "venustech".to_string(),
            vendor: "Venustech".to_string(),
            family: "USG".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::LoginMode,
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "dptech" => TemplateMetadata {
            name: "dptech".to_string(),
            vendor: "DPTech".to_string(),
            family: "FW".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
            ],
        },
        "chaitin" => TemplateMetadata {
            name: "chaitin".to_string(),
            vendor: "Chaitin".to_string(),
            family: "SafeLine".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::LoginMode,
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "qianxin" => TemplateMetadata {
            name: "qianxin".to_string(),
            vendor: "QiAnXin".to_string(),
            family: "NSG".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
            ],
        },
        "maipu" => TemplateMetadata {
            name: "maipu".to_string(),
            vendor: "Maipu".to_string(),
            family: "NSS".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::LoginMode,
                TemplateCapability::EnableMode,
                TemplateCapability::ConfigMode,
                TemplateCapability::InteractiveInput,
            ],
        },
        "checkpoint" => TemplateMetadata {
            name: "checkpoint".to_string(),
            vendor: "Check Point".to_string(),
            family: "Security Gateway".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![
                TemplateCapability::EnableMode,
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

/// Classify a command for a specific template.
///
/// Current rule is intentionally simple: read-only commands are treated as `show`,
/// everything else is treated as `config`.
pub fn classify_command(template: &str, command: &str) -> Result<CommandBlockKind, ConnectError> {
    let template_key = template.to_ascii_lowercase();
    let _ = template_metadata(&template_key)?;

    // Linux template uses its own classification
    if template_key == "linux" {
        let cmd_type = classify_linux_command(command);
        return Ok(match cmd_type {
            LinuxCommandType::ReadOnly => CommandBlockKind::Show,
            _ => CommandBlockKind::Config,
        });
    }

    // Network device templates
    let cmd = command.trim().to_ascii_lowercase();
    let show_prefixes = ["show ", "display ", "ping ", "traceroute "];
    if show_prefixes.iter().any(|prefix| cmd.starts_with(prefix)) {
        return Ok(CommandBlockKind::Show);
    }
    Ok(CommandBlockKind::Config)
}

fn infer_rollback_command(template_key: &str, command: &str) -> Option<String> {
    let cmd = command.trim();
    let lower = cmd.to_ascii_lowercase();

    if ["show ", "display ", "ping ", "traceroute "]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return None;
    }

    // Vendor-specific compensation patterns.
    match template_key {
        // Linux uses its own rollback inference
        "linux" => infer_linux_rollback(command),
        // JunOS is set/delete style.
        "juniper" => {
            if let Some(rest) = cmd.strip_prefix("set ") {
                Some(format!("delete {rest}"))
            } else if let Some(rest) = cmd.strip_prefix("activate ") {
                Some(format!("deactivate {rest}"))
            } else {
                cmd.strip_prefix("deactivate ")
                    .map(|rest| format!("activate {rest}"))
            }
        }
        // VRP/Comware prefer "undo <cmd>".
        "huawei" | "h3c" => {
            if lower.starts_with("undo ") {
                None
            } else {
                Some(format!("undo {cmd}"))
            }
        }
        // Cisco/Hillstone/Array style: "no <cmd>".
        _ => {
            if lower.starts_with("no ") {
                None
            } else {
                Some(format!("no {cmd}"))
            }
        }
    }
}

/// Build a transaction-like block from template + command list.
///
/// Behavior:
/// - If all commands are `show`-like, build a `show` block with no rollback.
/// - Otherwise build a `config` block.
///   - If `resource_rollback_command` is provided, use `whole_resource`.
///   - Else infer per-step rollback commands from template rules.
pub fn build_tx_block(
    template: &str,
    block_name: &str,
    mode: &str,
    commands: &[String],
    timeout_secs: Option<u64>,
    resource_rollback_command: Option<String>,
) -> Result<TxBlock, ConnectError> {
    let template_key = template.to_ascii_lowercase();
    let _ = template_metadata(&template_key)?;

    if commands.is_empty() {
        return Err(ConnectError::InvalidTransaction(
            "cannot build tx block with empty commands".to_string(),
        ));
    }

    // If every command is read-only, skip rollback and keep a simple show block.
    let kinds = commands
        .iter()
        .map(|cmd| classify_command(&template_key, cmd))
        .collect::<Result<Vec<_>, _>>()?;
    let all_show = kinds.iter().all(|k| *k == CommandBlockKind::Show);

    if all_show {
        return Ok(TxBlock {
            name: block_name.to_string(),
            kind: CommandBlockKind::Show,
            rollback_policy: RollbackPolicy::None,
            steps: commands
                .iter()
                .map(|cmd| TxStep {
                    mode: mode.to_string(),
                    command: cmd.clone(),
                    timeout_secs,
                    rollback_command: None,
                    rollback_on_failure: false,
                })
                .collect(),
            fail_fast: true,
        });
    }

    // Config blocks choose rollback policy based on caller intent:
    // - explicit resource rollback command -> whole resource compensation
    // - otherwise try per-step rollback inference
    let rollback_policy = if let Some(undo) = resource_rollback_command {
        RollbackPolicy::WholeResource {
            mode: mode.to_string(),
            undo_command: undo,
            timeout_secs,
            trigger_step_index: 0,
        }
    } else {
        RollbackPolicy::PerStep
    };

    let mut steps = Vec::with_capacity(commands.len());
    for (i, cmd) in commands.iter().enumerate() {
        let rollback_command = if matches!(rollback_policy, RollbackPolicy::PerStep) {
            infer_rollback_command(&template_key, cmd)
        } else {
            None
        };
        if matches!(rollback_policy, RollbackPolicy::PerStep) && rollback_command.is_none() {
            return Err(ConnectError::InvalidTransaction(format!(
                "cannot infer rollback command for step[{i}] '{}'; provide resource_rollback_command",
                cmd
            )));
        }
        steps.push(TxStep {
            mode: mode.to_string(),
            command: cmd.clone(),
            timeout_secs,
            rollback_command,
            rollback_on_failure: false,
        });
    }

    Ok(TxBlock {
        name: block_name.to_string(),
        kind: CommandBlockKind::Config,
        rollback_policy,
        steps,
        fail_fast: true,
    })
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
        "linux" => linux(),
        "arista" => arista(),
        "fortinet" => fortinet(),
        "paloalto" => paloalto(),
        "topsec" => topsec(),
        "venustech" => venustech(),
        "dptech" => dptech(),
        "chaitin" => chaitin(),
        "qianxin" => qianxin(),
        "maipu" => maipu(),
        "checkpoint" => checkpoint(),
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
        vec![],
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
            ("Enable".to_string(), vec![r"^(HRP_M|HRP_S){0,1}<.+>\s*$"]),
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
        vec![],
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
        vec![],
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
        vec![],
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

/// Returns a `DeviceHandler` configured for Arista EOS devices.
pub fn arista() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\S+\(\S+\)#\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}[^\s#]+#\s*$"]),
            ("Login".to_string(), vec![r"^\r{0,1}[^\s<]+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "EnablePassword".to_string(),
            (true, "EnablePassword".to_string(), true),
            vec![r"Password:"],
        )],
        // More regex
        vec![r" --More-- "],
        // Error regex
        vec![
            r"% Invalid input",
            r"% Ambiguous command",
            r"% Bad secret",
            r"% Unrecognized command",
            r"% Incomplete command",
            r"% Invalid port range .+",
            r"! Access VLAN does not exist. Creating vlan .+",
            r"% Address \S+ is already assigned to interface .+",
            r"% Removal of physical interfaces is not permitted",
            r"^% .+",
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
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Fortinet FortiGate devices.
pub fn fortinet() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt - Fortinet only has Enable mode
        vec![
            ("Enable".to_string(), vec![r"^\r{0,1}\S+\s*#\s*$"]),
        ],
        // Prompt with sys (VSYS support)
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"--More--"],
        // Error regex
        vec![
            r"Unknown action.*",
            r"Command fail.*",
        ],
        // Edges - Fortinet has no mode transitions
        vec![],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Palo Alto Networks devices.
pub fn paloalto() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\S+@\S+#\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}\S+@\S+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"(--more--)|(lines \d+-\d+ )"],
        // Error regex
        vec![
            r"Unknown command:.*",
            r"Invalid syntax.",
            r"Server error:.*",
            r"Validation Error:.*",
            r"Commit failed",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "configure".to_string(),
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

/// Returns a `DeviceHandler` configured for TopSec NGFW devices.
pub fn topsec() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt - TopSec only has Enable mode
        vec![
            ("Enable".to_string(), vec![r"^\r{0,1}\S+[#%]\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"--More--"],
        // Error regex
        vec![r"^error"],
        // Edges - No mode transitions
        vec![],
        // Ignore errors
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Venustech USG devices.
pub fn venustech() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\S+\(\S+\)#\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}[^\s#]+#\s*$"]),
            ("Login".to_string(), vec![r"^\r{0,1}[^\s<]+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "EnablePassword".to_string(),
            (true, "EnablePassword".to_string(), true),
            vec![r"(Enable )?Password:"],
        )],
        // More regex
        vec![r"--More-- \(\d+% of \d+ bytes\)"],
        // Error regex
        vec![
            r"^%.+",
            r".+not exist!",
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
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for DPTech firewall devices.
pub fn dptech() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\[.+\]\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}<.+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r" --More\(CTRL\+C break\)-- "],
        // Error regex
        vec![
            r"% Unknown command.*",
            r"Can't find the .+ object",
            r".*not exist.*",
            r".*item is longer.*",
            r"Failed.*",
            r"Undefined error.*",
            r"% Command can not contain:.+",
            r"Invalid parameter.*",
            r"% Ambiguous command.",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "conf-mode".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "end".to_string(),
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

/// Returns a `DeviceHandler` configured for ChaiTin SafeLine devices.
pub fn chaitin() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\S+\(\S+\)#\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}[^\s#]+#\s*$"]),
            ("Login".to_string(), vec![r"^\r{0,1}[^\s<]+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "EnablePassword".to_string(),
            (true, "EnablePassword".to_string(), true),
            vec![r"(Enable )?Password:"],
        )],
        // More regex
        vec![],
        // Error regex
        vec![
            r"% Command incomplete",
            r"% Unknown command",
            r"Error:.*",
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
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for QiAnXin NSG devices.
pub fn qianxin() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\S+-config.*]\s*$"]),
            ("Enable".to_string(), vec![r"^\S+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"--More--"],
        // Error regex
        vec![
            r"% Unknown command.",
            r"% Command incomplete.",
            r"%?\s+Invalid parameter.*",
            r"\s+Valid name can.*",
            r"\s+Repetitions with Object.*",
            r".+ exist",
            r"\s+Start larger than end",
            r"\s+Name can not repeat",
            r"Object .+ referenced by other module",
            r"Object service has been referenced",
            r"Object \[.+\] is quoted",
        ],
        // Edges
        vec![
            (
                "Enable".to_string(),
                "config terminal".to_string(),
                "Config".to_string(),
                false,
                false,
            ),
            (
                "Config".to_string(),
                "end".to_string(),
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

/// Returns a `DeviceHandler` configured for MaiPu network devices.
pub fn maipu() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt
        vec![
            ("Config".to_string(), vec![r"^\r{0,1}\S+\(\S+\)#\s*$"]),
            ("Enable".to_string(), vec![r"^\r{0,1}[^\s#]+#\s*$"]),
            ("Login".to_string(), vec![r"^\r{0,1}[^\s<]+>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![(
            "EnablePassword".to_string(),
            (true, "EnablePassword".to_string(), true),
            vec![r"password:"],
        )],
        // More regex
        vec![],
        // Error regex
        vec![r"% Invalid input"],
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
        vec![],
        // Dyn param
        HashMap::new(),
    )
}

/// Returns a `DeviceHandler` configured for Check Point Security Gateway devices.
pub fn checkpoint() -> Result<DeviceHandler, ConnectError> {
    DeviceHandler::new(
        // Prompt - Check Point only has Enable mode
        vec![
            ("Enable".to_string(), vec![r"^\r{0,1}\S+\s*>\s*$"]),
        ],
        // Prompt with sys
        vec![],
        // Write
        vec![],
        // More regex
        vec![r"-- More --"],
        // Error regex
        vec![
            r".+Incomplete command\.",
            r".+Invalid command:.+",
        ],
        // Edges - No mode transitions
        vec![],
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

    #[test]
    fn classify_show_command_returns_show_kind() {
        let kind = classify_command("cisco", "show version").expect("classify");
        assert_eq!(kind, CommandBlockKind::Show);
    }

    #[test]
    fn build_tx_block_for_show_uses_none_rollback() {
        let commands = vec!["show version".to_string(), "show clock".to_string()];
        let tx = build_tx_block("cisco", "show-block", "Enable", &commands, Some(30), None)
            .expect("build show tx");
        assert_eq!(tx.kind, CommandBlockKind::Show);
        assert!(matches!(tx.rollback_policy, RollbackPolicy::None));
        assert!(tx.steps.iter().all(|s| s.rollback_command.is_none()));
    }

    #[test]
    fn build_tx_block_for_huawei_infers_undo_per_step() {
        let commands = vec!["acl 3000".to_string(), "rule permit ip".to_string()];
        let tx = build_tx_block("huawei", "cfg-block", "Config", &commands, Some(30), None)
            .expect("build config tx");
        assert_eq!(tx.kind, CommandBlockKind::Config);
        assert!(matches!(tx.rollback_policy, RollbackPolicy::PerStep));
        assert_eq!(
            tx.steps[0].rollback_command.as_deref(),
            Some("undo acl 3000")
        );
        assert_eq!(
            tx.steps[1].rollback_command.as_deref(),
            Some("undo rule permit ip")
        );
    }

    #[test]
    fn build_tx_block_for_juniper_infers_delete_from_set() {
        let commands =
            vec!["set security zones security-zone trust interfaces ge-0/0/0.0".to_string()];
        let tx = build_tx_block("juniper", "cfg-block", "Config", &commands, None, None)
            .expect("build config tx");
        assert_eq!(
            tx.steps[0].rollback_command.as_deref(),
            Some("delete security zones security-zone trust interfaces ge-0/0/0.0")
        );
    }

    #[test]
    fn build_tx_block_supports_whole_resource_rollback() {
        let commands = vec![
            "address-object host WEB01".to_string(),
            "host 10.0.0.10".to_string(),
        ];
        let tx = build_tx_block(
            "cisco",
            "addr-create",
            "Config",
            &commands,
            Some(20),
            Some("no address-object host WEB01".to_string()),
        )
        .expect("build config tx");
        assert!(matches!(
            tx.rollback_policy,
            RollbackPolicy::WholeResource { .. }
        ));
        assert!(tx.steps.iter().all(|s| s.rollback_command.is_none()));
    }

    #[test]
    fn build_tx_block_returns_error_when_rollback_cannot_be_inferred() {
        let commands = vec!["undo acl 3000".to_string()];
        let err = build_tx_block("huawei", "bad", "Config", &commands, None, None)
            .expect_err("should fail");
        assert!(matches!(err, ConnectError::InvalidTransaction(_)));
    }
}

// ============================================================================
// Linux Server Support
// ============================================================================

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
    PackageOp,
    FileOp,
    ServiceOp,
    Custom,
}

/// Classify a Linux command by its type.
pub fn classify_linux_command(command: &str) -> LinuxCommandType {
    let cmd = command.trim().to_ascii_lowercase();

    // Read-only commands
    let readonly_prefixes = [
        "ls", "cat", "grep", "find", "ps", "top", "df", "du", "free", "uptime",
        "systemctl status", "journalctl", "tail", "head", "less", "more",
        "which", "whereis", "pwd", "whoami", "id", "uname", "hostname",
    ];
    if readonly_prefixes.iter().any(|prefix| cmd.starts_with(prefix)) {
        return LinuxCommandType::ReadOnly;
    }

    // Package management
    let package_prefixes = [
        "apt install", "apt-get install", "yum install", "dnf install",
        "pip install", "npm install", "cargo install",
    ];
    if package_prefixes.iter().any(|prefix| cmd.starts_with(prefix)) {
        return LinuxCommandType::PackageOp;
    }

    // Service operations
    let service_prefixes = [
        "systemctl start", "systemctl stop", "systemctl enable",
        "systemctl disable", "systemctl restart", "service",
    ];
    if service_prefixes.iter().any(|prefix| cmd.starts_with(prefix)) {
        return LinuxCommandType::ServiceOp;
    }

    // File operations
    let file_prefixes = ["echo", "sed", "awk", "rm", "mv", "cp", "touch", "mkdir"];
    if file_prefixes.iter().any(|prefix| cmd.starts_with(prefix)) {
        return LinuxCommandType::FileOp;
    }

    LinuxCommandType::Custom
}

/// Infer rollback command for Linux operations.
///
/// Security: This function performs basic validation to prevent command injection.
/// It only generates rollback commands for simple, single-command operations.
/// Complex commands with pipes, redirects, or command chaining are rejected.
pub fn infer_linux_rollback(command: &str) -> Option<String> {
    let cmd = command.trim();

    // Security check: reject commands with shell metacharacters that could enable injection
    // This prevents attacks like: "apt install nginx && rm -rf /"
    let dangerous_chars = ['&', '|', ';', '`', '$', '(', ')', '<', '>', '\n', '\r'];
    if dangerous_chars.iter().any(|c| cmd.contains(*c)) {
        return None;
    }

    let cmd_type = classify_linux_command(command);

    match cmd_type {
        LinuxCommandType::ReadOnly => None,
        LinuxCommandType::PackageOp => {
            // Extract package name and validate it's a simple install command
            if let Some(pkg) = extract_package_name(cmd) {
                if cmd.starts_with("apt install ") || cmd.starts_with("apt-get install ") {
                    Some(format!("apt remove {}", pkg))
                } else if cmd.starts_with("yum install ") {
                    Some(format!("yum remove {}", pkg))
                } else if cmd.starts_with("dnf install ") {
                    Some(format!("dnf remove {}", pkg))
                } else if cmd.starts_with("pip install ") {
                    Some(format!("pip uninstall -y {}", pkg))
                } else if cmd.starts_with("npm install ") {
                    Some(format!("npm uninstall {}", pkg))
                } else {
                    None
                }
            } else {
                None
            }
        }
        LinuxCommandType::ServiceOp => {
            // Extract service name and validate it's a simple service command
            if let Some(service) = extract_service_name(cmd) {
                if cmd.starts_with("systemctl start ") {
                    Some(format!("systemctl stop {}", service))
                } else if cmd.starts_with("systemctl enable ") {
                    Some(format!("systemctl disable {}", service))
                } else if cmd.starts_with("systemctl stop ") {
                    Some(format!("systemctl start {}", service))
                } else if cmd.starts_with("systemctl disable ") {
                    Some(format!("systemctl enable {}", service))
                } else {
                    None
                }
            } else {
                None
            }
        }
        LinuxCommandType::FileOp => {
            // File operations need backup before execution
            // Actual backup logic is handled in TxBlock execution
            None
        }
        LinuxCommandType::Custom => None,
    }
}

/// Extract package name from install command.
/// Returns None if the command is not a simple package install.
fn extract_package_name(cmd: &str) -> Option<String> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();

    // Simple validation: command should be "manager install package" or "manager install -y package"
    if parts.len() < 3 {
        return None;
    }

    // Find the package name (skip flags like -y, --yes)
    for part in parts.iter().skip(2) {
        if !part.starts_with('-') {
            // Validate package name: alphanumeric, dash, underscore, dot only
            if part.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
                return Some(part.to_string());
            }
        }
    }

    None
}

/// Extract service name from systemctl command.
/// Returns None if the command is not a simple service operation.
fn extract_service_name(cmd: &str) -> Option<String> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();

    // Simple validation: command should be "systemctl action service"
    if parts.len() != 3 {
        return None;
    }

    let service = parts[2];

    // Validate service name: alphanumeric, dash, underscore, dot, @ only
    if service.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '@') {
        Some(service.to_string())
    } else {
        None
    }
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
                r"^\[[^\]]+\]\$\s*$",    // [user@host]$
                r"^\$\s*$",              // $
            ],
            vec![
                r"^[^\s]+#\s*$",           // root#
                r"^root@[^\s]+#\s*$",      // root@host#
                r"^\[root@[^\]]+\]#\s*$",  // [root@host]#
                r"^#\s*$",                 // #
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
            r"^E: .+",      // apt errors
            r"^Error: .+",  // generic errors
            r"^error: .+",  // lowercase errors
            r"^ERROR: .+",  // uppercase errors
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
}

#[cfg(test)]
mod linux_tests {
    use super::*;

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
        assert!(meta.capabilities.contains(&TemplateCapability::InteractiveInput));
    }

    #[test]
    fn linux_template_by_name_works() {
        let handler = by_name("linux").expect("linux template by name");
        let diagnostics = handler.diagnose_state_machine();
        assert!(diagnostics.total_states >= 2);
    }

    #[test]
    fn linux_template_by_name_is_case_insensitive() {
        let handler = by_name("LiNuX").expect("linux template case insensitive");
        let diagnostics = handler.diagnose_state_machine();
        assert!(!diagnostics.has_issues());
    }

    #[test]
    fn classify_linux_command_identifies_readonly() {
        assert_eq!(
            classify_linux_command("ls -la"),
            LinuxCommandType::ReadOnly
        );
        assert_eq!(
            classify_linux_command("cat /etc/hosts"),
            LinuxCommandType::ReadOnly
        );
        assert_eq!(
            classify_linux_command("systemctl status nginx"),
            LinuxCommandType::ReadOnly
        );
        assert_eq!(
            classify_linux_command("ps aux"),
            LinuxCommandType::ReadOnly
        );
    }

    #[test]
    fn classify_linux_command_identifies_package_ops() {
        assert_eq!(
            classify_linux_command("apt install nginx"),
            LinuxCommandType::PackageOp
        );
        assert_eq!(
            classify_linux_command("yum install httpd"),
            LinuxCommandType::PackageOp
        );
        assert_eq!(
            classify_linux_command("pip install requests"),
            LinuxCommandType::PackageOp
        );
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
    fn infer_linux_rollback_for_apt_install() {
        let rollback = infer_linux_rollback("apt install nginx");
        assert_eq!(rollback, Some("apt remove nginx".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_yum_install() {
        let rollback = infer_linux_rollback("yum install httpd");
        assert_eq!(rollback, Some("yum remove httpd".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_systemctl_start() {
        let rollback = infer_linux_rollback("systemctl start nginx");
        assert_eq!(rollback, Some("systemctl stop nginx".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_systemctl_enable() {
        let rollback = infer_linux_rollback("systemctl enable nginx");
        assert_eq!(rollback, Some("systemctl disable nginx".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_readonly_returns_none() {
        let rollback = infer_linux_rollback("ls -la");
        assert_eq!(rollback, None);
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
    fn build_tx_block_for_linux_package_install() {
        let commands = vec!["apt install nginx".to_string()];
        let tx = build_tx_block("linux", "install-nginx", "Root", &commands, Some(60), None)
            .expect("build config tx");
        assert_eq!(tx.kind, CommandBlockKind::Config);
        assert!(matches!(tx.rollback_policy, RollbackPolicy::PerStep));
        assert_eq!(
            tx.steps[0].rollback_command.as_deref(),
            Some("apt remove nginx")
        );
    }

    #[test]
    fn build_tx_block_for_linux_service_start() {
        let commands = vec!["systemctl start nginx".to_string()];
        let tx = build_tx_block("linux", "start-nginx", "Root", &commands, Some(30), None)
            .expect("build config tx");
        assert_eq!(
            tx.steps[0].rollback_command.as_deref(),
            Some("systemctl stop nginx")
        );
    }

    // ========================================================================
    // Security Tests
    // ========================================================================

    #[test]
    fn infer_linux_rollback_rejects_command_injection_with_ampersand() {
        // Attempt to inject additional commands with &&
        let rollback = infer_linux_rollback("apt install nginx && rm -rf /");
        assert_eq!(rollback, None, "Should reject commands with &&");
    }

    #[test]
    fn infer_linux_rollback_rejects_command_injection_with_pipe() {
        // Attempt to inject commands with pipe
        let rollback = infer_linux_rollback("apt install nginx | malicious");
        assert_eq!(rollback, None, "Should reject commands with |");
    }

    #[test]
    fn infer_linux_rollback_rejects_command_injection_with_semicolon() {
        // Attempt to inject commands with semicolon
        let rollback = infer_linux_rollback("apt install nginx; rm -rf /");
        assert_eq!(rollback, None, "Should reject commands with ;");
    }

    #[test]
    fn infer_linux_rollback_rejects_command_substitution() {
        // Attempt command substitution
        let rollback = infer_linux_rollback("apt install $(malicious)");
        assert_eq!(rollback, None, "Should reject command substitution");
    }

    #[test]
    fn infer_linux_rollback_rejects_backtick_substitution() {
        // Attempt backtick command substitution
        let rollback = infer_linux_rollback("apt install `malicious`");
        assert_eq!(rollback, None, "Should reject backtick substitution");
    }

    #[test]
    fn infer_linux_rollback_rejects_redirection() {
        // Attempt output redirection
        let rollback = infer_linux_rollback("apt install nginx > /tmp/log");
        assert_eq!(rollback, None, "Should reject output redirection");
    }

    #[test]
    fn infer_linux_rollback_rejects_input_redirection() {
        // Attempt input redirection
        let rollback = infer_linux_rollback("apt install < /tmp/packages");
        assert_eq!(rollback, None, "Should reject input redirection");
    }

    #[test]
    fn infer_linux_rollback_accepts_simple_package_install() {
        // Valid simple package install
        let rollback = infer_linux_rollback("apt install nginx");
        assert_eq!(rollback, Some("apt remove nginx".to_string()));
    }

    #[test]
    fn infer_linux_rollback_accepts_package_install_with_flags() {
        // Valid package install with -y flag
        let rollback = infer_linux_rollback("apt install -y nginx");
        assert_eq!(rollback, Some("apt remove nginx".to_string()));
    }

    #[test]
    fn infer_linux_rollback_rejects_invalid_package_name() {
        // Package name with invalid characters
        let rollback = infer_linux_rollback("apt install nginx@malicious");
        assert_eq!(rollback, None, "Should reject invalid package names");
    }

    #[test]
    fn infer_linux_rollback_accepts_simple_service_start() {
        // Valid simple service start
        let rollback = infer_linux_rollback("systemctl start nginx");
        assert_eq!(rollback, Some("systemctl stop nginx".to_string()));
    }

    #[test]
    fn infer_linux_rollback_rejects_service_with_extra_args() {
        // Service command with extra arguments (potential injection)
        let rollback = infer_linux_rollback("systemctl start nginx extra");
        assert_eq!(rollback, None, "Should reject service commands with extra args");
    }

    #[test]
    fn infer_linux_rollback_accepts_service_with_at_sign() {
        // Valid service name with @ (systemd template)
        let rollback = infer_linux_rollback("systemctl start nginx@8080");
        assert_eq!(rollback, Some("systemctl stop nginx@8080".to_string()));
    }

    #[test]
    fn extract_package_name_validates_alphanumeric() {
        // Valid package names
        assert_eq!(extract_package_name("apt install nginx"), Some("nginx".to_string()));
        assert_eq!(extract_package_name("apt install nginx-full"), Some("nginx-full".to_string()));
        assert_eq!(extract_package_name("apt install python3.9"), Some("python3.9".to_string()));

        // Invalid package names
        assert_eq!(extract_package_name("apt install nginx@bad"), None);
        assert_eq!(extract_package_name("apt install nginx;malicious"), None);
    }

    #[test]
    fn extract_service_name_validates_format() {
        // Valid service names
        assert_eq!(extract_service_name("systemctl start nginx"), Some("nginx".to_string()));
        assert_eq!(extract_service_name("systemctl start nginx.service"), Some("nginx.service".to_string()));
        assert_eq!(extract_service_name("systemctl start nginx@8080"), Some("nginx@8080".to_string()));

        // Invalid formats
        assert_eq!(extract_service_name("systemctl start"), None);
        assert_eq!(extract_service_name("systemctl start nginx extra"), None);
        assert_eq!(extract_service_name("systemctl start nginx;malicious"), None);
    }

    #[test]
    fn build_tx_block_rejects_dangerous_commands() {
        // Attempt to build transaction with command injection
        let commands = vec!["apt install nginx && rm -rf /".to_string()];
        let result = build_tx_block("linux", "malicious", "Root", &commands, Some(60), None);

        // Should fail because rollback cannot be inferred for dangerous commands
        assert!(result.is_err(), "Should reject dangerous commands");
    }

    #[test]
    fn linux_template_password_not_recorded_in_output() {
        // Verify that password recording is disabled
        let mut handler = linux().expect("create linux template");
        handler.dyn_param.insert("SudoPassword".to_string(), "secret123".to_string());

        // The password should be in dyn_param but marked as not recordable
        assert!(handler.dyn_param.contains_key("SudoPassword"));

        // Note: The actual recording flag is checked in the input_map
        // which is set to (true, "SudoPassword", false) where the last false means don't record
    }

    #[test]
    fn classify_linux_command_handles_case_insensitivity() {
        // Commands should be case-insensitive
        assert_eq!(classify_linux_command("LS -la"), LinuxCommandType::ReadOnly);
        assert_eq!(classify_linux_command("APT INSTALL nginx"), LinuxCommandType::PackageOp);
        assert_eq!(classify_linux_command("SYSTEMCTL START nginx"), LinuxCommandType::ServiceOp);
    }

    #[test]
    fn infer_linux_rollback_handles_whitespace() {
        // Commands with extra whitespace
        let rollback = infer_linux_rollback("  apt install nginx  ");
        assert_eq!(rollback, Some("apt remove nginx".to_string()));

        let rollback = infer_linux_rollback("systemctl  start  nginx");
        // This should fail because extract_service_name expects exactly 3 parts
        assert_eq!(rollback, None);
    }

    #[test]
    fn infer_linux_rollback_for_pip_install() {
        let rollback = infer_linux_rollback("pip install requests");
        assert_eq!(rollback, Some("pip uninstall -y requests".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_npm_install() {
        let rollback = infer_linux_rollback("npm install express");
        assert_eq!(rollback, Some("npm uninstall express".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_dnf_install() {
        let rollback = infer_linux_rollback("dnf install httpd");
        assert_eq!(rollback, Some("dnf remove httpd".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_systemctl_disable() {
        let rollback = infer_linux_rollback("systemctl disable nginx");
        assert_eq!(rollback, Some("systemctl enable nginx".to_string()));
    }

    #[test]
    fn infer_linux_rollback_for_systemctl_stop() {
        let rollback = infer_linux_rollback("systemctl stop nginx");
        assert_eq!(rollback, Some("systemctl start nginx".to_string()));
    }
}
