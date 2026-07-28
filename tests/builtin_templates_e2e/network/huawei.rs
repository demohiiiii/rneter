//! Virtual-device E2E test for the `huawei` template.

use crate::support;
use rneter::testkit::{DevicePersona, FakeSshDevice};

#[tokio::test]
async fn huawei_full_scenario() {
    support::run_full_scenario("huawei").await;
}

#[tokio::test]
async fn huawei_autodetected_from_virtual_device() {
    support::run_autodetect_scenario("huawei").await;
}

#[tokio::test]
async fn huawei_answers_save_confirmation() {
    support::run_confirmation_scenario("huawei", "Enable", "save", "y").await;
}

#[tokio::test]
async fn huawei_collects_all_paged_output() {
    support::run_pager_scenario("huawei", "Enable", "display paged-output", "---- More ----").await;
}

#[tokio::test]
async fn paged_reply_requires_at_least_two_pages() {
    let persona = DevicePersona::builtin("huawei")
        .expect("build huawei persona")
        .with_paged_reply("display paged-output", "---- More ----", ["only page"]);

    let error = match FakeSshDevice::spawn(persona).await {
        Ok(_) => panic!("single-page reply must be rejected"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("at least two pages"),
        "unexpected validation error: {error:?}"
    );
}

#[tokio::test]
async fn pager_prompt_must_match_template_more_regex() {
    let persona = DevicePersona::builtin("huawei")
        .expect("build huawei persona")
        .with_paged_reply("display paged-output", "--More--", ["first", "second"]);

    let error = match FakeSshDevice::spawn(persona).await {
        Ok(_) => panic!("mismatched pager prompt must be rejected"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("does not match its more_regex"),
        "unexpected validation error: {error:?}"
    );
}
