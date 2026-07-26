//! Virtual-device persona for the `hillstone_stoneos` template.
//!
//! The template treats any line containing `%` (or `^`) as a device error,
//! so canned outputs must avoid those characters entirely.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn hillstone_stoneos() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "hillstone_stoneos",
        templates::by_name_config("hillstone_stoneos")?,
        "enable",
        &[("enable", "SG-6000#"), ("config", "SG-6000(config)#")],
    )
    .with_error_reply("testkit % forced failure")
    .with_canned_reply(
        "show version",
        "Hillstone StoneOS software, Version 5.5R9\n\
         Product name: SG-6000-E3660\n\
         Uptime is 66 days 12 hours",
    )
    .with_canned_reply(
        "show configuration",
        "Building configuration..\n\
         hostname \"SG-6000\"\n\
         interface ethernet0/0\n  \
         zone \"trust\"\n  \
         ip address 192.168.1.1 255.255.255.0\n\
         exit\n\
         rule id 1\n  \
         action permit\n\
         exit",
    )
    .with_canned_reply(
        "show interface",
        "Interface         IP Address/Mask      Zone       H A MAC\n\
         ethernet0/0       192.168.1.1/24       trust      U U 001c.5401.0001\n\
         ethernet0/1       0.0.0.0/0            untrust    D D 001c.5401.0002",
    ))
}
