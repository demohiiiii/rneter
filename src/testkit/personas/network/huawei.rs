//! Virtual-device persona for the `huawei` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn huawei() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "huawei",
        templates::by_name_config("huawei")?,
        "enable",
        &[("enable", "<HUAWEI>"), ("config", "[HUAWEI]")],
    )
    .with_error_reply("Error: forced failure")
    .with_challenge("save", "Are you sure to continue?[Y/N]: ", "y")
    .with_canned_reply(
        "display version",
        "Huawei Versatile Routing Platform Software\n\
         VRP (R) software, Version 8.180 (NE40E V800R010C10SPC500)\n\
         HUAWEI NE40E-X8A uptime is 102 days, 3 hours, 21 minutes",
    )
    .with_canned_reply(
        "display current-configuration",
        "#\n\
         sysname HUAWEI\n\
         #\n\
         interface GigabitEthernet0/0/1\n \
         ip address 192.168.1.1 255.255.255.0\n\
         #\n\
         ssh server enable\n\
         #\n\
         return",
    )
    .with_canned_reply(
        "display interface brief",
        "PHY: Physical\n\
         Interface                   PHY     Protocol  InUti OutUti   inErrors  outErrors\n\
         GigabitEthernet0/0/1        up      up        0.01  0.01          0          0\n\
         GigabitEthernet0/0/2        down    down      0     0             0          0",
    ))
}
