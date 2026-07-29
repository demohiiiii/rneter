use crate::device::{DeviceHandler, DeviceHandlerConfig, StateMachineDiagnostics};
use crate::error::ConnectError;
use crate::templates::TemplateDetectProfile;

use super::catalog::{BUILTIN_TEMPLATES, canonical_template_name};
use super::linux::linux_handler_config;
use super::network::{
    arista_config, array_config, aruba_aoscx_config, chaitin_config, checkpoint_config,
    cisco_asa_config, cisco_config, cisco_nxos_config, dell_os10_config, dptech_config,
    fortinet_config, h3c_config, hillstone_config, huawei_config, juniper_config, leadsec_config,
    maipu_config, paloalto_config, qianxin_config, ruijie_config, topsec_config, venustech_config,
    zte_zxros_config,
};

/// Creates a built-in template by name (case-insensitive).
pub fn by_name(name: &str) -> Result<DeviceHandler, ConnectError> {
    by_name_config(name)?.build()
}

/// Exports the underlying handler configuration for a built-in template by name.
pub fn by_name_config(name: &str) -> Result<DeviceHandlerConfig, ConnectError> {
    match canonical_template_name(name) {
        Some("cisco_ios" | "cisco_xe") => Ok(cisco_config()),
        Some("huawei") => Ok(huawei_config()),
        Some("h3c_comware" | "hp_comware") => Ok(h3c_config()),
        Some("hillstone_stoneos") => Ok(hillstone_config()),
        Some("juniper_junos") => Ok(juniper_config()),
        Some("leadsec_powerv") => Ok(leadsec_config()),
        Some("array") => Ok(array_config()),
        Some("linux") => Ok(linux_handler_config()),
        Some("arista_eos") => Ok(arista_config()),
        Some("aruba_aoscx") => Ok(aruba_aoscx_config()),
        Some("cisco_asa") => Ok(cisco_asa_config()),
        Some("cisco_nxos") => Ok(cisco_nxos_config()),
        Some("dell_os10") => Ok(dell_os10_config()),
        Some("fortinet") => Ok(fortinet_config()),
        Some("paloalto_panos") => Ok(paloalto_config()),
        Some("topsec") => Ok(topsec_config()),
        Some("venustech") => Ok(venustech_config()),
        Some("dptech") => Ok(dptech_config()),
        Some("chaitin") => Ok(chaitin_config()),
        Some("qianxin") => Ok(qianxin_config()),
        Some("maipu") => Ok(maipu_config()),
        Some("ruijie_os") => Ok(ruijie_config()),
        Some("zte_zxros") => Ok(zte_zxros_config()),
        Some("checkpoint_gaia") => Ok(checkpoint_config()),
        _ => Err(ConnectError::TemplateNotFound(name.to_string())),
    }
}

/// Returns one built-in detect profile by template name (case-insensitive).
pub fn detect_profile_by_name(name: &str) -> Option<TemplateDetectProfile> {
    super::catalog::template_metadata(name)
        .ok()
        .and_then(|metadata| metadata.detect_profile)
}

/// Returns all built-in detect profiles that are currently registered.
pub fn available_detect_profiles() -> Vec<(String, TemplateDetectProfile)> {
    BUILTIN_TEMPLATES
        .iter()
        .filter_map(|name| {
            detect_profile_by_name(name).map(|profile| ((*name).to_string(), profile))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_name_is_case_insensitive() {
        let handler = by_name("CiScO").expect("cisco template should load");
        let diagnostics = handler.diagnose_state_machine();
        assert!(diagnostics.missing_edge_sources.is_empty());
        assert!(diagnostics.missing_edge_targets.is_empty());
    }

    #[test]
    fn by_name_config_builds_equivalent_handler() {
        let config = by_name_config("CiScO").expect("cisco config should load");
        let handler = by_name("cisco").expect("cisco handler should load");
        let rebuilt = config.build().expect("config should build");

        assert!(handler.is_equivalent(&rebuilt));
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
