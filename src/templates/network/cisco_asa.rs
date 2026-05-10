use super::cisco::cisco_config;
use crate::device::{DeviceHandler, DeviceHandlerConfig};
use crate::error::ConnectError;

pub fn cisco_asa_config() -> DeviceHandlerConfig {
    cisco_config()
}

pub fn cisco_asa() -> Result<DeviceHandler, ConnectError> {
    cisco_asa_config().build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cisco_asa_reuses_cisco_config() {
        assert_eq!(cisco_asa_config(), cisco_config());
    }
}
