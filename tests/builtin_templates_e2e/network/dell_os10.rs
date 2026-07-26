//! Virtual-device E2E test for the `dell_os10` template.

use crate::support;

#[tokio::test]
async fn dell_os10_full_scenario() {
    support::run_full_scenario("dell_os10").await;
}
