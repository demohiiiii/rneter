//! Virtual-device persona for the `array` template, including the
//! sys-captured virtual-site prompts.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::{DEFAULT_ENABLE_PASSWORD, DevicePersona};

pub(crate) fn array() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "array",
        templates::by_name_config("array")?,
        "login",
        &[
            ("login", "AN>"),
            ("enable", "AN#"),
            ("config", "AN(config)#"),
            ("vsiteenable", "vs1$"),
            ("vsiteconfig", "vs1(config)$"),
        ],
    )
    .with_challenge("enable", "Enable password: ", DEFAULT_ENABLE_PASSWORD)
    .with_error_reply("Access denied!")
    .with_enable_password(DEFAULT_ENABLE_PASSWORD)
    .with_canned_reply(
        "show version",
        "Array Networks APV Series\n\
         ArrayOS Rel.APV.10.4.0.20\n\
         Build: 2022-01-10",
    )
    .with_canned_reply(
        "show running-config",
        "hostname AN\n\
         interface port1\n\
         ip address port1 192.168.1.1 255.255.255.0\n\
         webui on",
    )
    .with_canned_reply(
        "show interface",
        "Port    Link    IP Address        Netmask\n\
         port1   up      192.168.1.1       255.255.255.0\n\
         port2   down    0.0.0.0           0.0.0.0",
    ))
}
