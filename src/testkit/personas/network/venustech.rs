//! Virtual-device persona for the `venustech` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn venustech() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("venustech")?,
        "venustech",
        "USG",
        "% forced failure",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Venusense USG Software\n\
         Software Version: V3.6\n\
         System uptime: 88 days",
    )
    .with_canned_reply(
        "show running-config",
        "Building configuration...\n\
         hostname USG\n\
         interface ge0/0\n \
         ip address 192.168.1.1 255.255.255.0\n\
         end",
    )
    .with_canned_reply(
        "show interface",
        "Interface    Status    IP Address        Speed\n\
         ge0/0        up        192.168.1.1/24    1000M\n\
         ge0/1        down      unassigned        auto",
    ))
}
