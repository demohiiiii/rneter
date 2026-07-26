//! Virtual-device persona for the `linux` template (shell exit-status
//! execution model).

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::{DEFAULT_ENABLE_PASSWORD, DevicePersona};

pub(crate) fn linux() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "linux",
        templates::by_name_config("linux")?,
        "user",
        &[("user", "admin@debian:~$"), ("root", "root@debian:~#")],
    )
    .with_challenge(
        "sudo -i",
        "[sudo] password for admin: ",
        DEFAULT_ENABLE_PASSWORD,
    )
    .with_error_reply("testkit forced failure")
    .with_enable_password(DEFAULT_ENABLE_PASSWORD)
    .with_canned_reply(
        "uname -a",
        "Linux debian 6.1.0-13-amd64 #1 SMP PREEMPT_DYNAMIC Debian 6.1.55-1 (2023-09-29) x86_64 GNU/Linux",
    )
    .with_canned_reply(
        "ip -brief address",
        "lo               UNKNOWN        127.0.0.1/8 ::1/128\n\
         eth0             UP             192.168.1.10/24 fe80::20c:29ff:fe11:2233/64",
    )
    .with_canned_reply(
        "cat /etc/os-release",
        "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\n\
         NAME=\"Debian GNU/Linux\"\n\
         VERSION_ID=\"12\"\n\
         VERSION=\"12 (bookworm)\"\n\
         ID=debian",
    ))
}
