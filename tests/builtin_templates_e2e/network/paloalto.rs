//! Virtual-device E2E test for the `paloalto_panos` template.

use crate::support;

#[tokio::test]
async fn paloalto_panos_full_scenario() {
    support::run_full_scenario("paloalto_panos").await;
}

#[tokio::test]
async fn paloalto_panos_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("paloalto_panos").await;
}
