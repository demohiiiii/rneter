//! Virtual-device E2E test for the `checkpoint_gaia` template.

use crate::support;

#[tokio::test]
async fn checkpoint_gaia_full_scenario() {
    support::run_full_scenario("checkpoint_gaia").await;
}

#[tokio::test]
async fn checkpoint_gaia_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("checkpoint_gaia").await;
}
