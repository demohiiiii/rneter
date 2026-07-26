//! Virtual-device E2E test for the `venustech` template.

use crate::support;

#[tokio::test]
async fn venustech_full_scenario() {
    support::run_full_scenario("venustech").await;
}
