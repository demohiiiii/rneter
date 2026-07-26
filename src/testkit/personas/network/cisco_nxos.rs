//! Virtual-device persona for the `cisco_nxos` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn cisco_nxos() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("cisco_nxos")?,
        "cisco_nxos",
        "switch",
        "% Invalid command",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Cisco Nexus Operating System (NX-OS) Software\n\
         NXOS: version 9.3(10)\n\
         Hardware\n  cisco Nexus9000 C9336C-FX2 Chassis",
    )
    .with_canned_reply(
        "show running-config",
        // Real NX-OS prints a leading `!Command: show running-config`
        // line; it is omitted here because a line ending in the sent
        // command is indistinguishable from the command echo and gets
        // filtered from cleaned output.
        "!Running configuration last done at: Mon May  6 10:21:01 2024\n\
         version 9.3(10) Bios:version 05.45\n\
         hostname switch\n\
         \n\
         interface Ethernet1/1\n  \
         description uplink\n  \
         no shutdown",
    )
    .with_canned_reply(
        "show interface brief",
        "Ethernet      VLAN    Type Mode   Status  Reason                 Speed     Port\n\
         Eth1/1        1       eth  access up      none                   10G(D)    --\n\
         Eth1/2        1       eth  access down    Administratively down  auto(D)   --",
    ))
}
