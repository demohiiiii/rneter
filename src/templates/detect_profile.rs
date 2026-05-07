use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Declarative detect profile attached to one built-in template.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct TemplateDetectProfile {
    #[serde(default)]
    pub initial_rules: Vec<TemplateProbeRule>,
    #[serde(default)]
    pub probes: Vec<TemplateProbe>,
}

/// One probe command plus the rules used to score its output.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TemplateProbe {
    pub command: String,
    #[serde(default)]
    pub rules: Vec<TemplateProbeRule>,
    #[serde(default)]
    pub error_patterns: Vec<String>,
}

/// One weighted regex rule inside a detect profile.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TemplateProbeRule {
    pub pattern: String,
    pub weight: u32,
}
