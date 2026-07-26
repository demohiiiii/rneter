//! Virtual-device persona for the `ruijie_os` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn ruijie_os() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("ruijie_os")?,
        "ruijie_os",
        "Ruijie",
        "% Invalid input",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "System description      : Ruijie Full 10G Routing Switch (S5760C) By Ruijie Networks\n\
         System uptime           : 30 days, 6 hours\n\
         System software version : S5760C_RGOS 11.4(1)B70P2",
    )
    .with_canned_reply(
        "show running-config",
        "Building configuration...\n\
         !\n\
         hostname Ruijie\n\
         !\n\
         interface GigabitEthernet 0/1\n \
         ip address 192.168.1.1 255.255.255.0\n\
         !\n\
         end",
    )
    .with_canned_reply(
        "show ip interface brief",
        "Interface                        IP-Address(Pri)    IP-Address(Sec)    Status    Protocol\n\
         GigabitEthernet 0/1              192.168.1.1/24     no address         up        up\n\
         GigabitEthernet 0/2              no address         no address         down      down",
    ))
}
