//! Virtual-device E2E test for the `juniper_junos` template.

use crate::support;

#[tokio::test]
async fn juniper_junos_full_scenario() {
    support::run_full_scenario("juniper_junos").await;
}

#[tokio::test]
async fn juniper_junos_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("juniper_junos").await;
}

#[tokio::test]
async fn juniper_answers_uncommitted_changes_confirmation() {
    support::run_confirmation_scenario("juniper_junos", "Config", "exit", "yes").await;
}
