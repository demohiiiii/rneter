//! Virtual-device E2E test for the `dptech` template.

use crate::support;

#[tokio::test]
async fn dptech_full_scenario() {
    support::run_full_scenario("dptech").await;
}
