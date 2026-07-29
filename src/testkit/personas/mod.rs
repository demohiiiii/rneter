//! Ready-made virtual-device personas for every built-in template,
//! organized to mirror `crate::templates`: one module per network vendor
//! under `network/`, plus `linux`.

mod linux;
mod network;

use crate::error::ConnectError;
use crate::templates;

use super::DevicePersona;

/// Creates the persona for a built-in template name.
///
/// Accepts the same names (and aliases) as [`templates::by_name`].
pub(super) fn builtin(template: &str) -> Result<DevicePersona, ConnectError> {
    let Some(canonical) = templates::canonical_template_name(template) else {
        return Err(ConnectError::TemplateNotFound(format!(
            "no testkit persona for template '{template}'"
        )));
    };
    match canonical {
        "cisco_ios" => network::cisco::cisco_ios(),
        "cisco_xe" => network::cisco::cisco_xe(),
        "cisco_asa" => network::cisco_asa::cisco_asa(),
        "cisco_nxos" => network::cisco_nxos::cisco_nxos(),
        "arista_eos" => network::arista::arista_eos(),
        "aruba_aoscx" => network::aruba_aoscx::aruba_aoscx(),
        "dell_os10" => network::dell_os10::dell_os10(),
        "zte_zxros" => network::zte_zxros::zte_zxros(),
        "venustech" => network::venustech::venustech(),
        "maipu" => network::maipu::maipu(),
        "ruijie_os" => network::ruijie::ruijie_os(),
        "chaitin" => network::chaitin::chaitin(),
        "huawei" => network::huawei::huawei(),
        "h3c_comware" => network::h3c::h3c_comware(),
        "hp_comware" => network::h3c::hp_comware(),
        "hillstone_stoneos" => network::hillstone::hillstone_stoneos(),
        "juniper_junos" => network::juniper::juniper_junos(),
        "leadsec_powerv" => network::leadsec::leadsec_powerv(),
        "paloalto_panos" => network::paloalto::paloalto_panos(),
        "fortinet" => network::fortinet::fortinet(),
        "checkpoint_gaia" => network::checkpoint::checkpoint_gaia(),
        "topsec" => network::topsec::topsec(),
        "dptech" => network::dptech::dptech(),
        "qianxin" => network::qianxin::qianxin(),
        "array" => network::array::array(),
        "linux" => linux::linux(),
        other => Err(ConnectError::TemplateNotFound(format!(
            "no testkit persona for template '{other}'"
        ))),
    }
}
