//! Virtual-device persona for the `paloalto_panos` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn paloalto_panos() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "paloalto_panos",
        templates::by_name_config("paloalto_panos")?,
        "enable",
        &[("enable", "admin@PA-3220>"), ("config", "admin@PA-3220#")],
    )
    .with_error_reply("Unknown command: make-error")
    .with_canned_reply(
        "show system info",
        "hostname: PA-3220\n\
         model: PA-3220\n\
         sw-version: 10.2.4\n\
         app-version: 8700-8000",
    )
    .with_canned_reply(
        "show config running",
        "config {\n  \
         devices {\n    \
         localhost.localdomain {\n      \
         deviceconfig {\n        \
         system {\n          \
         hostname PA-3220;\n        \
         }\n      \
         }\n    \
         }\n  \
         }\n\
         }",
    )
    .with_canned_reply(
        "show interface all",
        "total configured hardware interfaces: 2\n\
         name                    id    speed    mac address        state\n\
         ethernet1/1             16    1000     b4:0c:25:e0:00:10  up\n\
         ethernet1/2             17    1000     b4:0c:25:e0:00:11  down",
    ))
}
