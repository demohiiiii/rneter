//! Virtual-device E2E test for the `topsec` template.

use crate::support;

#[tokio::test]
async fn topsec_full_scenario() {
    support::run_full_scenario("topsec").await;
}

#[tokio::test]
async fn topsec_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("topsec").await;
}
