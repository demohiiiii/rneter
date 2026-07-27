//! Virtual-device E2E test for the `fortinet` template.

use crate::support;

#[tokio::test]
async fn fortinet_full_scenario() {
    support::run_full_scenario("fortinet").await;
}

#[tokio::test]
async fn fortinet_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("fortinet").await;
}
