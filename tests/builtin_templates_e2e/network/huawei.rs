//! Virtual-device E2E test for the `huawei` template.

use crate::support;

#[tokio::test]
async fn huawei_full_scenario() {
    support::run_full_scenario("huawei").await;
}
