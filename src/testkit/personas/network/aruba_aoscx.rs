//! Virtual-device persona for the `aruba_aoscx` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

use super::cisco_like;

pub(crate) fn aruba_aoscx() -> Result<DevicePersona, ConnectError> {
    Ok(cisco_like(
        templates::by_name_config("aruba_aoscx")?,
        "aruba_aoscx",
        "switch",
        "% Invalid input",
        "Password: ",
    )
    .with_canned_reply(
        "show version",
        "ArubaOS-CX\n\
         (c) Copyright 2017-2023 Hewlett Packard Enterprise Development LP\n\
         Version      : FL.10.10.1040\n\
         Build Date   : 2023-01-20",
    )
    .with_canned_reply(
        "show running-config",
        "Current configuration:\n\
         !\n\
         !Version ArubaOS-CX FL.10.10.1040\n\
         hostname switch\n\
         !\n\
         interface 1/1/1\n    \
         no shutdown\n    \
         ip address 192.168.1.1/24",
    )
    .with_canned_reply(
        "show interface brief",
        "Port       Native  Mode   Type          Enabled Status  Reason   Speed\n\
         1/1/1      1       access 1GbT          yes     up               1000\n\
         1/1/2      1       access 1GbT          yes     down    Waiting  auto",
    ))
}
