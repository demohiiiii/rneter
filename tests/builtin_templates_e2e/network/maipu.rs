//! Virtual-device E2E test for the `maipu` template.

use crate::support;

#[tokio::test]
async fn maipu_full_scenario() {
    support::run_full_scenario("maipu").await;
}

#[tokio::test]
async fn maipu_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("maipu").await;
}
