use std::collections::{HashMap, HashSet, VecDeque};

use log::trace;

use super::{AdjacencyList, DeviceHandler, ExitPath};
use crate::error::ConnectError;

impl DeviceHandler {
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
}
