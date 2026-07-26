//! Virtual-device persona for the `checkpoint_gaia` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn checkpoint_gaia() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "checkpoint_gaia",
        templates::by_name_config("checkpoint_gaia")?,
        "enable",
        &[("enable", "gw-13800b>")],
    )
    .with_error_reply("CLINFR0329 Invalid command:'make-error'.")
    .with_canned_reply(
        "show version all",
        "Product version Check Point Gaia R81.20\n\
         OS build 631\n\
         OS kernel version 3.10.0-957.21.3cpx86_64",
    )
    .with_canned_reply(
        "show configuration",
        "set hostname gw-13800b\n\
         set interface eth0 ipv4-address 192.168.1.1 mask-length 24\n\
         set interface eth0 state on\n\
         set management interface eth0",
    )
    .with_canned_reply(
        "show interfaces all",
        "Interface eth0\n    \
         state on\n    \
         ipv4-address 192.168.1.1/24\n    \
         link-state link up\n\
         Interface eth1\n    \
         state off",
    ))
}
