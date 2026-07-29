//! One persona module per network vendor, mirroring
//! `crate::templates::network`.

pub(crate) mod arista;
pub(crate) mod array;
pub(crate) mod aruba_aoscx;
pub(crate) mod chaitin;
pub(crate) mod checkpoint;
pub(crate) mod cisco;
pub(crate) mod cisco_asa;
pub(crate) mod cisco_nxos;
pub(crate) mod dell_os10;
pub(crate) mod dptech;
pub(crate) mod fortinet;
pub(crate) mod h3c;
pub(crate) mod hillstone;
pub(crate) mod huawei;
pub(crate) mod juniper;
pub(crate) mod leadsec;
pub(crate) mod maipu;
pub(crate) mod paloalto;
pub(crate) mod qianxin;
pub(crate) mod ruijie;
pub(crate) mod topsec;
pub(crate) mod venustech;
pub(crate) mod zte_zxros;

use crate::device::DeviceHandlerConfig;
use crate::testkit::{DEFAULT_ENABLE_PASSWORD, DevicePersona};

/// Shared builder for Cisco-style CLIs (Login/Enable/Config shape).
///
/// Hostname, error text, and the enable-password challenge text differ per
/// vendor (some templates match a lowercase `password:` only).
pub(crate) fn cisco_like(
    config: DeviceHandlerConfig,
    name: &str,
    host: &str,
    error: &str,
    challenge: &str,
) -> DevicePersona {
    DevicePersona::for_config(
        name,
        config,
        "login",
        &[
            ("login", &format!("{host}>")),
            ("enable", &format!("{host}#")),
            ("config", &format!("{host}(config)#")),
        ],
    )
    .with_challenge("enable", challenge, DEFAULT_ENABLE_PASSWORD)
    .with_error_reply(error)
    .with_enable_password(DEFAULT_ENABLE_PASSWORD)
}
