//! Virtual-device E2E test for the `juniper_junos` template.

use crate::support;

#[tokio::test]
async fn juniper_junos_full_scenario() {
    support::run_full_scenario("juniper_junos").await;
}
