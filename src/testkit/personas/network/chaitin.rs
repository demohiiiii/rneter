//! Virtual-device persona for the `chaitin` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn chaitin() -> Result<DevicePersona, ConnectError> {
    // The chaitin template treats any line containing `Error:` as a device
    // error, so canned outputs must avoid that substring entirely.
    Ok(cisco_like(
        templates::by_name_config("chaitin")?,
        "chaitin",
        "safeline",
        "Error: forced failure",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Chaitin SafeLine WAF\n\
         Version: 6.6.0\n\
         Build: 2023-05-11",
    )
    .with_canned_reply(
        "show running-config",
        "hostname safeline\n\
         interface eth0\n  \
         ip address 192.168.1.5 255.255.255.0\n\
         waf mode transparent",
    )
    .with_canned_reply(
        "show interface",
        "Interface    Link    IP Address        MAC\n\
         eth0         up      192.168.1.5/24    00:0c:29:11:22:33\n\
         eth1         down    unassigned        00:0c:29:11:22:34",
    ))
}
