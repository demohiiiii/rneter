//! Virtual-device E2E test for the `fortinet` template.

use crate::support;

#[tokio::test]
async fn fortinet_full_scenario() {
    support::run_full_scenario("fortinet").await;
}
