//! Network device templates.
//!
//! This module contains device templates for various network equipment vendors.

mod arista;
mod array;
mod aruba_aoscx;
mod chaitin;
mod checkpoint;
mod cisco;
mod cisco_asa;
mod cisco_nxos;
mod dell_os10;
mod dptech;
mod fortinet;
mod h3c;
mod hillstone;
mod huawei;
mod juniper;
mod leadsec;
mod maipu;
mod paloalto;
mod qianxin;
mod ruijie;
mod topsec;
mod venustech;
mod zte_zxros;

pub use arista::arista;
pub use arista::arista_config;
pub use array::array;
pub use array::array_config;
pub use aruba_aoscx::aruba_aoscx;
pub use aruba_aoscx::aruba_aoscx_config;
pub use chaitin::chaitin;
pub use chaitin::chaitin_config;
pub use checkpoint::checkpoint;
pub use checkpoint::checkpoint_config;
pub use cisco::cisco;
pub use cisco::cisco_config;
pub use cisco_asa::cisco_asa;
pub use cisco_asa::cisco_asa_config;
pub use cisco_nxos::cisco_nxos;
pub use cisco_nxos::cisco_nxos_config;
pub use dell_os10::dell_os10;
pub use dell_os10::dell_os10_config;
pub use dptech::dptech;
pub use dptech::dptech_config;
pub use fortinet::fortinet;
pub use fortinet::fortinet_config;
pub use h3c::h3c;
pub use h3c::h3c_config;
pub use hillstone::hillstone;
pub use hillstone::hillstone_config;
pub use huawei::huawei;
pub use huawei::huawei_config;
pub use juniper::juniper;
pub use juniper::juniper_config;
pub use leadsec::leadsec;
pub use leadsec::leadsec_config;
pub use maipu::maipu;
pub use maipu::maipu_config;
pub use paloalto::paloalto;
pub use paloalto::paloalto_config;
pub use qianxin::qianxin;
pub use qianxin::qianxin_config;
pub use ruijie::ruijie;
pub use ruijie::ruijie_config;
pub use topsec::topsec;
pub use topsec::topsec_config;
pub use venustech::venustech;
pub use venustech::venustech_config;
pub use zte_zxros::zte_zxros;
pub use zte_zxros::zte_zxros_config;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceHandler, DeviceHandlerConfig, DevicePromptRule};
    use crate::error::ConnectError;
    use crate::session::SessionOperation;
    use crate::templates::{
        DetectSnapshot, TemplateCapability, available_templates, by_name, score_builtin_templates,
        template_metadata,
    };
    use std::collections::HashMap;

    fn prompt_state_order(prompts: &[DevicePromptRule]) -> Vec<&str> {
        prompts.iter().map(|rule| rule.state.as_str()).collect()
    }

    fn assert_prompt_order(prompts: &[DevicePromptRule], expected: &[&str], name: &str) {
        let actual = prompt_state_order(prompts);
        assert_eq!(
            actual, expected,
            "prompt order mismatch for {name}; actual prompt states: {:?}",
            actual
        );
    }

    fn after_connect_command_strings(config: &DeviceHandlerConfig) -> Vec<String> {
        config
            .hooks
            .after_connect
            .iter()
            .filter_map(|action| match &action.operation {
                SessionOperation::Command(command) => Some(command.command.clone()),
                _ => None,
            })
            .collect()
    }

    fn assert_best_match_from_show_version(template_name: &str, output: &str) {
        let report = score_builtin_templates(&DetectSnapshot {
            initial_output: String::new(),
            initial_prompt: "device#".to_string(),
            probe_outputs: HashMap::from([("show version".to_string(), output.to_string())]),
        });
        let best = report
            .best_match
            .unwrap_or_else(|| panic!("expected autodetect match for {template_name}"));
        assert_eq!(
            best.template_name, template_name,
            "unexpected best match for show version output {output:?}; candidates: {:?}",
            report.candidates
        );
    }

    type ConfigBuilder = fn() -> DeviceHandlerConfig;

    struct NetworkTemplateCase {
        name: &'static str,
        builder: fn() -> Result<DeviceHandler, ConnectError>,
        config_builder: ConfigBuilder,
        expected_states: &'static [&'static str],
        expected_capabilities: &'static [TemplateCapability],
        expected_prompt_order: &'static [&'static str],
    }

    fn network_cases() -> Vec<NetworkTemplateCase> {
        vec![
            NetworkTemplateCase {
                name: "arista_eos",
                builder: arista,
                config_builder: arista_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "array",
                builder: array,
                config_builder: array_config,
                expected_states: &["login", "enable", "config", "vsiteenable", "vsiteconfig"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::SysContext,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "aruba_aoscx",
                builder: aruba_aoscx,
                config_builder: aruba_aoscx_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "chaitin",
                builder: chaitin,
                config_builder: chaitin_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "checkpoint_gaia",
                builder: checkpoint,
                config_builder: checkpoint_config,
                expected_states: &["enable"],
                expected_prompt_order: &["Enable"],
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "cisco_ios",
                builder: cisco,
                config_builder: cisco_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "cisco_asa",
                builder: cisco_asa,
                config_builder: cisco_asa_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "cisco_nxos",
                builder: cisco_nxos,
                config_builder: cisco_nxos_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "dell_os10",
                builder: dell_os10,
                config_builder: dell_os10_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "dptech",
                builder: dptech,
                config_builder: dptech_config,
                expected_states: &["enable", "config"],
                expected_prompt_order: &["Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "fortinet",
                builder: fortinet,
                config_builder: fortinet_config,
                expected_states: &["enable", "vdomenable"],
                expected_prompt_order: &["Enable"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::SysContext,
                ],
            },
            NetworkTemplateCase {
                name: "h3c_comware",
                builder: h3c,
                config_builder: h3c_config,
                expected_states: &["enable", "config"],
                expected_prompt_order: &["Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "hillstone_stoneos",
                builder: hillstone,
                config_builder: hillstone_config,
                expected_states: &["enable", "config"],
                expected_prompt_order: &["Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "huawei",
                builder: huawei,
                config_builder: huawei_config,
                expected_states: &["enable", "config"],
                expected_prompt_order: &["Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "juniper_junos",
                builder: juniper,
                config_builder: juniper_config,
                expected_states: &["enable", "config"],
                expected_prompt_order: &["Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "leadsec_powerv",
                builder: leadsec,
                config_builder: leadsec_config,
                expected_states: &["login"],
                expected_prompt_order: &["Login"],
                expected_capabilities: &[TemplateCapability::LoginMode],
            },
            NetworkTemplateCase {
                name: "maipu",
                builder: maipu,
                config_builder: maipu_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "paloalto_panos",
                builder: paloalto,
                config_builder: paloalto_config,
                expected_states: &["enable", "config"],
                expected_prompt_order: &["Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "qianxin",
                builder: qianxin,
                config_builder: qianxin_config,
                expected_states: &["enable", "config"],
                expected_prompt_order: &["Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "ruijie_os",
                builder: ruijie,
                config_builder: ruijie_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "topsec",
                builder: topsec,
                config_builder: topsec_config,
                expected_states: &["enable"],
                expected_prompt_order: &["Enable"],
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "venustech",
                builder: venustech,
                config_builder: venustech_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "zte_zxros",
                builder: zte_zxros,
                config_builder: zte_zxros_config,
                expected_states: &["login", "enable", "config"],
                expected_prompt_order: &["Login", "Enable", "Config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
        ]
    }

    #[test]
    fn network_templates_build_and_registry_match() {
        for case in network_cases() {
            let direct = (case.builder)().unwrap_or_else(|err| {
                panic!("direct builder should work for {}: {}", case.name, err)
            });
            let from_config = (case.config_builder)().build().unwrap_or_else(|err| {
                panic!("config builder should work for {}: {}", case.name, err)
            });
            let via_registry = by_name(case.name)
                .unwrap_or_else(|err| panic!("registry should resolve {}: {}", case.name, err));

            assert_eq!(
                direct.states(),
                from_config.states(),
                "config state mismatch for {}",
                case.name
            );
            assert_eq!(
                direct.edges(),
                from_config.edges(),
                "config edge mismatch for {}",
                case.name
            );
            assert!(
                direct.is_equivalent(&from_config),
                "config-built handler should be equivalent for {}",
                case.name
            );

            assert_eq!(
                direct.states(),
                via_registry.states(),
                "state mismatch for {}",
                case.name
            );
            assert_eq!(
                direct.edges(),
                via_registry.edges(),
                "edge mismatch for {}",
                case.name
            );

            let diagnostics = via_registry.diagnose_state_machine();
            assert!(
                diagnostics.missing_edge_sources.is_empty(),
                "missing edge source(s) for {}: {:?}",
                case.name,
                diagnostics.missing_edge_sources
            );
            assert!(
                diagnostics.missing_edge_targets.is_empty(),
                "missing edge target(s) for {}: {:?}",
                case.name,
                diagnostics.missing_edge_targets
            );

            let states = via_registry.states();
            for expected_state in case.expected_states {
                assert!(
                    states.iter().any(|state| state == expected_state),
                    "missing state {} for template {}; actual states: {:?}",
                    expected_state,
                    case.name,
                    states
                );
            }
        }
    }

    #[test]
    fn network_template_catalog_matches_expectations() {
        let names = available_templates();

        for case in network_cases() {
            assert!(
                names.contains(&case.name),
                "template {} missing from available_templates",
                case.name
            );

            let metadata = template_metadata(case.name)
                .unwrap_or_else(|err| panic!("metadata should exist for {}: {}", case.name, err));

            for capability in case.expected_capabilities {
                assert!(
                    metadata.capabilities.contains(capability),
                    "template {} missing capability {:?}; actual capabilities: {:?}",
                    case.name,
                    capability,
                    metadata.capabilities
                );
            }
        }
    }

    #[test]
    fn network_template_prompts_follow_default_privilege_order() {
        for case in network_cases() {
            let config = (case.config_builder)();
            assert_prompt_order(&config.prompt, case.expected_prompt_order, case.name);
        }
    }

    #[test]
    fn cisco_like_prompts_distinguish_enable_and_config_modes() {
        let cases: &[(&str, ConfigBuilder)] = &[
            ("arista_eos", arista_config),
            ("array", array_config),
            ("aruba_aoscx", aruba_aoscx_config),
            ("chaitin", chaitin_config),
            ("cisco_ios", cisco_config),
            ("cisco_asa", cisco_asa_config),
            ("cisco_nxos", cisco_nxos_config),
            ("dell_os10", dell_os10_config),
            ("hillstone", hillstone_config),
            ("maipu", maipu_config),
            ("ruijie", ruijie_config),
            ("venustech", venustech_config),
            ("zte_zxros", zte_zxros_config),
        ];

        for (name, config_builder) in cases {
            let mut handler = config_builder()
                .build()
                .unwrap_or_else(|err| panic!("failed to build {name}: {err}"));

            handler.read("\rdevice#");
            assert_eq!(handler.current_state(), "enable", "template: {name}");

            for prompt in ["\rdevice(config)#", "\rdevice(config-if)#"] {
                handler.read(prompt);
                assert_eq!(
                    handler.current_state(),
                    "config",
                    "template: {name}, prompt: {prompt:?}"
                );
            }
        }
    }

    #[test]
    fn netdriver_prompt_compatibility_regressions_keep_vendor_specific_modes() {
        let mut array = array().expect("create Array handler");
        for (prompt, state, sys) in [
            ("AN#", "enable", None),
            ("vs1$", "vsiteenable", Some("vs1")),
            ("vs1(config)$", "vsiteconfig", Some("vs1")),
        ] {
            array.read(prompt);
            assert_eq!(array.current_state(), state, "Array prompt: {prompt:?}");
            assert_eq!(array.current_sys(), sys, "Array prompt: {prompt:?}");
        }

        let mut topsec = topsec().expect("create TopSec handler");
        for prompt in ["TopsecOS#", "TopsecOS%"] {
            assert!(
                topsec.read_prompt(prompt),
                "TopSec prompt should match: {prompt:?}"
            );
        }

        let mut qianxin = qianxin().expect("create QiAnXin handler");
        for prompt in [
            "QiAnXin-config]",
            "QiAnXin-config-policy-security]",
            "QiAnXin-config-object-address-group]",
        ] {
            qianxin.read(prompt);
            assert_eq!(
                qianxin.current_state(),
                "config",
                "QiAnXin prompt: {prompt:?}"
            );
        }
    }

    #[test]
    fn netmiko_driver_session_preparation_is_reflected_in_new_templates() {
        let aruba_hooks = after_connect_command_strings(&aruba_aoscx_config());
        assert!(aruba_hooks.contains(&"no page".to_string()));
        assert!(
            aruba_aoscx_config()
                .edges
                .iter()
                .any(|edge| edge.from_state == "Enable"
                    && edge.command == "configure term"
                    && edge.to_state == "Config")
        );

        let nxos_hooks = after_connect_command_strings(&cisco_nxos_config());
        assert!(nxos_hooks.contains(&"terminal width 511".to_string()));
        assert!(nxos_hooks.contains(&"terminal length 0".to_string()));

        let dell_os10_hooks = after_connect_command_strings(&dell_os10_config());
        assert!(dell_os10_hooks.contains(&"terminal length 0".to_string()));

        let ruijie_hooks = after_connect_command_strings(&ruijie_config());
        assert!(ruijie_hooks.contains(&"terminal width 256".to_string()));
        assert!(ruijie_hooks.contains(&"terminal length 0".to_string()));

        let h3c_hooks = after_connect_command_strings(&h3c_config());
        assert!(h3c_hooks.contains(&"screen-length disable".to_string()));

        let zte_hooks = after_connect_command_strings(&zte_zxros_config());
        assert!(zte_hooks.contains(&"terminal length 0".to_string()));
    }

    #[test]
    fn netmiko_driver_ruijie_password_change_prompt_is_reflected() {
        let mut handler = ruijie().expect("create ruijie handler");
        assert_eq!(
            handler.read_need_write("Do you want to change the password"),
            Some(("n".to_string(), true))
        );
    }

    #[test]
    fn netmiko_autodetect_markers_score_expected_new_templates() {
        assert_best_match_from_show_version("aruba_aoscx", "ArubaOS-CX");
        assert_best_match_from_show_version("aruba_aoscx", "AOS-CX");
        assert_best_match_from_show_version("cisco_asa", "Cisco Adaptive Security Appliance");
        assert_best_match_from_show_version("cisco_asa", "Cisco ASA");
        assert_best_match_from_show_version("cisco_nxos", "Cisco Nexus Operating System");
        assert_best_match_from_show_version("cisco_nxos", "NX-OS");
        assert_best_match_from_show_version("dell_os10", "Dell EMC Networking OS10 Enterprise");
        assert_best_match_from_show_version("dell_os10", "Dell SmartFabric OS10-Enterprise");
    }
}
