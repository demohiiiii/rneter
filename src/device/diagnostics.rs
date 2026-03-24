use std::collections::{HashMap, HashSet, VecDeque};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::DeviceHandler;

/// Diagnostics summary for a device state machine graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StateMachineDiagnostics {
    /// Number of declared states.
    pub total_states: usize,
    /// States participating in transition graph edges.
    pub graph_states: Vec<String>,
    /// Graph entry states (in-degree = 0).
    pub entry_states: Vec<String>,
    /// Edge sources that do not exist in declared states.
    pub missing_edge_sources: Vec<String>,
    /// Edge targets that do not exist in declared states.
    pub missing_edge_targets: Vec<String>,
    /// Graph states unreachable from entry states.
    pub unreachable_states: Vec<String>,
    /// Graph states with no outgoing edges.
    pub dead_end_states: Vec<String>,
    /// Prompt regex patterns shared by multiple states.
    pub duplicate_prompt_patterns: Vec<String>,
    /// States participating in duplicate prompt-pattern groups.
    pub potentially_ambiguous_prompt_states: Vec<String>,
    /// States whose outgoing transitions are only self-loop edges.
    pub self_loop_only_states: Vec<String>,
}

impl StateMachineDiagnostics {
    /// Returns true if diagnostics indicate potential template issues.
    pub fn has_issues(&self) -> bool {
        !self.missing_edge_sources.is_empty()
            || !self.missing_edge_targets.is_empty()
            || !self.unreachable_states.is_empty()
            || !self.dead_end_states.is_empty()
            || !self.duplicate_prompt_patterns.is_empty()
            || !self.self_loop_only_states.is_empty()
    }
}

