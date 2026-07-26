//! Virtual-device persona for the `fortinet` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn fortinet() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "fortinet",
        templates::by_name_config("fortinet")?,
        "enable",
        &[("enable", "FGT60F #")],
    )
    .with_error_reply("Command fail. Return code -61")
    .with_canned_reply(
        "get system status",
        "Version: FortiGate-60F v7.2.5,build1517,230508 (GA.F)\n\
         Serial-Number: FGT60FTK20000000\n\
         Hostname: FGT60F\n\
         Operation Mode: NAT",
    )
    .with_canned_reply(
        "show system interface",
        "config system interface\n    \
         edit \"wan1\"\n        \
         set vdom \"root\"\n        \
         set ip 203.0.113.2 255.255.255.0\n        \
         set allowaccess ping https ssh\n    \
         next\n    \
         edit \"lan\"\n        \
         set ip 192.168.1.99 255.255.255.0\n    \
         next\n\
         end",
    )
    .with_canned_reply(
        "get system performance status",
        "CPU states: 2 user 1 system 0 nice 97 idle\n\
         Memory: 2054768k total, 866588k used (42.2), 1188180k free\n\
         Uptime: 66 days, 3 hours, 12 minutes",
    ))
}
