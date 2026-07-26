//! Virtual-device personas for the `cisco_ios` and `cisco_xe` templates.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn cisco_ios() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("cisco_ios")?,
        "cisco_ios",
        "Router",
        "ERROR: forced failure",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Cisco IOS Software, C2960X Software (C2960X-UNIVERSALK9-M), Version 15.2(7)E7, RELEASE SOFTWARE (fc2)\n\
         Technical Support: http://www.cisco.com/techsupport\n\
         Router uptime is 12 weeks, 3 days, 4 hours, 12 minutes\n\
         System image file is \"flash:c2960x-universalk9-mz.152-7.E7.bin\"",
    )
    .with_canned_reply(
        "show running-config",
        "Building configuration...\n\
         \n\
         Current configuration : 1290 bytes\n\
         !\n\
         version 15.2\n\
         hostname Router\n\
         !\n\
         interface GigabitEthernet0/1\n \
         ip address 192.168.1.1 255.255.255.0\n \
         no shutdown\n\
         !\n\
         line vty 0 4\n \
         transport input ssh\n\
         !\n\
         end",
    )
    .with_canned_reply(
        "show ip interface brief",
        "Interface              IP-Address      OK? Method Status                Protocol\n\
         GigabitEthernet0/1     192.168.1.1     YES manual up                    up\n\
         GigabitEthernet0/2     unassigned      YES unset  administratively down down",
    ))
}

pub(crate) fn cisco_xe() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("cisco_xe")?,
        "cisco_xe",
        "Switch",
        "ERROR: forced failure",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Cisco IOS XE Software, Version 17.03.05\n\
         Cisco IOS Software [Amsterdam], Catalyst L3 Switch Software (CAT9K_IOSXE), Version 17.3.5, RELEASE SOFTWARE (fc2)\n\
         Switch uptime is 1 year, 2 weeks, 5 days",
    )
    .with_canned_reply(
        "show running-config",
        "Building configuration...\n\
         \n\
         Current configuration : 4102 bytes\n\
         !\n\
         version 17.3\n\
         hostname Switch\n\
         !\n\
         interface TenGigabitEthernet1/0/1\n \
         switchport mode trunk\n\
         !\n\
         end",
    )
    .with_canned_reply(
        "show ip interface brief",
        "Interface              IP-Address      OK? Method Status                Protocol\n\
         Vlan1                  10.0.0.2        YES NVRAM  up                    up\n\
         TenGigabitEthernet1/0/1 unassigned     YES unset  up                    up",
    ))
}
