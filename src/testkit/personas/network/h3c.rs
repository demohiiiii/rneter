//! Virtual-device personas for the `h3c_comware` and `hp_comware` templates.
//!
//! Both templates treat any line containing `%` (or `^`) as a device error,
//! so canned outputs must avoid those characters entirely.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn h3c_comware() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "h3c_comware",
        templates::by_name_config("h3c_comware")?,
        "enable",
        &[("enable", "<H3C>"), ("config", "[H3C]")],
    )
    .with_error_reply("testkit % forced failure")
    .with_canned_reply(
        "display version",
        "H3C Comware Software, Version 7.1.070, Release 6555P01\n\
         Copyright (c) 2004-2021 New H3C Technologies Co., Ltd. All rights reserved.\n\
         H3C S6520X-30QC-EI uptime is 21 weeks, 4 days",
    )
    .with_canned_reply(
        "display current-configuration",
        "#\n\
         sysname H3C\n\
         #\n\
         interface GigabitEthernet1/0/1\n \
         port link-mode bridge\n\
         #\n\
         ssh server enable\n\
         #\n\
         return",
    )
    .with_canned_reply(
        "display interface brief",
        "Brief information on interfaces in route mode:\n\
         Interface            Link Protocol Primary IP      Description\n\
         GE1/0/1              UP   UP       192.168.1.1\n\
         GE1/0/2              DOWN DOWN     --",
    ))
}

pub(crate) fn hp_comware() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "hp_comware",
        templates::by_name_config("hp_comware")?,
        "enable",
        &[("enable", "<HP>"), ("config", "[HP]")],
    )
    .with_error_reply("testkit % forced failure")
    .with_canned_reply(
        "display version",
        "HPE Comware Software, Version 7.1.045, Release 2432P06\n\
         Copyright (c) 2010-2017 Hewlett Packard Enterprise Development LP\n\
         HPE 5130-24G-4SFP+ EI Switch uptime is 12 weeks, 1 day",
    )
    .with_canned_reply(
        "display current-configuration",
        "#\n\
         sysname HP\n\
         #\n\
         interface GigabitEthernet1/0/1\n \
         port link-mode bridge\n\
         #\n\
         return",
    )
    .with_canned_reply(
        "display interface brief",
        "Brief information on interfaces in route mode:\n\
         Interface            Link Protocol Primary IP      Description\n\
         GE1/0/1              UP   UP       192.168.1.1\n\
         GE1/0/2              DOWN DOWN     --",
    ))
}
