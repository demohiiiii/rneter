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
pub use array::array;
pub use chaitin::chaitin;
pub use checkpoint::checkpoint;
pub use cisco::cisco;
pub use dptech::dptech;
pub use fortinet::fortinet;
pub use h3c::h3c;
pub use hillstone::hillstone;
pub use huawei::huawei;
pub use juniper::juniper;
pub use maipu::maipu;
pub use paloalto::paloalto;
pub use qianxin::qianxin;
pub use topsec::topsec;
pub use venustech::venustech;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceHandler;
    use crate::error::ConnectError;
    use crate::templates::{TemplateCapability, available_templates, by_name, template_metadata};

    struct NetworkTemplateCase {
        name: &'static str,
        builder: fn() -> Result<DeviceHandler, ConnectError>,
        expected_states: &'static [&'static str],
        expected_capabilities: &'static [TemplateCapability],
    }

    fn network_cases() -> Vec<NetworkTemplateCase> {
        vec![
            NetworkTemplateCase {
                name: "arista",
                builder: arista,
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
                expected_states: &["enable"],
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "cisco",
                builder: cisco,
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
                expected_states: &["enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "fortinet",
                builder: fortinet,
                expected_states: &["enable", "vdomenable"],
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "h3c",
                builder: h3c,
                expected_states: &["enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "hillstone",
                builder: hillstone,
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
                expected_states: &["enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "qianxin",
                builder: qianxin,
                expected_states: &["enable", "config"],
                expected_capabilities: &[
                    TemplateCapability::EnableMode,
                    TemplateCapability::ConfigMode,
                ],
            },
            NetworkTemplateCase {
                name: "topsec",
                builder: topsec,
                expected_states: &["enable"],
                expected_capabilities: &[TemplateCapability::EnableMode],
            },
            NetworkTemplateCase {
                name: "venustech",
                builder: venustech,
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
            let via_registry = by_name(case.name)
                .unwrap_or_else(|err| panic!("registry should resolve {}: {}", case.name, err));

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
