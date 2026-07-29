//! Virtual-device E2E test for the `leadsec_powerv` template.

use crate::support;

#[tokio::test]
async fn leadsec_powerv_full_scenario() {
    support::run_full_scenario("leadsec_powerv").await;
}

#[tokio::test]
async fn leadsec_powerv_autodetect_does_not_claim_an_unrelated_high_confidence_match() {
    support::run_autodetect_scenario("leadsec_powerv").await;
}
