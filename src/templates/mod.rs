//! Predefined device templates.
//!
//! Concrete vendor implementations live in submodules. This root module keeps
//! the public exports stable while the implementation is split by concern.

mod catalog;
mod linux;
mod network;
mod registry;
mod transaction;

pub use catalog::{
    BUILTIN_TEMPLATES, TemplateCapability, TemplateMetadata, available_templates, template_catalog,
    template_metadata,
};
pub use linux::{
    CustomPrompts, LinuxCommandType, LinuxTemplateConfig, SudoMode, classify_linux_command, linux,
    linux_with_config,
};
pub use network::{
    arista, array, chaitin, checkpoint, cisco, dptech, fortinet, h3c, hillstone, huawei, juniper,
    maipu, paloalto, qianxin, topsec, venustech,
};
pub use registry::{
    by_name, diagnose_all_templates_json, diagnose_template, diagnose_template_json,
};
pub use transaction::{build_tx_block, classify_command};
