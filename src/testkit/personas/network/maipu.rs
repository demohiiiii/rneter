//! Virtual-device persona for the `maipu` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn maipu() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("maipu")?,
        "maipu",
        "MyPower",
        "% Invalid input",
        // The maipu template matches a lowercase `password:` prompt only.
        "password: ",
    )
    .with_canned_reply(
        "show version",
        "MyPower (R) Operating System Software\n\
         MP1800, Version 7.5.3\n\
         Compiled: 2022-06-30",
    )
    .with_canned_reply(
        "show running-config",
        "Building configuration...\n\
         !\n\
         version 7.5.3\n\
         hostname MyPower\n\
         !\n\
         interface gigabitethernet0/1\n \
         ip address 192.168.1.1 255.255.255.0\n\
         !\n\
         end",
    )
    .with_canned_reply(
        "show ip interface brief",
        "Interface              IP-Address      Status    Protocol\n\
         gigabitethernet0/1     192.168.1.1     up        up\n\
         gigabitethernet0/2     unassigned      down      down",
    ))
}
