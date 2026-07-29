//! Virtual-device persona for the `leadsec_powerv` template.

use crate::error::ConnectError;
use crate::templates;
use crate::testkit::DevicePersona;

pub(crate) fn leadsec_powerv() -> Result<DevicePersona, ConnectError> {
    Ok(DevicePersona::for_config(
        "leadsec_powerv",
        templates::by_name_config("leadsec_powerv")?,
        "login",
        &[("login", "PowerV>")],
    )
    .with_error_reply("unknown keyword")
    .with_canned_reply(
        "show version",
        "LeadSec PowerV Security Gateway\n\
         Software version: PowerV 5.0\n\
         Device name: PowerV",
    ))
}
