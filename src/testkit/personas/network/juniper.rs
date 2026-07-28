//! Virtual-device persona for the `juniper_junos` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn juniper_junos() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "juniper_junos",
        templates::by_name_config("juniper_junos")?,
        "enable",
        &[("enable", "admin@SRX>"), ("config", "admin@SRX#")],
    )
    .with_error_reply("syntax error, unexpected input")
    .with_challenge(
        "exit",
        "Exit with uncommitted changes? [yes,no] (yes) ",
        "yes",
    )
    .with_canned_reply(
        "show version",
        "Hostname: SRX\n\
         Model: srx345\n\
         Junos: 21.4R3-S4.9\n\
         JUNOS Software Release [21.4R3-S4.9]",
    )
    .with_canned_reply(
        "show configuration",
        "## Last commit: 2024-05-11 10:22:33 UTC by admin\n\
         version 21.4R3-S4.9;\n\
         system {\n    \
         host-name SRX;\n    \
         services {\n        \
         ssh;\n    \
         }\n\
         }\n\
         interfaces {\n    \
         ge-0/0/0 {\n        \
         unit 0 {\n            \
         family inet {\n                \
         address 192.168.1.1/24;\n            \
         }\n        \
         }\n    \
         }\n\
         }",
    )
    .with_canned_reply(
        "show interfaces terse",
        "Interface               Admin Link Proto    Local                 Remote\n\
         ge-0/0/0                up    up\n\
         ge-0/0/0.0              up    up   inet     192.168.1.1/24\n\
         ge-0/0/1                up    down",
    ))
}
