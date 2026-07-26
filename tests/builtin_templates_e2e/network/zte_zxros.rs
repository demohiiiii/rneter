//! Virtual-device E2E test for the `zte_zxros` template.

use crate::support;

#[tokio::test]
async fn zte_zxros_full_scenario() {
    support::run_full_scenario("zte_zxros").await;
}
