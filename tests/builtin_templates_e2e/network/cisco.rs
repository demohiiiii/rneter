//! Virtual-device E2E tests for the `cisco_ios` and `cisco_xe` templates.

use crate::support;

#[tokio::test]
async fn cisco_ios_full_scenario() {
    support::run_full_scenario("cisco_ios").await;
}

#[tokio::test]
async fn cisco_xe_full_scenario() {
    support::run_full_scenario("cisco_xe").await;
}

#[tokio::test]
async fn cisco_ios_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("cisco_ios").await;
}

#[tokio::test]
async fn cisco_xe_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("cisco_xe").await;
}
