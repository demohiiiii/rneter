//! Virtual-device E2E test for the `arista_eos` template.

use crate::support;

#[tokio::test]
async fn arista_eos_full_scenario() {
    support::run_full_scenario("arista_eos").await;
}

#[tokio::test]
async fn arista_eos_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("arista_eos").await;
}
