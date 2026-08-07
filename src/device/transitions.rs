use std::collections::{HashMap, HashSet, VecDeque};

use log::trace;

use super::{AdjacencyList, DeviceHandler, ExitPath};
use crate::error::ConnectError;

impl DeviceHandler {
    /// Calculates a transition path for one of several acceptable states.
    ///
    /// The current state always wins when it is one of the candidates. When it
    /// is not, an exit-only path is preferred so callers can request an outer
    /// mode (for example `login,config`) without accidentally entering a
    /// deeper mode first. If no outer candidate is reachable, candidates are
    /// tried in their supplied order using the normal transition graph.
    pub fn trans_state_write_candidates(
        &self,
        states: &[&str],
        sys: Option<&String>,
    ) -> Result<(String, Vec<(String, String)>), ConnectError> {
        let candidates = states
            .iter()
            .map(|state| state.trim().to_ascii_lowercase())
            .filter(|state| !state.is_empty())
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(ConnectError::InvalidCommandFlow(
                "command mode must include at least one state".to_string(),
            ));
        }

        let current = self.current_state().to_string();
        if candidates.iter().any(|state| state == &current) {
            let path = self.trans_state_write(&current, sys)?;
            return Ok((current, path));
        }

        // Follow only exit edges to find the outermost acceptable state.
        // This deliberately ignores enter edges: from `enable`, `login` is
        // preferred over `config` when both were requested.
        let mut queue = VecDeque::from([(current.clone(), 0usize)]);
        let mut visited = HashSet::from([current]);
        let mut outer_candidate = None::<(String, usize, usize)>;
        while let Some((state, distance)) = queue.pop_front() {
            for (from, _, to, is_exit, _) in &self.edges {
                if !*is_exit || from != &state || visited.contains(to) {
                    continue;
                }
                let next_distance = distance + 1;
                if let Some(candidate_index) = candidates
                    .iter()
                    .position(|candidate| candidate == &to.to_ascii_lowercase())
                {
                    let replace =
                        outer_candidate
                            .as_ref()
                            .is_none_or(|(_, best_distance, best_index)| {
                                next_distance > *best_distance
                                    || (next_distance == *best_distance
                                        && candidate_index < *best_index)
                            });
                    if replace {
                        outer_candidate =
                            Some((to.to_ascii_lowercase(), next_distance, candidate_index));
                    }
                }
                visited.insert(to.clone());
                queue.push_back((to.clone(), next_distance));
            }
        }
        if let Some((target, _, _)) = outer_candidate {
            return Ok((target.clone(), self.trans_state_write(&target, sys)?));
        }

        let mut first_error = None;
        for candidate in candidates {
            match self.trans_state_write(&candidate, sys) {
                Ok(path) => return Ok((candidate, path)),
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }

        Err(first_error.unwrap_or_else(|| {
            ConnectError::UnreachableState(
                states
                    .first()
                    .map(|state| state.trim())
                    .unwrap_or_default()
                    .to_string(),
            )
        }))
    }

    /// Finds the path to exit from system-specific prompts.
    fn exit_until_no_sys(&self, sys: Option<&String>) -> Result<ExitPath, ConnectError> {
        if !self.match_sys_prompt(self.current_state_index) {
            return Ok(None);
        }
        let exit_edges = self.edges.iter().filter(|(_, _, _, exit, _)| *exit);
        let mut edge_map = HashMap::new();
        for (start, cmd, end, _, format) in exit_edges {
            edge_map.insert(start, (cmd, end, format));
        }
        let mut path = Vec::new();
        let mut current = &self.current_state().to_string();
        loop {
            if let Some((cmd, end, format)) = edge_map.get(current) {
                path.push((
                    Self::format_cmd(**format, cmd, sys.map(|s| s.as_str()))?,
                    (*end).to_string(),
                ));
                if let Some(index) = self.all_states.iter().position(|v| v.eq(*end)) {
                    if !self.match_sys_prompt(index) {
                        return Ok(Some(((*end).to_string(), path)));
                    }
                    current = *end;
                } else {
                    return Err(ConnectError::TargetStateNotExistError);
                }
            } else {
                return Err(ConnectError::NoExitCommandError(current.clone()));
            }
        }
    }

