//! Virtual-device E2E test for the `cisco_nxos` template.

use crate::support;

#[tokio::test]
async fn cisco_nxos_full_scenario() {
    support::run_full_scenario("cisco_nxos").await;
}
