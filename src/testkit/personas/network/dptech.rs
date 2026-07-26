//! Virtual-device persona for the `dptech` template.
//!
//! The template's error patterns are unanchored (`Failed.*`,
//! `.*not exist.*`, `Invalid parameter.*`), so canned outputs must avoid
//! those substrings anywhere in a line.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn dptech() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "dptech",
        templates::by_name_config("dptech")?,
        "enable",
        &[("enable", "<DPTECH>"), ("config", "[DPTECH]")],
    )
    .with_error_reply("% Unknown command")
    .with_canned_reply(
        "show version",
        "DPtech FW1000 Series\n\
         Software Version: FW1000-GC-N\n\
         Conboot Version: 1.12",
    )
    .with_canned_reply(
        "show running-config",
        "sysname DPTECH\n\
         interface gigabitethernet0/1\n \
         ip address 192.168.1.1 255.255.255.0\n\
         security-zone trust\n \
         import interface gigabitethernet0/1",
    )
    .with_canned_reply(
        "show interface brief",
        "Interface            Link   Speed   Duplex  Description\n\
         gigabitethernet0/1   up     1000M   full    uplink\n\
         gigabitethernet0/2   down   auto    auto",
    ))
}