    /// Formats a command string with system name substitution.
    ///
    /// A transition edge marked as needing formatting requires a system name;
    /// returning an error here (instead of silently producing an empty
    /// command) prevents the state machine from believing a transition
    /// succeeded when only a bare newline was sent to the device.
    fn format_cmd(format: bool, cmd: &str, sys: Option<&str>) -> Result<String, ConnectError> {
        if format {
            match sys {
                Some(s) if s.is_empty() || s.chars().any(char::is_control) => {
                    Err(ConnectError::InvalidCommandFlow(
                        "transition system name must be non-empty and contain no control characters"
                            .to_string(),
                    ))
                }
                Some(s) => Ok(cmd.replace("{}", s)),
                None => Err(ConnectError::InvalidCommandFlow(format!(
                    "transition command '{cmd}' requires a system name (sys), but none was provided"
                ))),
            }
        } else {
            Ok(cmd.to_string())
        }
    }

    /// Returns the cached adjacency list over the transition edges.
    ///
    /// The edge set is immutable after construction, so the list is built
    /// once and reused by every subsequent transition computation. Commands
    /// are kept unformatted here; system-name substitution happens only for
    /// the edges actually chosen for a path.
    fn adjacency_list(&self) -> &AdjacencyList {
        self.adjacency.get_or_init(|| {
            let mut adj_list: AdjacencyList = HashMap::new();
            for (from, label, to, _, format) in &self.edges {
                adj_list.entry(from.clone()).or_default().push((
                    to.clone(),
                    label.clone(),
                    *format,
                ));
            }
            adj_list
        })
    }

