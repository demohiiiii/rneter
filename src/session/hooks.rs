use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::SessionOperation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTrigger<'a> {
    AfterConnect,
    BeforeDisconnect,
    BeforeExitState(&'a str),
    AfterEnterState(&'a str),
}

impl HookTrigger<'_> {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::AfterConnect => "after_connect",
            Self::BeforeDisconnect => "before_disconnect",
            Self::BeforeExitState(_) => "before_exit_state",
            Self::AfterEnterState(_) => "after_enter_state",
        }
    }

    pub(crate) fn state(&self) -> Option<&str> {
        match self {
            Self::BeforeExitState(state) | Self::AfterEnterState(state) => Some(state),
            Self::AfterConnect | Self::BeforeDisconnect => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookFailurePolicy {
    Required,
    #[default]
    BestEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct HookAction {
    pub name: String,
    pub operation: SessionOperation,
    #[serde(default)]
    pub failure_policy: HookFailurePolicy,
    #[serde(default)]
    pub record_output: bool,
}

impl HookAction {
    pub fn new(name: impl Into<String>, operation: SessionOperation) -> Self {
        Self {
            name: name.into(),
            operation,
            failure_policy: HookFailurePolicy::BestEffort,
            record_output: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SessionHooks {
    #[serde(default)]
    pub after_connect: Vec<HookAction>,
    #[serde(default)]
    pub before_disconnect: Vec<HookAction>,
    #[serde(default)]
    pub after_enter_state: HashMap<String, Vec<HookAction>>,
    #[serde(default)]
    pub before_exit_state: HashMap<String, Vec<HookAction>>,
}

impl SessionHooks {
    pub(crate) fn normalized(self) -> Self {
        Self {
            after_connect: self.after_connect,
            before_disconnect: self.before_disconnect,
            after_enter_state: normalize_state_hook_map(self.after_enter_state),
            before_exit_state: normalize_state_hook_map(self.before_exit_state),
        }
    }

    pub fn after_enter_state(&self, state: &str) -> &[HookAction] {
        state_actions(&self.after_enter_state, state)
    }

    pub fn before_exit_state(&self, state: &str) -> &[HookAction] {
        state_actions(&self.before_exit_state, state)
    }
}

fn normalize_state_hook_map(
    map: HashMap<String, Vec<HookAction>>,
) -> HashMap<String, Vec<HookAction>> {
    let mut normalized = HashMap::with_capacity(map.len());
    for (state, actions) in map {
        normalized
            .entry(state.to_ascii_lowercase())
            .or_insert_with(Vec::new)
            .extend(actions);
    }
    normalized
}

fn state_actions<'a>(
    map: &'a HashMap<String, Vec<HookAction>>,
    state: &str,
) -> &'a [HookAction] {
    map.get(state)
        .or_else(|| map.get(&state.to_ascii_lowercase()))
        .or_else(|| {
            map.iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(state))
                .map(|(_, actions)| actions)
        })
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Command, SessionOperation};

    #[test]
    fn hook_action_defaults_to_best_effort_and_no_record_output() {
        let hook = HookAction::new(
            "disable-paging",
            SessionOperation::from(Command {
                mode: "enable".to_string(),
                command: "terminal length 0".to_string(),
                ..Command::default()
            }),
        );

        assert_eq!(hook.failure_policy, HookFailurePolicy::BestEffort);
        assert!(!hook.record_output);
    }

    #[test]
    fn session_hooks_state_lookup_returns_empty_when_missing() {
        let hooks = SessionHooks::default();
        assert!(hooks.after_enter_state("config").is_empty());
        assert!(hooks.before_exit_state("enable").is_empty());
    }

    #[test]
    fn session_hooks_state_lookup_is_case_insensitive() {
        let hook = HookAction::new(
            "prepare-config",
            SessionOperation::from(Command {
                mode: "enable".to_string(),
                command: "configure terminal".to_string(),
                ..Command::default()
            }),
        );
        let hooks = SessionHooks {
            after_enter_state: HashMap::from([("Config".to_string(), vec![hook.clone()])]),
            before_exit_state: HashMap::from([("ENABLE".to_string(), vec![hook.clone()])]),
            ..SessionHooks::default()
        };

        assert_eq!(hooks.after_enter_state("config"), &[hook.clone()]);
        assert_eq!(hooks.after_enter_state("CONFIG"), &[hook.clone()]);
        assert_eq!(hooks.before_exit_state("enable"), &[hook.clone()]);
        assert_eq!(hooks.before_exit_state("Enable"), &[hook]);
    }

    #[test]
    fn session_hooks_normalized_lowercases_state_maps() {
        let hook = HookAction::new(
            "prepare-config",
            SessionOperation::from(Command {
                mode: "enable".to_string(),
                command: "configure terminal".to_string(),
                ..Command::default()
            }),
        );
        let hooks = SessionHooks {
            after_enter_state: HashMap::from([("Config".to_string(), vec![hook.clone()])]),
            before_exit_state: HashMap::from([("ENABLE".to_string(), vec![hook.clone()])]),
            ..SessionHooks::default()
        }
        .normalized();

        assert!(hooks.after_enter_state.contains_key("config"));
        assert!(!hooks.after_enter_state.contains_key("Config"));
        assert!(hooks.before_exit_state.contains_key("enable"));
        assert_eq!(hooks.after_enter_state("CONFIG"), &[hook.clone()]);
        assert_eq!(hooks.before_exit_state("Enable"), &[hook]);
    }
}
