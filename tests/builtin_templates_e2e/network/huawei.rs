//! Virtual-device E2E test for the `huawei` template.

use crate::support;

#[tokio::test]
async fn huawei_full_scenario() {
    support::run_full_scenario("huawei").await;
}

#[tokio::test]
async fn huawei_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("huawei").await;
}

#[tokio::test]
async fn huawei_answers_save_confirmation() {
    support::run_confirmation_scenario("huawei", "Enable", "save", "y").await;
}
