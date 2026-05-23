use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

use log::{debug, trace};
use regex::Regex;

use super::catalog::BUILTIN_TEMPLATES;
use super::detect_profile::TemplateDetectProfile;
use super::{by_name_config, detect_profile_by_name};
use crate::device::DeviceHandlerConfig;
use crate::error::ConnectError;
use crate::session::{CmdJob, ConnectionRequest, DetectRequest, ExecutionContext, MANAGER};

/// Confidence bucket derived from the total autodetect score.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectConfidence {
    High,
    Medium,
    Low,
}

impl DetectConfidence {
    pub fn from_score(score: u32) -> Self {
        if score >= 90 {
            Self::High
        } else if score >= 50 {
            Self::Medium
        } else {
            Self::Low
        }
    }

    pub fn satisfies_minimum(self, minimum: Self) -> bool {
        self.rank() >= minimum.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
        }
    }
}

/// Policy controlling whether autodetect may continue into a live connection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DetectConnectPolicy {
    pub minimum_confidence: DetectConfidence,
}

impl Default for DetectConnectPolicy {
    fn default() -> Self {
        Self {
            minimum_confidence: DetectConfidence::Medium,
        }
    }
}

/// Origin of one matched autodetect fact.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectFactSource {
    InitialPrompt,
    ProbeOutput,
}

/// Meaning of one autodetect fact inside scoring or diagnostics.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectFactKind {
    PositiveMatch,
    ErrorPattern,
}

/// One scoring fact collected during template autodetection.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TemplateDetectFact {
    pub kind: DetectFactKind,
    pub source: DetectFactSource,
    pub command: String,
    pub pattern: String,
    pub sample: String,
    pub weight: u32,
}

/// One ranked candidate in an autodetect report.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TemplateDetectCandidate {
    pub template_name: String,
    pub score: u32,
    pub confidence: DetectConfidence,
    #[serde(default)]
    pub matched_facts: Vec<TemplateDetectFact>,
}

impl TemplateDetectCandidate {
    pub fn new(
        template_name: impl Into<String>,
        score: u32,
        matched_facts: Vec<TemplateDetectFact>,
    ) -> Self {
        Self {
            template_name: template_name.into(),
            score,
            confidence: DetectConfidence::from_score(score),
            matched_facts,
        }
    }
}

/// Ranked autodetect result including all candidates and raw matched facts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct TemplateDetectReport {
    pub best_match: Option<TemplateDetectCandidate>,
    #[serde(default)]
    pub candidates: Vec<TemplateDetectCandidate>,
    #[serde(default)]
    pub raw_facts: Vec<TemplateDetectFact>,
}

/// Result of a successful autodetect-then-connect flow.
pub struct AutodetectedConnection {
    pub template_name: String,
    pub report: TemplateDetectReport,
    pub sender: mpsc::Sender<CmdJob>,
}

impl AutodetectedConnection {
    pub fn new(
        sender: mpsc::Sender<CmdJob>,
        report: TemplateDetectReport,
    ) -> Result<Self, ConnectError> {
        let template_name = report
            .best_match
            .as_ref()
            .map(|candidate| candidate.template_name.clone())
            .ok_or_else(|| {
                ConnectError::AutodetectNoMatch("report contained no best_match".to_string())
            })?;

        Ok(Self {
            template_name,
            report,
            sender,
        })
    }
}

/// One autodetect-capable template definition supplied by the caller.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DetectTemplateDefinition {
    pub template_name: String,
    pub handler_config: DeviceHandlerConfig,
    pub detect_profile: TemplateDetectProfile,
}

impl DetectTemplateDefinition {
    /// Build one autodetect-capable template definition from a handler config and detect profile.
    pub fn new(
        template_name: impl Into<String>,
        handler_config: DeviceHandlerConfig,
        detect_profile: TemplateDetectProfile,
    ) -> Self {
        Self {
            template_name: template_name.into(),
            handler_config,
            detect_profile,
        }
    }
}

impl TemplateDetectReport {
    pub fn from_candidates(candidates: Vec<TemplateDetectCandidate>) -> Self {
        let raw_facts = candidates
            .iter()
            .flat_map(|candidate| candidate.matched_facts.iter().cloned())
            .collect();
        Self::from_parts(candidates, raw_facts)
    }

