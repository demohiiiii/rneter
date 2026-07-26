//! Virtual-device persona for the `qianxin` template.
//!
//! The template's error patterns are unanchored (e.g. `.+ exist`), so
//! canned outputs must avoid those substrings anywhere in a line.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn qianxin() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "qianxin",
        templates::by_name_config("qianxin")?,
        "enable",
        &[("enable", "QiAnXin>"), ("config", "QiAnXin-config]")],
    )
    .with_error_reply("% Unknown command.")
    .with_canned_reply(
        "show version",
        "QiAnXin NetGod Firewall\n\
         Software version: NSG-5.6\n\
         Serial number: QAX00112233",
    )
    .with_canned_reply(
        "show running-config",
        "hostname QiAnXin\n\
         interface ge0/0\n \
         ip address 192.168.1.1 255.255.255.0\n\
         security-zone trust\n \
         add interface ge0/0",
    )
    .with_canned_reply(
        "show interface",
        "Interface    Status    IP Address       Zone\n\
         ge0/0        up        192.168.1.1/24   trust\n\
         ge0/1        down      unassigned       untrust",
    ))
}
