//! Virtual-device persona for the `dell_os10` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn dell_os10() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("dell_os10")?,
        "dell_os10",
        "OS10",
        "% Error: forced failure",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "Dell EMC Networking OS10 Enterprise\n\
         OS Version: 10.5.4.0\n\
         Build Version: 10.5.4.0.383",
    )
    .with_canned_reply(
        "show running-configuration",
        "! Version 10.5.4.0\n\
         ! Last configuration change at May 06 10:21:01 2024\n\
         !\n\
         hostname OS10\n\
         !\n\
         interface ethernet1/1/1\n \
         no shutdown\n \
         no switchport\n \
         ip address 10.0.0.1/24",
    )
    .with_canned_reply(
        "show interface status",
        "Port          Description  Duplex  Speed  Auto-Neg  Link-Status\n\
         Eth 1/1/1                  full    100G   off       up\n\
         Eth 1/1/2                  full    0      off       down",
    ))
}
