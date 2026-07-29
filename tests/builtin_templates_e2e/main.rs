//! Per-device virtual-device E2E tests, organized to mirror `src/templates`:
//! one module per vendor under `builtin_templates_e2e/network/`, plus
//! `builtin_templates_e2e/linux.rs`.
//!
//! Each template runs the full scenario from `support::run_full_scenario`:
//! SSH connect (including after-connect hooks), prompt detection, a benign
//! command in every prompt state (walking all transition edges forward and
//! back), vendor-styled error detection, transaction rollback ordering
//! verified from the device's own command log, and session recording.

mod linux;
mod network;
mod support;

use rneter::templates;
use rneter::testkit::builtin_personas;

/// Template names covered by the per-vendor test modules.
///
/// Keep in sync with the modules under `builtin_templates_e2e/`; the guard
/// below fails when a new built-in template ships without its own
/// virtual-device test.
const COVERED_TEMPLATES: &[&str] = &[
    "cisco_ios",
    "cisco_xe",
    "huawei",
    "h3c_comware",
    "hp_comware",
    "hillstone_stoneos",
    "juniper_junos",
    "leadsec_powerv",
    "array",
    "linux",
    "arista_eos",
    "aruba_aoscx",
    "cisco_asa",
    "cisco_nxos",
    "dell_os10",
    "fortinet",
    "paloalto_panos",
    "topsec",
    "venustech",
    "dptech",
    "chaitin",
    "qianxin",
    "maipu",
    "ruijie_os",
    "zte_zxros",
    "checkpoint_gaia",
];

#[test]
fn every_builtin_template_has_its_own_virtual_device_test() {
    let available = templates::available_templates();
    for name in available {
        assert!(
            COVERED_TEMPLATES.contains(name),
            "template '{name}' has no virtual-device test module; add one under \
             tests/builtin_templates_e2e/ and list it in COVERED_TEMPLATES"
        );
    }
    for name in COVERED_TEMPLATES {
        assert!(
            available.contains(name),
            "COVERED_TEMPLATES lists '{name}', which is not a built-in template"
        );
    }
    assert_eq!(COVERED_TEMPLATES.len(), available.len());
}

#[test]
fn every_builtin_template_has_a_persona() {
    let personas = builtin_personas().expect("every builtin template must ship a persona");
    assert_eq!(
        personas.len(),
        templates::available_templates().len(),
        "persona coverage must match the builtin template list"
    );
}
