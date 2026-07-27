//! Virtual-device E2E test for the `qianxin` template.

use crate::support;

#[tokio::test]
async fn qianxin_full_scenario() {
    support::run_full_scenario("qianxin").await;
}

#[tokio::test]
async fn qianxin_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("qianxin").await;
}
