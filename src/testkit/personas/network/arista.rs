//! Virtual-device persona for the `arista_eos` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn arista_eos() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("arista_eos")?,
        "arista_eos",
        "switch",
        "% Unrecognized command",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Arista DCS-7050SX3-48YC8\n\
         Hardware version: 11.01\n\
         Software image version: 4.28.3M\n\
         Architecture: x86_64",
    )
    .with_canned_reply(
        "show running-config",
        "! device: switch (DCS-7050SX3, EOS-4.28.3M)\n\
         !\n\
         hostname switch\n\
         !\n\
         interface Ethernet1\n   \
         switchport mode trunk\n\
         !\n\
         end",
    )
    .with_canned_reply(
        "show interfaces status",
        "Port       Name   Status       Vlan     Duplex Speed  Type\n\
         Et1               connected    trunk    full   10G    10GBASE-SR\n\
         Et2               notconnect   1        auto   auto   Not Present",
    ))
}
