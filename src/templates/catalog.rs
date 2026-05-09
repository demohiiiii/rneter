use crate::error::ConnectError;
use super::detect_profile::{TemplateDetectProfile, TemplateProbe, TemplateProbeRule};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Built-in template names supported by this crate.
pub const BUILTIN_TEMPLATES: &[&str] = &[
    "cisco",
    "huawei",
    "h3c",
    "hillstone",
    "juniper",
    "array",
    "linux",
    "arista",
    "fortinet",
    "paloalto",
    "topsec",
    "venustech",
    "dptech",
    "chaitin",
    "qianxin",
    "maipu",
    "checkpoint",
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
    pub detect_profile: Option<TemplateDetectProfile>,
}

fn rule(pattern: &str, weight: u32) -> TemplateProbeRule {
    TemplateProbeRule {
        pattern: pattern.to_string(),
        weight,
    }
}

fn probe_with_errors(
    command: &str,
    rules: Vec<TemplateProbeRule>,
    error_patterns: Vec<&str>,
) -> TemplateProbe {
    TemplateProbe {
        command: command.to_string(),
        rules,
        error_patterns: error_patterns.into_iter().map(str::to_string).collect(),
    }
}

fn cisco_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^[^\s<]+>\s*$", 15), rule(r"^[^\s#]+#\s*$", 15)],
        probes: vec![probe_with_errors(
            "show version",
            vec![rule(
                r"Cisco IOS Software|Cisco Internetwork Operating System Software|Cisco IOS XE Software|Cisco Adaptive Security Appliance|Cisco ASA",
                95,
            )],
            vec![r"Invalid input", r"Unknown command", r"Unrecognized command"],
        )],
    }
}

fn juniper_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^\S+@\S+[>#]\s*$", 20)],
        probes: vec![probe_with_errors(
            "show version",
            vec![rule(
                r"JUNOS Software Release|JUNOS .+ Software|JUNOS OS Kernel|JUNOS Base Version",
                99,
            )],
            vec![r"unknown command", r"syntax error", r"Invalid input"],
        )],
    }
}

fn huawei_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^<[^>]+>\s*$", 15), rule(r"^\[[^\]]+\]\s*$", 15)],
        probes: vec![probe_with_errors(
            "display version",
            vec![rule(
                r"Huawei Technologies|Huawei Versatile Routing Platform Software|Huawei Versatile Routing Platform|VRP \(R\)",
                99,
            )],
            vec![r"Unrecognized command", r"Invalid input", r"Error:"],
        )],
    }
}

fn h3c_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^<[^>]+>\s*$", 15), rule(r"^\[[^\]]+\]\s*$", 15)],
        probes: vec![probe_with_errors(
            "display version",
            vec![rule(r"H3C Comware Software|HPE Comware|HP Comware|Comware Software", 99)],
            vec![r"Unrecognized command", r"Invalid input", r"Error:"],
        )],
    }
}

fn linux_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![
            rule(r"^[^\n]*[$#]\s*$", 10),
            rule(r"^\S+@\S+:[^\n]*[$#]\s*$", 20),
        ],
        probes: vec![
            probe_with_errors(
                "uname -a",
                vec![rule(r"Linux", 70)],
                vec![r"command not found", r"not recognized", r"Unknown command"],
            ),
            probe_with_errors(
                "echo $SHELL",
                vec![rule(r"/(?:ba|z|fi|da)?sh", 20)],
                vec![r"command not found", r"not recognized", r"Unknown command"],
            ),
        ],
    }
}

fn hillstone_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^[A-Za-z0-9._-]+#\s*$", 10)],
        probes: vec![probe_with_errors(
            "show version",
            vec![rule(r"Hillstone Networks StoneOS software|StoneOS software", 99)],
            vec![r"Invalid input", r"Unknown command", r"Unrecognized command"],
        )],
    }
}

fn arista_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^[^\s<]+>\s*$", 15), rule(r"^[^\s#]+#\s*$", 15)],
        probes: vec![probe_with_errors(
            "show version",
            vec![rule(r"Arista|EOS", 90)],
            vec![r"Invalid input", r"Unknown command", r"Unrecognized command"],
        )],
    }
}

fn fortinet_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^[\w.-]+\s*[#$]\s*$", 10)],
        probes: vec![probe_with_errors(
            "get system status",
            vec![rule(r"FortiGate|FortiOS", 90)],
            vec![r"Command fail", r"Unknown action", r"Unknown command"],
        )],
    }
}

