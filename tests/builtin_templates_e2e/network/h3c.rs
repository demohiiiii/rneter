//! Virtual-device E2E tests for the `h3c_comware` and `hp_comware` templates.

use crate::support;

#[tokio::test]
async fn h3c_comware_full_scenario() {
    support::run_full_scenario("h3c_comware").await;
}

#[tokio::test]
async fn hp_comware_full_scenario() {
    support::run_full_scenario("hp_comware").await;
}

#[tokio::test]
async fn h3c_comware_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("h3c_comware").await;
}

#[tokio::test]
async fn hp_comware_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("hp_comware").await;
}

#[tokio::test]
async fn h3c_comware_collects_all_paged_output() {
    support::run_pager_scenario(
        "h3c_comware",
        "Enable",
        "display paged-output",
        "---- More ----",
    )
    .await;
}
