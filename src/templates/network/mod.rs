//! Network device templates.
//!
//! This module contains device templates for various network equipment vendors.

mod arista;
mod array;
mod chaitin;
mod checkpoint;
mod cisco;
mod dptech;
mod fortinet;
mod h3c;
mod hillstone;
mod huawei;
mod juniper;
mod maipu;
mod paloalto;
mod qianxin;
mod topsec;
mod venustech;

pub use arista::arista;
pub use arista::arista_config;
pub use array::array;
pub use array::array_config;
pub use chaitin::chaitin;
pub use chaitin::chaitin_config;
pub use checkpoint::checkpoint;
pub use checkpoint::checkpoint_config;
pub use cisco::cisco;
pub use cisco::cisco_config;
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
pub use maipu::maipu;
pub use maipu::maipu_config;
pub use paloalto::paloalto;
pub use paloalto::paloalto_config;
pub use qianxin::qianxin;
pub use qianxin::qianxin_config;
pub use topsec::topsec;
pub use topsec::topsec_config;
pub use venustech::venustech;
pub use venustech::venustech_config;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceHandler, DeviceHandlerConfig};
    use crate::error::ConnectError;
    use crate::templates::{TemplateCapability, available_templates, by_name, template_metadata};

    struct NetworkTemplateCase {
        name: &'static str,
        builder: fn() -> Result<DeviceHandler, ConnectError>,
        config_builder: fn() -> DeviceHandlerConfig,
        expected_states: &'static [&'static str],
        expected_capabilities: &'static [TemplateCapability],
    }

    fn network_cases() -> Vec<NetworkTemplateCase> {
        vec![
            NetworkTemplateCase {
                name: "arista",
                builder: arista,
                config_builder: arista_config,
                expected_states: &["login", "enable", "config"],
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
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::SysContext,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "chaitin",
                builder: chaitin,
                config_builder: chaitin_config,
                expected_states: &["login", "enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "checkpoint",
                builder: checkpoint,
                config_builder: checkpoint_config,
                expected_states: &["enable"],
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "cisco",
                builder: cisco,
                config_builder: cisco_config,
                expected_states: &["login", "enable", "config"],
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
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "h3c",
                builder: h3c,
                config_builder: h3c_config,
                expected_states: &["enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "hillstone",
                builder: hillstone,
                config_builder: hillstone_config,
                expected_states: &["enable", "config"],
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
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "juniper",
                builder: juniper,
                config_builder: juniper_config,
                expected_states: &["enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "maipu",
                builder: maipu,
                config_builder: maipu_config,
                expected_states: &["login", "enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::LoginMode,
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                    TemplateCapability::InteractiveInput,
                ],
            },
            NetworkTemplateCase {
                name: "paloalto",
                builder: paloalto,
                config_builder: paloalto_config,
                expected_states: &["enable", "config"],
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
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "topsec",
                builder: topsec,
                config_builder: topsec_config,
                expected_states: &["enable"],
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "venustech",
                builder: venustech,
                config_builder: venustech_config,
                expected_states: &["login", "enable", "config"],
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
}
