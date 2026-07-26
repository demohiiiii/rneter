//! Virtual-device persona for the `zte_zxros` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn zte_zxros() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("zte_zxros")?,
        "zte_zxros",
        "ZXR10",
        "% Invalid input",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "ZTE ZXR10 Software, Version: V4.00.10\n\
         Copyright (c) 2000-2020 ZTE Corporation\n\
         System uptime is 45 days 2 hours",
    )
    .with_canned_reply(
        "show running-config",
        "Building configuration...\n\
         hostname ZXR10\n\
         !\n\
         interface gei-0/1/0/1\n  \
         no shutdown\n  \
         ip address 10.0.0.1 255.255.255.0\n\
         !\n\
         end",
    )
    .with_canned_reply(
        "show ip interface brief",
        "Interface         IP-Address      Mask            Admin Phy   Prot\n\
         gei-0/1/0/1       10.0.0.1        255.255.255.0   up    up    up\n\
         gei-0/1/0/2       unassigned      --              down  down  down",
    ))
}
