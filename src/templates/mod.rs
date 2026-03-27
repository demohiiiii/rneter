//! Predefined device templates.
//!
//! Concrete vendor implementations live in submodules. This root module keeps
//! the public exports stable while the implementation is split by concern.

mod catalog;
mod command_flow_template;
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
pub use linux::{
    CustomPrompts, LinuxCommandType, LinuxTemplateConfig, SudoMode, classify_linux_command, linux,
    linux_handler_config, linux_with_config,
};
pub use network::{
    arista, arista_config, array, array_config, chaitin, chaitin_config, checkpoint,
    checkpoint_config, cisco, cisco_config, dptech, dptech_config, fortinet, fortinet_config, h3c,
    h3c_config, hillstone, hillstone_config, huawei, huawei_config, juniper, juniper_config, maipu,
    maipu_config, paloalto, paloalto_config, qianxin, qianxin_config, topsec, topsec_config,
    venustech, venustech_config,
};
pub use registry::{
    by_name, by_name_config, diagnose_all_templates_json, diagnose_template, diagnose_template_json,
};
pub use transaction::{build_tx_block, classify_command};
pub use transfer::cisco_like_copy_template;