    /// Calculates the commands needed to transition to a target state.
    pub fn trans_state_write(
        &self,
        state: &str,
        sys: Option<&String>,
    ) -> Result<Vec<(String, String)>, ConnectError> {
        let mut start_node = self.current_state().to_string();
        let end_node = state;
        let mut switch_path = Vec::new();

        if let (Some(current_sys), Some(target_sys)) = (&self.sys, sys)
            && current_sys != target_sys
        {
            trace!("Need to switch system: {} to {}", current_sys, target_sys);
            if let Some((node, exit_path)) = self.exit_until_no_sys(sys)? {
                start_node = node;
                switch_path.extend(exit_path);
            }
        }

        if start_node == end_node {
            return Ok(switch_path);
        }

        let adj_list = self.adjacency_list();

        let mut queue = VecDeque::new();
        queue.push_back(start_node.clone());

        let mut visited = HashSet::new();
        visited.insert(start_node.clone());

        // Maps a node to (parent, raw_command, needs_format); formatting is
        // deferred until the final path is known so unrelated edges that
        // need a system name never abort an independent transition.
        let mut predecessors: HashMap<String, (String, String, bool)> = HashMap::new();

        while let Some(current_node) = queue.pop_front() {
            trace!("Current node: '{:?}'", current_node);
            if current_node == end_node {
                break;
            }

            if let Some(neighbors) = adj_list.get(&current_node) {
                for (neighbor_node, edge_label, needs_format) in neighbors {
                    if !visited.contains(neighbor_node) {
                        visited.insert(neighbor_node.clone());
                        predecessors.insert(
                            neighbor_node.clone(),
                            (current_node.clone(), edge_label.clone(), *needs_format),
                        );
                        queue.push_back(neighbor_node.clone());
                    }
                }
            }
        }

        if !predecessors.contains_key(end_node) {
            return Err(ConnectError::UnreachableState(end_node.to_string()));
        }

        let mut current = end_node.to_string();
        let mut path = Vec::new();

        while current != start_node {
            if let Some((parent, edge_label, needs_format)) = predecessors.get(&current) {
                path.push((
                    Self::format_cmd(*needs_format, edge_label, sys.map(|s| s.as_str()))?,
                    current.clone(),
                ));
                current = parent.clone();
            } else {
                return Err(ConnectError::InternalServerError(format!(
                    "failed to backtrack path from '{}' to '{}'",
                    end_node, start_node
                )));
            }
        }

        path.reverse();
        switch_path.extend(path);
        trace!("Command path: '{:?}'", switch_path);
        Ok(switch_path)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::{
        DeviceHandler, DeviceHandlerConfig, build_test_handler, prompt_rule, transition_rule,
    };
    use crate::error::ConnectError;

    #[test]
    fn transition_path_is_found_for_reachable_state() {
        let mut handler = build_test_handler();
        handler.read("dev>");

        let path = handler
            .trans_state_write("config", None)
            .expect("reachable path should be found");

        assert_eq!(
            path,
            vec![
                ("enable".to_string(), "enable".to_string()),
                ("configure terminal".to_string(), "config".to_string()),
            ]
        );
    }

    #[test]
    fn transition_to_unknown_state_returns_error() {
        let mut handler = build_test_handler();
        handler.read("dev>");

        let err = handler
            .trans_state_write("does-not-exist", None)
            .expect_err("unknown target state should return error");
        match err {
            ConnectError::UnreachableState(s) => assert_eq!(s, "does-not-exist"),
            other => panic!("unexpected error type: {other}"),
        }
    }

    fn build_sys_edge_handler() -> DeviceHandler {
        DeviceHandler::new(DeviceHandlerConfig {
            prompt: vec![
                prompt_rule("Enable", &[r"^dev#\s*$"]),
                prompt_rule("Config", &[r"^dev\(cfg\)#\s*$"]),
                prompt_rule("VSite", &[r"^dev\(vsite\)#\s*$"]),
            ],
            edges: vec![
                transition_rule("Enable", "configure terminal", "Config", false, false),
                transition_rule("Config", "exit", "Enable", true, false),
                transition_rule("Enable", "switch {}", "VSite", false, true),
                transition_rule("VSite", "switch back", "Enable", true, false),
            ],
            dyn_param: HashMap::new(),
            ..Default::default()
        })
        .expect("sys edge handler config should be valid")
    }

    #[test]
    fn transition_over_sys_edge_without_sys_returns_error() {
        let mut handler = build_sys_edge_handler();
        handler.read("dev#");

        let err = handler
            .trans_state_write("vsite", None)
            .expect_err("sys-formatted edge without sys must not send an empty command");
        match err {
            ConnectError::InvalidCommandFlow(message) => {
                assert!(message.contains("switch {}"), "message: {message}");
            }
            other => panic!("unexpected error type: {other}"),
        }
    }

    #[test]
    fn transition_over_sys_edge_with_sys_substitutes_name() {
        let mut handler = build_sys_edge_handler();
        handler.read("dev#");

        let sys = "site-a".to_string();
        let path = handler
            .trans_state_write("vsite", Some(&sys))
            .expect("sys-formatted edge with sys should succeed");
        assert_eq!(
            path,
            vec![("switch site-a".to_string(), "vsite".to_string())]
        );
    }

    #[test]
    fn transition_rejects_control_characters_in_system_name() {
        let mut handler = build_sys_edge_handler();
        handler.read("dev#");

        let sys = "site-a\nconfigure terminal".to_string();
        let error = handler
            .trans_state_write("vsite", Some(&sys))
            .expect_err("control characters must not reach a transition command");

        assert!(matches!(error, ConnectError::InvalidCommandFlow(_)));
    }

    #[test]
    fn transition_avoiding_sys_edge_still_works_without_sys() {
        // The path Enable -> Config never touches the sys-formatted edge, so
        // a missing system name must not abort this unrelated transition.
        let mut handler = build_sys_edge_handler();
        handler.read("dev#");

        let path = handler
            .trans_state_write("config", None)
            .expect("path without sys edges should not require sys");
        assert_eq!(
            path,
            vec![("configure terminal".to_string(), "config".to_string())]
        );
    }

    #[test]
    fn candidate_transition_keeps_current_outer_or_inner_state() {
        let mut handler = build_test_handler();
        handler.read("dev#");

        let (target, path) = handler
            .trans_state_write_candidates(&["config", "enable"], None)
            .expect("current state should be accepted");
        assert_eq!(target, "enable");
        assert!(path.is_empty());
    }

    #[test]
    fn candidate_transition_prefers_exit_path_to_outer_state() {
        let mut handler = build_test_handler();
        handler.read("dev#");

        let (target, path) = handler
            .trans_state_write_candidates(&["config", "login"], None)
            .expect("outer candidate should be reachable");
        assert_eq!(target, "login");
        assert_eq!(path, vec![("exit".to_string(), "login".to_string())]);
    }

    #[test]
    fn candidate_transition_chooses_outermost_exit_candidate() {
        let mut handler = build_test_handler();
        handler.read("dev(cfg)#");

        let (target, path) = handler
            .trans_state_write_candidates(&["enable", "login"], None)
            .expect("outermost candidate should be reachable");
        assert_eq!(target, "login");
        assert_eq!(
            path,
            vec![
                ("exit".to_string(), "enable".to_string()),
                ("exit".to_string(), "login".to_string()),
            ]
        );
    }

    #[test]
    fn linux_candidate_transition_does_not_drop_root_to_user() {
        let mut handler = DeviceHandler::new(DeviceHandlerConfig {
            prompt: vec![
                prompt_rule("User", &[r"^user\$\s*$"]),
                prompt_rule("Root", &[r"^root#\s*$"]),
            ],
            edges: vec![
                transition_rule("User", "sudo -i", "Root", false, false),
                transition_rule("Root", "exit", "User", true, false),
            ],
            ..Default::default()
        })
        .expect("linux-like handler should build");
        handler.read("root#");

        let (target, path) = handler
            .trans_state_write_candidates(&["root", "user"], None)
            .expect("root should be accepted as-is");
        assert_eq!(target, "root");
        assert!(path.is_empty());
    }
}