fn paloalto_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^[^\s<]+>\s*$", 10), rule(r"^[^\s#]+#\s*$", 10)],
        probes: vec![probe_with_errors(
            "show system info",
            vec![rule(r"PAN-OS|sw-version|model:\s+PA-", 90)],
            vec![r"Invalid syntax", r"Unknown command", r"command not found"],
        )],
    }
}

fn checkpoint_detect_profile() -> TemplateDetectProfile {
    TemplateDetectProfile {
        initial_rules: vec![rule(r"^[^\s<]+>\s*$", 10), rule(r"^[^\s#]+#\s*$", 10)],
        probes: vec![probe_with_errors(
            "show version all",
            vec![rule(r"Check Point|Gaia", 90)],
            vec![r"Invalid command", r"command not found", r"Unknown command"],
        )],
    }
}

pub(crate) fn metadata_for(name: &str) -> Option<TemplateMetadata> {
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
            detect_profile: Some(cisco_detect_profile()),
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
            detect_profile: Some(huawei_detect_profile()),
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
            detect_profile: Some(h3c_detect_profile()),
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
            detect_profile: Some(hillstone_detect_profile()),
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
            detect_profile: Some(juniper_detect_profile()),
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
            detect_profile: None,
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
            detect_profile: Some(linux_detect_profile()),
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
            detect_profile: Some(arista_detect_profile()),
        },
        "fortinet" => TemplateMetadata {
            name: "fortinet".to_string(),
            vendor: "Fortinet".to_string(),
            family: "FortiGate".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![TemplateCapability::EnableMode],
            detect_profile: Some(fortinet_detect_profile()),
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
            detect_profile: Some(paloalto_detect_profile()),
        },
        "topsec" => TemplateMetadata {
            name: "topsec".to_string(),
            vendor: "Topsec".to_string(),
            family: "NGFW".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![TemplateCapability::EnableMode],
            detect_profile: None,
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
            detect_profile: None,
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
            detect_profile: None,
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
            detect_profile: None,
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
            detect_profile: None,
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
            detect_profile: None,
        },
        "checkpoint" => TemplateMetadata {
            name: "checkpoint".to_string(),
            vendor: "Check Point".to_string(),
            family: "Security Gateway".to_string(),
            template_version: "1.0.0".to_string(),
            capabilities: vec![TemplateCapability::EnableMode],
            detect_profile: Some(checkpoint_detect_profile()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::detect_profile_by_name;

    #[test]
    fn available_templates_contains_expected_names() {
        let names = available_templates();
        assert!(names.contains(&"cisco"));
        assert!(names.contains(&"juniper"));
        assert!(names.contains(&"array"));
        assert!(names.contains(&"linux"));
        assert!(names.contains(&"arista"));
    }

    #[test]
    fn template_catalog_has_metadata_for_all_builtin_templates() {
        let catalog = template_catalog();
        assert_eq!(catalog.len(), BUILTIN_TEMPLATES.len());
        assert!(catalog.iter().any(|m| m.name == "cisco"));
        assert!(catalog.iter().any(|m| m.name == "array"));
        assert!(catalog.iter().any(|m| m.name == "linux"));
    }

    #[test]
    fn template_metadata_is_case_insensitive() {
        let meta = template_metadata("JuNiPeR").expect("metadata should resolve");
        assert_eq!(meta.name, "juniper");
        assert_eq!(meta.vendor, "Juniper");
    }

    #[test]
    fn cisco_metadata_includes_detect_profile() {
        let meta = template_metadata("cisco").expect("cisco metadata");
        assert!(meta.detect_profile.is_some());
    }

    #[test]
    fn builtin_detect_profiles_exist_for_extended_builtin_templates() {
        for name in [
            "cisco",
            "juniper",
            "huawei",
            "h3c",
            "hillstone",
            "linux",
            "arista",
            "fortinet",
            "paloalto",
            "checkpoint",
        ] {
            assert!(
                detect_profile_by_name(name).is_some(),
                "missing detect profile for {name}"
            );
        }
    }

    #[test]
    fn builtin_detect_profiles_include_error_patterns_for_probe_commands() {
        let cisco = detect_profile_by_name("cisco").expect("cisco detect profile");
        assert!(cisco
            .probes
            .iter()
            .any(|probe| !probe.error_patterns.is_empty()));
    }
}
