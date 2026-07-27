//! Virtual-device E2E test for the `aruba_aoscx` template.

use crate::support;

#[tokio::test]
async fn aruba_aoscx_full_scenario() {
    support::run_full_scenario("aruba_aoscx").await;
}

#[tokio::test]
async fn aruba_aoscx_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("aruba_aoscx").await;
}
