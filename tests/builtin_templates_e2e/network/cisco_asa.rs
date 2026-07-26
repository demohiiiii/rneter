//! Virtual-device E2E test for the `cisco_asa` template.

use crate::support;

#[tokio::test]
async fn cisco_asa_full_scenario() {
    support::run_full_scenario("cisco_asa").await;
}
