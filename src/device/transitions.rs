use std::collections::{HashMap, HashSet, VecDeque};

use log::trace;

use super::{DeviceHandler, ExitPath};
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
                    Self::format_cmd(**format, cmd, sys.map(|s| s.as_str())),
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
    fn format_cmd(format: bool, cmd: &str, sys: Option<&str>) -> String {
        if format {
            if let Some(s) = sys {
                cmd.replace("{}", s)
            } else {
                String::new()
            }
        } else {
            cmd.to_string()
        }
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

        let mut adj_list: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (from, label, to, _, format) in &self.edges {
            adj_list.entry(from.clone()).or_default().push((
                to.clone(),
                Self::format_cmd(*format, label, sys.map(|s| s.as_str())),
            ));
        }

        let mut queue = VecDeque::new();
        queue.push_back(start_node.clone());

        let mut visited = HashSet::new();
        visited.insert(start_node.clone());

        let mut predecessors: HashMap<String, (String, String)> = HashMap::new();

        while let Some(current_node) = queue.pop_front() {
            trace!("Current node: '{:?}'", current_node);
            if current_node == end_node {
                break;
            }

            if let Some(neighbors) = adj_list.get(&current_node) {
                for (neighbor_node, edge_label) in neighbors {
                    if !visited.contains(neighbor_node) {
                        visited.insert(neighbor_node.clone());
                        predecessors.insert(
                            neighbor_node.clone(),
                            (current_node.clone(), edge_label.clone()),
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
            if let Some((parent, edge_label)) = predecessors.get(&current) {
                path.push((edge_label.clone(), current.clone()));
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
    use super::super::build_test_handler;
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
}