impl DeviceHandler {
    /// Analyze the state transition graph for common template issues.
    pub fn diagnose_state_machine(&self) -> StateMachineDiagnostics {
        let all_states_set: HashSet<String> = self.all_states.iter().cloned().collect();
        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut out_degree: HashMap<String, usize> = HashMap::new();
        let mut graph_states_set: HashSet<String> = HashSet::new();
        let mut missing_edge_sources = HashSet::new();
        let mut missing_edge_targets = HashSet::new();

        for (from, _cmd, to, _is_exit, _needs_format) in &self.edges {
            if !all_states_set.contains(from) {
                missing_edge_sources.insert(from.clone());
                continue;
            }
            if !all_states_set.contains(to) {
                missing_edge_targets.insert(to.clone());
                continue;
            }

            graph_states_set.insert(from.clone());
            graph_states_set.insert(to.clone());

            adjacency.entry(from.clone()).or_default().push(to.clone());
            *out_degree.entry(from.clone()).or_insert(0) += 1;
            *in_degree.entry(to.clone()).or_insert(0) += 1;
            in_degree.entry(from.clone()).or_insert(0);
            out_degree.entry(to.clone()).or_insert(0);
        }

        let mut graph_states = graph_states_set.into_iter().collect::<Vec<_>>();
        graph_states.sort();

        let mut entry_states = graph_states
            .iter()
            .filter(|state| in_degree.get(*state).copied().unwrap_or(0) == 0)
            .cloned()
            .collect::<Vec<_>>();
        entry_states.sort();

        let seeds = if entry_states.is_empty() {
            graph_states
                .first()
                .cloned()
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            entry_states.clone()
        };

        let mut reachable = HashSet::new();
        let mut queue = VecDeque::new();
        for seed in seeds {
            if reachable.insert(seed.clone()) {
                queue.push_back(seed);
            }
        }
        while let Some(node) = queue.pop_front() {
            if let Some(neighbors) = adjacency.get(&node) {
                for next in neighbors {
                    if reachable.insert(next.clone()) {
                        queue.push_back(next.clone());
                    }
                }
            }
        }

        let mut unreachable_states = graph_states
            .iter()
            .filter(|state| !reachable.contains(*state))
            .cloned()
            .collect::<Vec<_>>();
        unreachable_states.sort();

        let mut dead_end_states = graph_states
            .iter()
            .filter(|state| out_degree.get(*state).copied().unwrap_or(0) == 0)
            .cloned()
            .collect::<Vec<_>>();
        dead_end_states.sort();

        let mut missing_edge_sources = missing_edge_sources.into_iter().collect::<Vec<_>>();
        missing_edge_sources.sort();
        let mut missing_edge_targets = missing_edge_targets.into_iter().collect::<Vec<_>>();
        missing_edge_targets.sort();

        let mut duplicate_prompt_patterns = Vec::new();
        let mut ambiguous_states = HashSet::new();
        let mut pattern_states: HashMap<String, HashSet<String>> = HashMap::new();
        for (state, pattern) in &self.prompt_patterns {
            pattern_states
                .entry(pattern.clone())
                .or_default()
                .insert(state.clone());
        }
        for (pattern, states) in pattern_states {
            if states.len() > 1 {
                let mut states_vec = states.into_iter().collect::<Vec<_>>();
                states_vec.sort();
                for state in &states_vec {
                    ambiguous_states.insert(state.clone());
                }
                duplicate_prompt_patterns.push(format!("{pattern} => {}", states_vec.join(",")));
            }
        }
        duplicate_prompt_patterns.sort();
        let mut potentially_ambiguous_prompt_states =
            ambiguous_states.into_iter().collect::<Vec<_>>();
        potentially_ambiguous_prompt_states.sort();

        let mut self_loop_only_states = graph_states
            .iter()
            .filter(|state| {
                let outs = adjacency.get(*state);
                out_degree.get(*state).copied().unwrap_or(0) > 0
                    && outs
                        .map(|targets| targets.iter().all(|target| target == *state))
                        .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>();
        self_loop_only_states.sort();

        StateMachineDiagnostics {
            total_states: self.all_states.len(),
            graph_states,
            entry_states,
            missing_edge_sources,
            missing_edge_targets,
            unreachable_states,
            dead_end_states,
            duplicate_prompt_patterns,
            potentially_ambiguous_prompt_states,
            self_loop_only_states,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::build_test_handler;
    use super::DeviceHandler;
    use crate::device::{DeviceHandlerConfig, prompt_rule, transition_rule};

    #[test]
    fn state_machine_diagnostics_are_clean_for_valid_template() {
        let handler = build_test_handler();
        let report = handler.diagnose_state_machine();

        assert!(!report.has_issues());
        assert!(report.missing_edge_sources.is_empty());
        assert!(report.missing_edge_targets.is_empty());
        assert!(report.unreachable_states.is_empty());
        assert!(report.dead_end_states.is_empty());
        assert!(report.duplicate_prompt_patterns.is_empty());
        assert!(report.potentially_ambiguous_prompt_states.is_empty());
        assert!(report.self_loop_only_states.is_empty());
    }

    #[test]
    fn state_machine_diagnostics_detect_invalid_edges_and_dead_ends() {
        let handler = DeviceHandler::new(DeviceHandlerConfig {
            prompt: vec![
                prompt_rule("Login", &[r"^dev>\s*$"]),
                prompt_rule("Enable", &[r"^dev#\s*$"]),
            ],
            more_regex: vec![r"^--More--$".to_string()],
            error_regex: vec![r"^ERROR: .+$".to_string()],
            edges: vec![
                transition_rule("Login", "enable", "Enable", false, false),
                transition_rule("Enable", "to-ghost", "Ghost", false, false),
            ],
            ..Default::default()
        })
        .expect("handler should build");

        let report = handler.diagnose_state_machine();
        assert!(report.has_issues());
        assert_eq!(report.missing_edge_targets, vec!["ghost".to_string()]);
        assert_eq!(report.dead_end_states, vec!["enable".to_string()]);
    }

    #[test]
    fn state_machine_diagnostics_detect_duplicate_prompt_patterns() {
        let handler = DeviceHandler::new(DeviceHandlerConfig {
            prompt: vec![
                prompt_rule("Login", &[r"^dup>\s*$"]),
                prompt_rule("Enable", &[r"^dup>\s*$"]),
            ],
            more_regex: vec![r"^--More--$".to_string()],
            error_regex: vec![r"^ERROR: .+$".to_string()],
            edges: vec![transition_rule("Login", "noop", "Enable", false, false)],
            ..Default::default()
        })
        .expect("handler should build");

        let report = handler.diagnose_state_machine();
        assert!(report.has_issues());
        assert!(!report.duplicate_prompt_patterns.is_empty());
        assert!(
            report
                .potentially_ambiguous_prompt_states
                .contains(&"enable".to_string())
        );
        assert!(
            report
                .potentially_ambiguous_prompt_states
                .contains(&"login".to_string())
        );
    }

    #[test]
    fn state_machine_diagnostics_detect_self_loop_only_states() {
        let handler = DeviceHandler::new(DeviceHandlerConfig {
            prompt: vec![
                prompt_rule("Login", &[r"^dev>\s*$"]),
                prompt_rule("Enable", &[r"^dev#\s*$"]),
            ],
            more_regex: vec![r"^--More--$".to_string()],
            error_regex: vec![r"^ERROR: .+$".to_string()],
            edges: vec![transition_rule("Enable", "noop", "Enable", false, false)],
            ..Default::default()
        })
        .expect("handler should build");

        let report = handler.diagnose_state_machine();
        assert!(report.self_loop_only_states.contains(&"enable".to_string()));
    }
}
