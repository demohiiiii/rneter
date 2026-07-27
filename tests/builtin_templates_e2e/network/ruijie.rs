//! Virtual-device E2E test for the `ruijie_os` template.

use crate::support;

#[tokio::test]
async fn ruijie_os_full_scenario() {
    support::run_full_scenario("ruijie_os").await;
}

#[tokio::test]
async fn ruijie_os_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("ruijie_os").await;
}
