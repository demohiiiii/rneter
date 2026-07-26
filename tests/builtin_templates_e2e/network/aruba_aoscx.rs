//! Virtual-device E2E test for the `aruba_aoscx` template.

use crate::support;

#[tokio::test]
async fn aruba_aoscx_full_scenario() {
    support::run_full_scenario("aruba_aoscx").await;
}
