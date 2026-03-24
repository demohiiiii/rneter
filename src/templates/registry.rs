use crate::device::{DeviceHandler, DeviceHandlerConfig, StateMachineDiagnostics};
use crate::error::ConnectError;

use super::catalog::BUILTIN_TEMPLATES;
use super::linux::{LinuxTemplateConfig, linux_handler_config};
use super::network::{
    arista_config, array_config, chaitin_config, checkpoint_config, cisco_config, dptech_config,
    fortinet_config, h3c_config, hillstone_config, huawei_config, juniper_config, maipu_config,
    paloalto_config, qianxin_config, topsec_config, venustech_config,
};

/// Creates a built-in template by name (case-insensitive).
pub fn by_name(name: &str) -> Result<DeviceHandler, ConnectError> {
    by_name_config(name)?.build()
}

/// Exports the underlying handler configuration for a built-in template by name.
pub fn by_name_config(name: &str) -> Result<DeviceHandlerConfig, ConnectError> {
    match name.to_ascii_lowercase().as_str() {
        "cisco" => Ok(cisco_config()),
        "huawei" => Ok(huawei_config()),
        "h3c" => Ok(h3c_config()),
        "hillstone" => Ok(hillstone_config()),
        "juniper" => Ok(juniper_config()),
        "array" => Ok(array_config()),
        "linux" => Ok(linux_handler_config(LinuxTemplateConfig::default())),
        "arista" => Ok(arista_config()),
        "fortinet" => Ok(fortinet_config()),
        "paloalto" => Ok(paloalto_config()),
        "topsec" => Ok(topsec_config()),
        "venustech" => Ok(venustech_config()),
        "dptech" => Ok(dptech_config()),
        "chaitin" => Ok(chaitin_config()),
        "qianxin" => Ok(qianxin_config()),
        "maipu" => Ok(maipu_config()),
        "checkpoint" => Ok(checkpoint_config()),
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
