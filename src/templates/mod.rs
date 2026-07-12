//! Predefined device templates.
//!
//! Concrete vendor implementations live in submodules. This root module keeps
//! the public exports stable while the implementation is split by concern.

mod catalog;
mod command_flow_template;
mod detect;
mod detect_profile;
mod linux;
mod network;
mod registry;
mod transaction;
mod transfer;

pub use catalog::{
    BUILTIN_TEMPLATES, TemplateCapability, TemplateMetadata, available_templates, template_catalog,
    template_metadata,
};
pub use command_flow_template::{
    CommandFlowTemplate, CommandFlowTemplatePrompt, CommandFlowTemplateRuntime,
    CommandFlowTemplateStep, CommandFlowTemplateText, CommandFlowTemplateVar,
    CommandFlowTemplateVarKind,
};
pub(crate) use detect::summarize_detect_log_text;
pub use detect::{
    AutodetectedConnection, DetectConfidence, DetectConnectPolicy, DetectFactKind,
    DetectFactSource, DetectSnapshot, DetectTemplateDefinition, TemplateDetectCandidate,
    TemplateDetectFact, TemplateDetectReport,
    autodetect_and_connect_with_builtin_and_templates_and_context,
    autodetect_and_connect_with_context, autodetect_and_connect_with_templates_and_context,
    autodetect_with_builtin_and_templates_and_context, autodetect_with_context,
    autodetect_with_profiles_and_context, autodetect_with_templates_and_context,
    builtin_detect_template_definitions, collect_detect_snapshot_with_context,
    merge_with_builtin_detect_templates, score_builtin_templates, score_detect_profiles,
};
pub use detect_profile::{TemplateDetectProfile, TemplateProbe, TemplateProbeRule};
pub use linux::{linux, linux_handler_config};
pub use network::{
    arista, arista_config, array, array_config, aruba_aoscx, aruba_aoscx_config, chaitin,
    chaitin_config, checkpoint, checkpoint_config, cisco, cisco_asa, cisco_asa_config,
    cisco_config, cisco_nxos, cisco_nxos_config, dell_os10, dell_os10_config, dptech,
    dptech_config, fortinet, fortinet_config, h3c, h3c_config, hillstone, hillstone_config, huawei,
    huawei_config, juniper, juniper_config, maipu, maipu_config, paloalto, paloalto_config,
    qianxin, qianxin_config, ruijie, ruijie_config, topsec, topsec_config, venustech,
    venustech_config, zte_zxros, zte_zxros_config,
};
pub use registry::{
    available_detect_profiles, by_name, by_name_config, detect_profile_by_name,
    diagnose_all_templates_json, diagnose_template, diagnose_template_json,
};
pub use transaction::build_tx_block;
pub use transfer::cisco_like_copy_template;