    pub fn from_parts(
        mut candidates: Vec<TemplateDetectCandidate>,
        raw_facts: Vec<TemplateDetectFact>,
    ) -> Self {
        candidates.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.template_name.cmp(&right.template_name))
        });
        let best_match = candidates.first().cloned();

        Self {
            best_match,
            candidates,
            raw_facts,
        }
    }
}

/// Raw detect snapshot collected before scoring profiles.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
pub struct DetectSnapshot {
    pub initial_output: String,
    pub initial_prompt: String,
    #[serde(default)]
    pub probe_outputs: HashMap<String, String>,
}

pub(crate) fn summarize_detect_log_text(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }

    collapsed
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end()
        .to_string()
        + "..."
}

fn summarize_detect_candidates(candidates: &[TemplateDetectCandidate]) -> String {
    if candidates.is_empty() {
        return "none".to_string();
    }

    candidates
        .iter()
        .map(|candidate| {
            format!(
                "{}:{}({})",
                candidate.template_name,
                candidate.score,
                match candidate.confidence {
                    DetectConfidence::High => "high",
                    DetectConfidence::Medium => "medium",
                    DetectConfidence::Low => "low",
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Score one set of detect profiles against a collected snapshot.
pub fn score_detect_profiles(
    snapshot: &DetectSnapshot,
    profiles: Vec<(String, TemplateDetectProfile)>,
) -> TemplateDetectReport {
    let mut candidates = Vec::new();
    let mut raw_facts = Vec::new();

    debug!(
        "autodetect scoring {} templates against prompt='{}' with {} probe outputs",
        profiles.len(),
        summarize_detect_log_text(&snapshot.initial_prompt, 80),
        snapshot.probe_outputs.len()
    );

    for (template_name, profile) in profiles {
        let mut facts = Vec::new();
        trace!(
            "autodetect scoring template='{}' initial_rules={} probes={}",
            template_name,
            profile.initial_rules.len(),
            profile.probes.len()
        );

        for rule in &profile.initial_rules {
            if regex_matches(&rule.pattern, &snapshot.initial_prompt)
                || regex_matches(&rule.pattern, &snapshot.initial_output)
            {
                trace!(
                    "autodetect initial rule matched template='{}' pattern='{}' weight={} sample='{}'",
                    template_name,
                    rule.pattern,
                    rule.weight,
                    summarize_detect_log_text(
                        if !snapshot.initial_prompt.is_empty() {
                            &snapshot.initial_prompt
                        } else {
                            &snapshot.initial_output
                        },
                        120
                    )
                );
                facts.push(TemplateDetectFact {
                    kind: DetectFactKind::PositiveMatch,
                    source: DetectFactSource::InitialPrompt,
                    command: "__initial__".to_string(),
                    pattern: rule.pattern.clone(),
                    sample: if !snapshot.initial_prompt.is_empty() {
                        snapshot.initial_prompt.clone()
                    } else {
                        snapshot.initial_output.clone()
                    },
                    weight: rule.weight,
                });
            }
        }

        for probe in &profile.probes {
            let Some(output) = snapshot.probe_outputs.get(&probe.command) else {
                trace!(
                    "autodetect probe output missing template='{}' command='{}'",
                    template_name, probe.command
                );
                continue;
            };

            let matched_error_pattern = default_probe_error_patterns()
                .iter()
                .copied()
                .chain(probe.error_patterns.iter().map(String::as_str))
                .find(|pattern| regex_matches(pattern, output));
            if let Some(pattern) = matched_error_pattern {
                trace!(
                    "autodetect probe error matched template='{}' command='{}' pattern='{}' output='{}'",
                    template_name,
                    probe.command,
                    pattern,
                    summarize_detect_log_text(output, 120)
                );
                facts.push(TemplateDetectFact {
                    kind: DetectFactKind::ErrorPattern,
                    source: DetectFactSource::ProbeOutput,
                    command: probe.command.clone(),
                    pattern: pattern.to_string(),
                    sample: output.clone(),
                    weight: 0,
                });
                continue;
            }

            for rule in &probe.rules {
                if regex_matches(&rule.pattern, output) {
                    trace!(
                        "autodetect probe rule matched template='{}' command='{}' pattern='{}' weight={} output='{}'",
                        template_name,
                        probe.command,
                        rule.pattern,
                        rule.weight,
                        summarize_detect_log_text(output, 120)
                    );
                    facts.push(TemplateDetectFact {
                        kind: DetectFactKind::PositiveMatch,
                        source: DetectFactSource::ProbeOutput,
                        command: probe.command.clone(),
                        pattern: rule.pattern.clone(),
                        sample: output.clone(),
                        weight: rule.weight,
                    });
                }
            }
        }

        raw_facts.extend(facts.iter().cloned());

        let score: u32 = facts
            .iter()
            .filter(|fact| fact.kind == DetectFactKind::PositiveMatch)
            .map(|fact| fact.weight)
            .sum();
        trace!(
            "autodetect template='{}' score={} facts={}",
            template_name,
            score,
            facts.len()
        );
        if score > 0 {
            candidates.push(TemplateDetectCandidate::new(template_name, score, facts));
        }
    }

    let report = TemplateDetectReport::from_parts(candidates, raw_facts);
    debug!(
        "autodetect scoring completed: best_match={} candidates=[{}] raw_facts={}",
        report
            .best_match
            .as_ref()
            .map(|candidate| format!(
                "{}:{}({:?})",
                candidate.template_name, candidate.score, candidate.confidence
            ))
            .unwrap_or_else(|| "none".to_string()),
        summarize_detect_candidates(&report.candidates),
        report.raw_facts.len()
    );
    report
}

/// Score the built-in detect profiles against one collected snapshot.
pub fn score_builtin_templates(snapshot: &DetectSnapshot) -> TemplateDetectReport {
    score_detect_profiles(snapshot, builtin_detect_profiles())
}

/// Collect one raw detect snapshot with caller-supplied probe commands.
pub async fn collect_detect_snapshot_with_context(
    request: DetectRequest,
    context: ExecutionContext,
    probe_commands: Vec<String>,
) -> Result<DetectSnapshot, ConnectError> {
    let probe_commands = probe_commands
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    debug!(
        "autodetect snapshot collection start target={} port={} probes={}",
        request.addr,
        request.port,
        probe_commands.len()
    );

    let snapshot = MANAGER
        .collect_detect_snapshot(&request, &context, &probe_commands)
        .await?;

    debug!(
        "autodetect snapshot collected prompt='{}' initial_output='{}' probes={}",
        summarize_detect_log_text(&snapshot.initial_prompt, 80),
        summarize_detect_log_text(&snapshot.initial_output, 120),
        snapshot.probe_outputs.len()
    );

    Ok(snapshot)
}

/// Run SSH-based autodetect against caller-supplied detect profiles.
pub async fn autodetect_with_profiles_and_context(
    request: DetectRequest,
    context: ExecutionContext,
    profiles: Vec<(String, TemplateDetectProfile)>,
) -> Result<TemplateDetectReport, ConnectError> {
    let probe_commands = collect_probe_commands(&profiles);

    debug!(
        "autodetect start target={} port={} templates={} probes={}",
        request.addr,
        request.port,
        profiles.len(),
        probe_commands.len()
    );

    let snapshot = collect_detect_snapshot_with_context(request, context, probe_commands).await?;

    Ok(score_detect_profiles(&snapshot, profiles))
}

/// Run SSH-based autodetect against caller-supplied template definitions.
pub async fn autodetect_with_templates_and_context(
    request: DetectRequest,
    context: ExecutionContext,
    templates: Vec<DetectTemplateDefinition>,
) -> Result<TemplateDetectReport, ConnectError> {
    autodetect_with_profiles_and_context(request, context, profiles_from_templates(&templates))
        .await
}

/// Run SSH-based autodetect against built-in templates plus caller-supplied definitions.
///
/// Caller-supplied templates override built-in definitions when `template_name`
/// matches case-insensitively.
pub async fn autodetect_with_builtin_and_templates_and_context(
    request: DetectRequest,
    context: ExecutionContext,
    templates: Vec<DetectTemplateDefinition>,
) -> Result<TemplateDetectReport, ConnectError> {
    autodetect_with_templates_and_context(
        request,
        context,
        merge_with_builtin_detect_templates(templates),
    )
    .await
}

/// Run SSH-based autodetect and return ranked built-in template candidates.
pub async fn autodetect_with_context(
    request: DetectRequest,
    context: ExecutionContext,
) -> Result<TemplateDetectReport, ConnectError> {
    autodetect_with_profiles_and_context(request, context, builtin_detect_profiles()).await
}

/// Run autodetect against caller-supplied templates, enforce a confidence policy,
/// then connect using the winning handler config.
pub async fn autodetect_and_connect_with_templates_and_context(
    request: DetectRequest,
    enable_password: Option<String>,
    context: ExecutionContext,
    policy: DetectConnectPolicy,
    templates: Vec<DetectTemplateDefinition>,
) -> Result<AutodetectedConnection, ConnectError> {
    let report =
        autodetect_with_templates_and_context(request.clone(), context.clone(), templates.clone())
            .await?;
    let best = select_best_detected_template(&report, policy)?;
    debug!(
        "autodetect selected template='{}' score={} confidence={:?}; connecting",
        best.template_name, best.score, best.confidence
    );
    let connection_request = build_detected_connection_request_from_templates(
        request,
        enable_password,
        &best,
        &templates,
    )?;
    let sender = MANAGER
        .get_with_context(connection_request, context)
        .await?;

    AutodetectedConnection::new(sender, report)
}

/// Run autodetect against built-in templates plus caller-supplied definitions,
/// then connect using the winning handler config.
///
/// Caller-supplied templates override built-in definitions when `template_name`
/// matches case-insensitively.
pub async fn autodetect_and_connect_with_builtin_and_templates_and_context(
    request: DetectRequest,
    enable_password: Option<String>,
    context: ExecutionContext,
    policy: DetectConnectPolicy,
    templates: Vec<DetectTemplateDefinition>,
) -> Result<AutodetectedConnection, ConnectError> {
    autodetect_and_connect_with_templates_and_context(
        request,
        enable_password,
        context,
        policy,
        merge_with_builtin_detect_templates(templates),
    )
    .await
}

/// Run autodetect, enforce a minimum confidence policy, then connect using the best template.
pub async fn autodetect_and_connect_with_context(
    request: DetectRequest,
    enable_password: Option<String>,
    context: ExecutionContext,
    policy: DetectConnectPolicy,
) -> Result<AutodetectedConnection, ConnectError> {
    autodetect_and_connect_with_templates_and_context(
        request,
        enable_password,
        context,
        policy,
        builtin_detect_templates(),
    )
    .await
}

fn select_best_detected_template(
    report: &TemplateDetectReport,
    policy: DetectConnectPolicy,
) -> Result<TemplateDetectCandidate, ConnectError> {
    let best = report.best_match.clone().ok_or_else(|| {
        ConnectError::AutodetectNoMatch(format!(
            "device produced no scored candidates; facts={}",
            report.raw_facts.len()
        ))
    })?;

    if best.confidence.satisfies_minimum(policy.minimum_confidence) {
        debug!(
            "autodetect confidence accepted template='{}' confidence={:?} minimum={:?}",
            best.template_name, best.confidence, policy.minimum_confidence
        );
        Ok(best)
    } else {
        debug!(
            "autodetect confidence rejected template='{}' confidence={:?} minimum={:?}",
            best.template_name, best.confidence, policy.minimum_confidence
        );
        Err(ConnectError::AutodetectConfidenceTooLow(format!(
            "best_match={} confidence={:?} score={} minimum={:?}",
            best.template_name, best.confidence, best.score, policy.minimum_confidence
        )))
    }
}

fn build_detected_connection_request_from_templates(
    request: DetectRequest,
    enable_password: Option<String>,
    best: &TemplateDetectCandidate,
    templates: &[DetectTemplateDefinition],
) -> Result<ConnectionRequest, ConnectError> {
    let template = templates
        .iter()
        .find(|template| {
            template
                .template_name
                .eq_ignore_ascii_case(&best.template_name)
        })
        .ok_or_else(|| ConnectError::TemplateNotFound(best.template_name.clone()))?;
    let handler = template.handler_config.build()?;

    Ok(ConnectionRequest::new(
        request.user,
        request.addr,
        request.port,
        request.password,
        enable_password,
        handler,
    ))
}

fn builtin_detect_profiles() -> Vec<(String, TemplateDetectProfile)> {
    builtin_detect_templates()
        .into_iter()
        .map(|template| (template.template_name, template.detect_profile))
        .collect()
}

/// Return all built-in autodetect-capable template definitions.
pub fn builtin_detect_template_definitions() -> Vec<DetectTemplateDefinition> {
    BUILTIN_TEMPLATES
        .iter()
        .filter_map(|name| {
            let profile = detect_profile_by_name(name)?;
            let config = by_name_config(name).ok()?;
            Some(DetectTemplateDefinition::new(*name, config, profile))
        })
        .collect()
}

/// Merge caller-supplied autodetect templates onto the built-in set.
///
/// Caller-supplied templates override built-in definitions when `template_name`
/// matches case-insensitively; otherwise they are appended.
pub fn merge_with_builtin_detect_templates(
    templates: Vec<DetectTemplateDefinition>,
) -> Vec<DetectTemplateDefinition> {
    let mut merged = builtin_detect_template_definitions();

    for template in templates {
        if let Some(index) = merged.iter().position(|builtin| {
            builtin
                .template_name
                .eq_ignore_ascii_case(&template.template_name)
        }) {
            merged[index] = template;
        } else {
            merged.push(template);
        }
    }

    merged
}

fn builtin_detect_templates() -> Vec<DetectTemplateDefinition> {
    builtin_detect_template_definitions()
}

fn profiles_from_templates(
    templates: &[DetectTemplateDefinition],
) -> Vec<(String, TemplateDetectProfile)> {
    templates
        .iter()
        .map(|template| {
            (
                template.template_name.clone(),
                template.detect_profile.clone(),
            )
        })
        .collect()
}

fn collect_probe_commands(profiles: &[(String, TemplateDetectProfile)]) -> Vec<String> {
    profiles
        .iter()
        .flat_map(|(_, profile)| profile.probes.iter().map(|probe| probe.command.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn regex_matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern)
        .map(|regex| regex.is_match(text))
        .unwrap_or(false)
}

fn default_probe_error_patterns() -> &'static [&'static str] {
    &[
        r"% Invalid input detected",
        r"syntax error, expecting",
        r"Error: Unrecognized command",
        r"%Error",
        r"command not found",
        r"Syntax Error: unexpected argument",
        r"% Unrecognized command found at",
        r"% Unknown command, the error locates at",
        r"Invalid input",
        r"Unknown command",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::prompt_rule;
    use crate::templates::{TemplateDetectProfile, TemplateProbe, TemplateProbeRule};

    #[test]
    fn log_text_summary_collapses_whitespace_and_truncates() {
        let summary = summarize_detect_log_text("  line one\nline   two\tline three  ", 18);

        assert_eq!(summary, "line one line two...");
    }

    #[test]
    fn candidate_log_summary_lists_ranked_candidates() {
        let summary = summarize_detect_candidates(&[
            TemplateDetectCandidate::new("cisco", 95, Vec::new()),
            TemplateDetectCandidate::new("linux", 20, Vec::new()),
        ]);

        assert_eq!(summary, "cisco:95(high), linux:20(low)");
    }

    #[test]
    fn detect_confidence_maps_expected_score_ranges() {
        assert_eq!(DetectConfidence::from_score(95), DetectConfidence::High);
        assert_eq!(DetectConfidence::from_score(60), DetectConfidence::Medium);
        assert_eq!(DetectConfidence::from_score(10), DetectConfidence::Low);
    }

    #[test]
    fn detect_report_picks_highest_scored_candidate_as_best_match() {
        let report = TemplateDetectReport::from_candidates(vec![
            TemplateDetectCandidate::new("linux", 20, Vec::new()),
            TemplateDetectCandidate::new("cisco", 90, Vec::new()),
        ]);

        assert_eq!(
            report
                .best_match
                .as_ref()
                .map(|candidate| candidate.template_name.as_str()),
            Some("cisco")
        );
    }

    #[test]
    fn scoring_ranks_higher_weight_template_first() {
        let snapshot = DetectSnapshot {
            initial_output: "router>".to_string(),
            initial_prompt: "router>".to_string(),
            probe_outputs: HashMap::from([(
                "show version".to_string(),
                "Cisco IOS XE Software".to_string(),
            )]),
        };

        let report = score_detect_profiles(
            &snapshot,
            vec![
                (
                    "cisco".to_string(),
                    TemplateDetectProfile {
                        initial_rules: vec![TemplateProbeRule {
                            pattern: r"router>".to_string(),
                            weight: 10,
                        }],
                        probes: vec![TemplateProbe {
                            command: "show version".to_string(),
                            rules: vec![TemplateProbeRule {
                                pattern: r"Cisco IOS XE Software".to_string(),
                                weight: 90,
                            }],
                            error_patterns: Vec::new(),
                        }],
                    },
                ),
                (
                    "linux".to_string(),
                    TemplateDetectProfile {
                        initial_rules: Vec::new(),
                        probes: vec![TemplateProbe {
                            command: "show version".to_string(),
                            rules: vec![TemplateProbeRule {
                                pattern: r"Linux".to_string(),
                                weight: 10,
                            }],
                            error_patterns: Vec::new(),
                        }],
                    },
                ),
            ],
        );

        assert_eq!(
            report
                .best_match
                .as_ref()
                .map(|candidate| candidate.template_name.as_str()),
            Some("cisco")
        );
        assert_eq!(report.candidates.len(), 1);
    }

    #[test]
    fn hillstone_show_version_banner_scores_hillstone_first() {
        let snapshot = DetectSnapshot {
            initial_output: "SG-6000#".to_string(),
            initial_prompt: "SG-6000#".to_string(),
            probe_outputs: HashMap::from([(
                "show version".to_string(),
                "Hillstone Networks StoneOS software Version 5.5R1".to_string(),
            )]),
        };

        let report = score_builtin_templates(&snapshot);

        assert_eq!(
            report
                .best_match
                .as_ref()
                .map(|candidate| candidate.template_name.as_str()),
            Some("hillstone")
        );
    }

    #[test]
    fn detect_report_returns_none_when_no_profile_matches() {
        let snapshot = DetectSnapshot {
            initial_output: "unknown prompt".to_string(),
            initial_prompt: "unknown prompt".to_string(),
            probe_outputs: HashMap::new(),
        };

        let report = score_builtin_templates(&snapshot);
        assert!(report.best_match.is_none());
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn detect_connect_policy_defaults_to_medium() {
        assert_eq!(
            DetectConnectPolicy::default().minimum_confidence,
            DetectConfidence::Medium
        );
    }

    #[test]
    fn select_best_detected_template_rejects_missing_match() {
        let report = TemplateDetectReport::from_candidates(Vec::new());

        let err = select_best_detected_template(&report, DetectConnectPolicy::default())
            .expect_err("missing match should fail");

        assert!(matches!(err, ConnectError::AutodetectNoMatch(_)));
    }

    #[test]
    fn select_best_detected_template_rejects_low_confidence_match() {
        let report = TemplateDetectReport::from_candidates(vec![TemplateDetectCandidate::new(
            "linux",
            20,
            Vec::new(),
        )]);

        let err = select_best_detected_template(&report, DetectConnectPolicy::default())
            .expect_err("low confidence match should fail");

        assert!(matches!(err, ConnectError::AutodetectConfidenceTooLow(_)));
    }

    #[test]
    fn select_best_detected_template_accepts_medium_or_higher_match() {
        let report = TemplateDetectReport::from_candidates(vec![TemplateDetectCandidate::new(
            "cisco",
            90,
            Vec::new(),
        )]);

        let best = select_best_detected_template(&report, DetectConnectPolicy::default())
            .expect("high confidence match should pass");

        assert_eq!(best.template_name, "cisco");
    }

    #[test]
    fn autodetected_connection_exposes_selected_template_name() {
        let report = TemplateDetectReport::from_candidates(vec![TemplateDetectCandidate::new(
            "juniper",
            90,
            Vec::new(),
        )]);
        let (sender, _recv) = mpsc::channel(1);
        let connection = AutodetectedConnection::new(sender, report).expect("connection");

        assert_eq!(connection.template_name, "juniper");
    }

    #[test]
    fn build_detected_connection_request_uses_selected_template() {
        let report = TemplateDetectReport::from_candidates(vec![TemplateDetectCandidate::new(
            "linux",
            90,
            Vec::new(),
        )]);
        let request = DetectRequest::new(
            "adam".to_string(),
            "127.0.0.1".to_string(),
            22,
            "secret".to_string(),
        );

        let best = select_best_detected_template(&report, DetectConnectPolicy::default())
            .expect("best candidate");
        let built = build_detected_connection_request_from_templates(
            request,
            None,
            &best,
            &builtin_detect_templates(),
        )
        .expect("connection request should build");

        let ConnectionRequest {
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
        } = built;

        assert_eq!(user, "adam");
        assert_eq!(addr, "127.0.0.1");
        assert_eq!(port, 22);
        assert_eq!(password, "secret");
        assert_eq!(enable_password, None);
        let expected = by_name_config("linux")
            .expect("linux config")
            .build()
            .expect("linux template");
        assert!(handler.is_equivalent(&expected));
    }

    #[test]
    fn build_detected_connection_request_from_templates_uses_custom_template() {
        let report = TemplateDetectReport::from_candidates(vec![TemplateDetectCandidate::new(
            "custom-linux",
            90,
            Vec::new(),
        )]);
        let request = DetectRequest::new(
            "adam".to_string(),
            "127.0.0.1".to_string(),
            22,
            "secret".to_string(),
        );
        let custom_config = DeviceHandlerConfig {
            prompt: vec![prompt_rule("Root", &[r"^custom#\s*$"])],
            ..DeviceHandlerConfig::default()
        };
        let templates = vec![DetectTemplateDefinition::new(
            "custom-linux",
            custom_config.clone(),
            TemplateDetectProfile::default(),
        )];

        let best = select_best_detected_template(&report, DetectConnectPolicy::default())
            .expect("best candidate");
        let built =
            build_detected_connection_request_from_templates(request, None, &best, &templates)
                .expect("connection request should build");
        let expected = custom_config.build().expect("custom handler");

        assert!(built.handler.is_equivalent(&expected));
    }

    #[test]
    fn build_detected_connection_request_from_templates_rejects_missing_template() {
        let report = TemplateDetectReport::from_candidates(vec![TemplateDetectCandidate::new(
            "custom-linux",
            90,
            Vec::new(),
        )]);
        let request = DetectRequest::new(
            "adam".to_string(),
            "127.0.0.1".to_string(),
            22,
            "secret".to_string(),
        );
        let best = select_best_detected_template(&report, DetectConnectPolicy::default())
            .expect("best candidate");

        let err = match build_detected_connection_request_from_templates(request, None, &best, &[])
        {
            Ok(_) => panic!("missing custom template should fail"),
            Err(err) => err,
        };

        assert!(matches!(err, ConnectError::TemplateNotFound(_)));
    }

    #[test]
    fn merge_with_builtin_detect_templates_appends_new_custom_template() {
        let merged = merge_with_builtin_detect_templates(vec![DetectTemplateDefinition::new(
            "custom-linux",
            DeviceHandlerConfig {
                prompt: vec![prompt_rule("Root", &[r"^custom#\s*$"])],
                ..DeviceHandlerConfig::default()
            },
            TemplateDetectProfile::default(),
        )]);

        assert!(
            merged
                .iter()
                .any(|template| template.template_name == "custom-linux")
        );
        assert!(
            merged
                .iter()
                .any(|template| template.template_name == "cisco")
        );
    }

    #[test]
    fn merge_with_builtin_detect_templates_overrides_builtin_by_name() {
        let custom = DetectTemplateDefinition::new(
            "CiScO",
            DeviceHandlerConfig {
                prompt: vec![prompt_rule("Root", &[r"^custom-cisco#\s*$"])],
                ..DeviceHandlerConfig::default()
            },
            TemplateDetectProfile {
                initial_rules: vec![TemplateProbeRule {
                    pattern: r"^custom-cisco#\s*$".to_string(),
                    weight: 42,
                }],
                probes: Vec::new(),
            },
        );

        let merged = merge_with_builtin_detect_templates(vec![custom]);
        let cisco = merged
            .iter()
            .find(|template| template.template_name.eq_ignore_ascii_case("cisco"))
            .expect("merged cisco template");

        assert_eq!(cisco.detect_profile.initial_rules.len(), 1);
        assert_eq!(cisco.detect_profile.initial_rules[0].weight, 42);
        let expected = DeviceHandlerConfig {
            prompt: vec![prompt_rule("Root", &[r"^custom-cisco#\s*$"])],
            ..DeviceHandlerConfig::default()
        }
        .build()
        .expect("expected custom handler");
        let actual = cisco.handler_config.build().expect("merged custom handler");
        assert!(actual.is_equivalent(&expected));
    }

    #[test]
    fn probe_error_pattern_prevents_positive_score_for_that_probe() {
        let snapshot = DetectSnapshot {
            initial_output: String::new(),
            initial_prompt: "device>".to_string(),
            probe_outputs: HashMap::from([(
                "show version".to_string(),
                "% Invalid input detected at '^' marker.".to_string(),
            )]),
        };

        let report = score_detect_profiles(
            &snapshot,
            vec![(
                "cisco".to_string(),
                TemplateDetectProfile {
                    initial_rules: Vec::new(),
                    probes: vec![TemplateProbe {
                        command: "show version".to_string(),
                        rules: vec![TemplateProbeRule {
                            pattern: r"Cisco".to_string(),
                            weight: 90,
                        }],
                        error_patterns: vec![r"Invalid input".to_string()],
                    }],
                },
            )],
        );

        assert!(report.best_match.is_none());
        assert!(report.candidates.is_empty());
        assert_eq!(report.raw_facts.len(), 1);
        assert_eq!(report.raw_facts[0].kind, DetectFactKind::ErrorPattern);
    }

    #[test]
    fn default_probe_error_patterns_block_scoring_even_without_profile_specific_errors() {
        let snapshot = DetectSnapshot {
            initial_output: String::new(),
            initial_prompt: "router#".to_string(),
            probe_outputs: HashMap::from([(
                "show version".to_string(),
                "% Invalid input detected at '^' marker.".to_string(),
            )]),
        };

        let report = score_detect_profiles(
            &snapshot,
            vec![(
                "cisco".to_string(),
                TemplateDetectProfile {
                    initial_rules: Vec::new(),
                    probes: vec![TemplateProbe {
                        command: "show version".to_string(),
                        rules: vec![TemplateProbeRule {
                            pattern: r"Cisco IOS Software".to_string(),
                            weight: 95,
                        }],
                        error_patterns: Vec::new(),
                    }],
                },
            )],
        );

        assert!(report.best_match.is_none());
        assert!(
            report
                .raw_facts
                .iter()
                .any(|fact| fact.kind == DetectFactKind::ErrorPattern)
        );
    }

    #[test]
    fn probe_error_pattern_is_recorded_without_discarding_initial_rule_score() {
        let snapshot = DetectSnapshot {
            initial_output: String::new(),
            initial_prompt: "<huawei>".to_string(),
            probe_outputs: HashMap::from([(
                "display version".to_string(),
                "Error: Unrecognized command found at '^' position.".to_string(),
            )]),
        };

        let report = score_detect_profiles(
            &snapshot,
            vec![(
                "huawei".to_string(),
                TemplateDetectProfile {
                    initial_rules: vec![TemplateProbeRule {
                        pattern: r"^<[^>]+>$".to_string(),
                        weight: 15,
                    }],
                    probes: vec![TemplateProbe {
                        command: "display version".to_string(),
                        rules: vec![TemplateProbeRule {
                            pattern: r"Huawei".to_string(),
                            weight: 90,
                        }],
                        error_patterns: vec![r"Unrecognized command".to_string()],
                    }],
                },
            )],
        );

        let best = report.best_match.expect("initial rule should still score");
        assert_eq!(best.score, 15);
        assert_eq!(best.matched_facts.len(), 2);
        assert!(
            best.matched_facts
                .iter()
                .any(|fact| fact.kind == DetectFactKind::ErrorPattern)
        );
    }
}
