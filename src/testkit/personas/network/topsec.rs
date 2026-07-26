//! Virtual-device persona for the `topsec` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn topsec() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "topsec",
        templates::by_name_config("topsec")?,
        "enable",
        &[("enable", "TopsecOS#")],
    )
    .with_error_reply("error: forced failure")
    .with_canned_reply(
        "system version show",
        "Topsec Operating System\n\
         TOS Version: 3.3.005.057\n\
         NGFW4000-UF",
    )
    .with_canned_reply(
        "system config show",
        // The reply must not start with a prefix of the command itself:
        // such a line is indistinguishable from the command echo and gets
        // filtered from cleaned output.
        "hostname TopsecOS\n\
         network interface eth0 ip add 192.168.1.1 mask 255.255.255.0\n\
         pf service ssh area area_eth0 addressname any",
    )
    .with_canned_reply(
        "network interface show",
        "eth0: flags=UP,BROADCAST,RUNNING mtu 1500\n    \
         inet 192.168.1.1 netmask 255.255.255.0\n\
         eth1: flags=DOWN mtu 1500",
    ))
}
