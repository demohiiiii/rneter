//! Virtual-device E2E test for the `ruijie_os` template.

use crate::support;

#[tokio::test]
async fn ruijie_os_full_scenario() {
    support::run_full_scenario("ruijie_os").await;
}
