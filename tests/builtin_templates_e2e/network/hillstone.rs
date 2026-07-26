//! Virtual-device E2E test for the `hillstone_stoneos` template.

use crate::support;

#[tokio::test]
async fn hillstone_stoneos_full_scenario() {
    support::run_full_scenario("hillstone_stoneos").await;
}
