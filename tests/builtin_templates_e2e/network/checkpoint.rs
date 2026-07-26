//! Virtual-device E2E test for the `checkpoint_gaia` template.

use crate::support;

#[tokio::test]
async fn checkpoint_gaia_full_scenario() {
    support::run_full_scenario("checkpoint_gaia").await;
}
