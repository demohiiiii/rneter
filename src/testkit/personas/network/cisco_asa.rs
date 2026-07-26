//! Virtual-device persona for the `cisco_asa` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn cisco_asa() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("cisco_asa")?,
        "cisco_asa",
        "ciscoasa",
        "ERROR: forced failure",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Cisco Adaptive Security Appliance Software Version 9.16(4)\n\
         Device Manager Version 7.18(1)\n\
         Hardware:   ASA5516, 8192 MB RAM, CPU Atom C2000 series 2416 MHz",
    )
    .with_canned_reply(
        "show running-config",
        "ASA Version 9.16(4)\n\
         !\n\
         hostname ciscoasa\n\
         !\n\
         interface GigabitEthernet1/1\n \
         nameif outside\n \
         security-level 0\n \
         ip address 203.0.113.2 255.255.255.0\n\
         !\n\
         object network LAN\n \
         subnet 192.168.1.0 255.255.255.0",
    )
    .with_canned_reply(
        "show interface ip brief",
        "Interface                  IP-Address      OK? Method Status                Protocol\n\
         GigabitEthernet1/1         203.0.113.2     YES CONFIG up                    up\n\
         GigabitEthernet1/2         192.168.1.1     YES CONFIG up                    up",
    ))
}
