//! Virtual-device E2E test for the `chaitin` template.

use crate::support;

#[tokio::test]
async fn chaitin_full_scenario() {
    support::run_full_scenario("chaitin").await;
}

#[tokio::test]
async fn chaitin_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("chaitin").await;
}
