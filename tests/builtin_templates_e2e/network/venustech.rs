//! Virtual-device E2E test for the `venustech` template.

use crate::support;

#[tokio::test]
async fn venustech_full_scenario() {
    support::run_full_scenario("venustech").await;
}

#[tokio::test]
async fn venustech_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("venustech").await;
}
