use super::*;

/// High-level command block type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandBlockKind {
    Show,
    Config,
}

/// Rollback strategy used when a config block fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RollbackPolicy {
    /// No rollback. Only valid for `show` blocks.
    None,
    /// Roll back the whole resource with one command.
    WholeResource {
        /// Mode used to execute the rollback command.
        mode: String,
        /// Single compensating command for the whole block.
        undo_command: String,
        /// Optional timeout for rollback command execution.
        timeout_secs: Option<u64>,
    },
    /// Roll back each executed step in reverse order.
    PerStep,
}

/// One command step inside a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxStep {
    /// Target mode before running this step.
    pub mode: String,
    /// Forward command.
    pub command: String,
    /// Optional timeout for this step.
    pub timeout_secs: Option<u64>,
    /// Compensating command for `PerStep` rollback policy.
    pub rollback_command: Option<String>,
}

/// Transaction-like command block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxBlock {
    /// Logical name used in logs/recording.
    pub name: String,
    /// Block type (`show` skips rollback, `config` requires rollback policy).
    pub kind: CommandBlockKind,
    /// Rollback behavior when any step fails.
    pub rollback_policy: RollbackPolicy,
    /// Commands executed in order.
    pub steps: Vec<TxStep>,
    /// Stop at first failure.
    pub fail_fast: bool,
}

/// Planned rollback command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRollback {
    /// Mode used for rollback command execution.
    pub mode: String,
    /// Rollback command text.
    pub command: String,
    /// Optional timeout for this rollback command.
    pub timeout_secs: Option<u64>,
}

/// Execution result of a transaction-like block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxResult {
    /// Input block name.
    pub block_name: String,
    /// True when all steps succeeded.
    pub committed: bool,
    /// First failed step index, if any.
    pub failed_step: Option<usize>,
    /// Number of forward steps executed successfully.
    pub executed_steps: usize,
    /// Whether rollback phase started.
    pub rollback_attempted: bool,
    /// Whether rollback phase ended without rollback errors.
    pub rollback_succeeded: bool,
    /// Number of rollback commands attempted.
    pub rollback_steps: usize,
    /// First-level failure summary for the forward phase.
    pub failure_reason: Option<String>,
    /// Rollback phase errors (can contain multiple entries).
    pub rollback_errors: Vec<String>,
}

/// Multi-block workflow transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxWorkflow {
    /// Workflow name used in logs/recording.
    pub name: String,
    /// Ordered transaction blocks.
    pub blocks: Vec<TxBlock>,
    /// Stop at first failed block (recommended true).
    pub fail_fast: bool,
}

/// Workflow execution result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxWorkflowResult {
    /// Input workflow name.
    pub workflow_name: String,
    /// True when all blocks committed.
    pub committed: bool,
    /// First failed block index.
    pub failed_block: Option<usize>,
    /// Per-block execution results in workflow order.
    pub block_results: Vec<TxResult>,
    /// Whether global rollback for previous committed blocks was attempted.
    pub rollback_attempted: bool,
    /// Whether global rollback completed without rollback errors.
    pub rollback_succeeded: bool,
    /// Aggregated rollback errors from global rollback stage.
    pub rollback_errors: Vec<String>,
}

/// Calculate reverse rollback order for committed blocks before a failure point.
///
/// Example:
/// - committed blocks: `[0, 1, 2]`
/// - failed block: `2`
/// - rollback order: `[1, 0]`
pub fn workflow_rollback_order(
    committed_block_indices: &[usize],
    failed_block: usize,
) -> Vec<usize> {
    committed_block_indices
        .iter()
        .rev()
        .copied()
        .filter(|idx| *idx < failed_block)
        .collect()
}

/// Extract initial workflow rollback status from the failed block result itself.
///
/// The failed block may already attempt rollback inside `execute_tx_block`.
/// Workflow-level rollback summary must include this outcome.
pub fn failed_block_rollback_summary(
    failed_block_result: Option<&TxResult>,
) -> (bool, bool, Vec<String>) {
    if let Some(result) = failed_block_result
        && result.rollback_attempted
    {
        return (
            true,
            result.rollback_succeeded,
            result.rollback_errors.clone(),
        );
    }
    (false, true, Vec::new())
}

impl TxBlock {
    /// Validate cross-field invariants before execution.
    ///
    /// Key rule: `show` blocks must not define rollback; `config` blocks must.
    pub fn validate(&self) -> Result<(), ConnectError> {
        if self.steps.is_empty() {
            return Err(ConnectError::InvalidTransaction(
                "block has no steps".to_string(),
            ));
        }

        for (i, step) in self.steps.iter().enumerate() {
            if step.mode.trim().is_empty() {
                return Err(ConnectError::InvalidTransaction(format!(
                    "step[{i}] mode is empty"
                )));
            }
            if step.command.trim().is_empty() {
                return Err(ConnectError::InvalidTransaction(format!(
                    "step[{i}] command is empty"
                )));
            }
        }

        match (&self.kind, &self.rollback_policy) {
            (CommandBlockKind::Show, RollbackPolicy::None) => {}
            (CommandBlockKind::Show, _) => {
                return Err(ConnectError::InvalidTransaction(
                    "show block must use rollback_policy=none".to_string(),
                ));
            }
            (CommandBlockKind::Config, RollbackPolicy::None) => {
                return Err(ConnectError::InvalidTransaction(
                    "config block requires rollback policy".to_string(),
                ));
            }
            (
                CommandBlockKind::Config,
                RollbackPolicy::WholeResource {
                    mode, undo_command, ..
                },
            ) => {
                if mode.trim().is_empty() || undo_command.trim().is_empty() {
                    return Err(ConnectError::InvalidTransaction(
                        "whole_resource rollback requires non-empty mode and undo_command"
                            .to_string(),
                    ));
                }
            }
            (CommandBlockKind::Config, RollbackPolicy::PerStep) => {
                for (i, step) in self.steps.iter().enumerate() {
                    if step
                        .rollback_command
                        .as_ref()
                        .map(|v| v.trim().is_empty())
                        .unwrap_or(true)
                    {
                        return Err(ConnectError::InvalidTransaction(format!(
                            "step[{i}] missing rollback_command for per_step policy"
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn plan_rollback(
        &self,
        executed_step_indices: &[usize],
    ) -> Result<Vec<PlannedRollback>, ConnectError> {
        match &self.rollback_policy {
            RollbackPolicy::None => Ok(Vec::new()),
            RollbackPolicy::WholeResource {
                mode,
                undo_command,
                timeout_secs,
            } => Ok(vec![PlannedRollback {
                mode: mode.clone(),
                command: undo_command.clone(),
                timeout_secs: *timeout_secs,
            }]),
            RollbackPolicy::PerStep => {
                let mut commands = Vec::new();
                // Roll back in reverse execution order to mirror stack unwind behavior.
                for idx in executed_step_indices.iter().rev() {
                    let step = self.steps.get(*idx).ok_or_else(|| {
                        ConnectError::InvalidTransaction(format!(
                            "executed step index out of range: {idx}"
                        ))
                    })?;
                    let rollback_command = step.rollback_command.as_ref().ok_or_else(|| {
                        ConnectError::InvalidTransaction(format!(
                            "step[{idx}] missing rollback_command"
                        ))
                    })?;
                    commands.push(PlannedRollback {
                        mode: step.mode.clone(),
                        command: rollback_command.clone(),
                        timeout_secs: step.timeout_secs,
                    });
                }
                Ok(commands)
            }
        }
    }
}

impl TxResult {
    pub fn committed(block_name: String, executed_steps: usize) -> Self {
        Self {
            block_name,
            committed: true,
            failed_step: None,
            executed_steps,
            rollback_attempted: false,
            rollback_succeeded: false,
            rollback_steps: 0,
            failure_reason: None,
            rollback_errors: Vec::new(),
        }
    }
}

impl TxWorkflow {
    /// Validate workflow and nested blocks.
    pub fn validate(&self) -> Result<(), ConnectError> {
        if self.blocks.is_empty() {
            return Err(ConnectError::InvalidTransaction(
                "workflow has no blocks".to_string(),
            ));
        }
        for (i, block) in self.blocks.iter().enumerate() {
            block.validate().map_err(|err| {
                ConnectError::InvalidTransaction(format!("block[{i}] validation failed: {err}"))
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn per_step_block() -> TxBlock {
        TxBlock {
            name: "addr-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![
                TxStep {
                    mode: "Config".to_string(),
                    command: "set addr 1".to_string(),
                    timeout_secs: None,
                    rollback_command: Some("unset addr 1".to_string()),
                },
                TxStep {
                    mode: "Config".to_string(),
                    command: "set addr 2".to_string(),
                    timeout_secs: None,
                    rollback_command: Some("unset addr 2".to_string()),
                },
            ],
            fail_fast: true,
        }
    }

    #[test]
    fn config_block_requires_rollback_policy() {
        let mut block = per_step_block();
        block.rollback_policy = RollbackPolicy::None;
        let err = block
            .validate()
            .expect_err("config requires rollback policy");
        assert!(matches!(err, ConnectError::InvalidTransaction(_)));
    }

    #[test]
    fn show_block_requires_none_rollback_policy() {
        let mut block = per_step_block();
        block.kind = CommandBlockKind::Show;
        let err = block.validate().expect_err("show must not define rollback");
        assert!(matches!(err, ConnectError::InvalidTransaction(_)));
    }

    #[test]
    fn per_step_rollback_plan_is_reverse_order() {
        let block = per_step_block();
        let plan = block.plan_rollback(&[0, 1]).expect("plan rollback");
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].command, "unset addr 2");
        assert_eq!(plan[1].command, "unset addr 1");
    }

    #[test]
    fn whole_resource_plan_is_single_command() {
        let block = TxBlock {
            name: "addr-create".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                mode: "Config".to_string(),
                undo_command: "no address-object A".to_string(),
                timeout_secs: Some(30),
            },
            steps: vec![TxStep {
                mode: "Config".to_string(),
                command: "address-object A".to_string(),
                timeout_secs: None,
                rollback_command: None,
            }],
            fail_fast: true,
        };
        let plan = block.plan_rollback(&[0]).expect("plan rollback");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].command, "no address-object A");
    }

    #[test]
    fn workflow_requires_at_least_one_block() {
        let workflow = TxWorkflow {
            name: "fw-policy".to_string(),
            blocks: vec![],
            fail_fast: true,
        };
        let err = workflow
            .validate()
            .expect_err("workflow without block should fail");
        assert!(matches!(err, ConnectError::InvalidTransaction(_)));
    }

    #[test]
    fn workflow_validation_reuses_block_validation() {
        let invalid_block = TxBlock {
            name: "bad".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![TxStep {
                mode: "Config".to_string(),
                command: "set x".to_string(),
                timeout_secs: None,
                rollback_command: None,
            }],
            fail_fast: true,
        };
        let workflow = TxWorkflow {
            name: "wf".to_string(),
            blocks: vec![invalid_block],
            fail_fast: true,
        };
        let err = workflow.validate().expect_err("invalid nested block");
        assert!(matches!(err, ConnectError::InvalidTransaction(_)));
    }

    #[test]
    fn workflow_rollback_order_reverses_committed_prefix() {
        let order = workflow_rollback_order(&[0, 1, 2], 2);
        assert_eq!(order, vec![1, 0]);
    }

    #[test]
    fn workflow_rollback_order_empty_when_first_block_failed() {
        let order = workflow_rollback_order(&[], 0);
        assert!(order.is_empty());
    }

    #[test]
    fn failed_block_rollback_summary_propagates_rollback_failure() {
        let failed = TxResult {
            block_name: "policy".to_string(),
            committed: false,
            failed_step: Some(2),
            executed_steps: 2,
            rollback_attempted: true,
            rollback_succeeded: false,
            rollback_steps: 1,
            failure_reason: Some("step failed".to_string()),
            rollback_errors: vec!["undo command failed".to_string()],
        };
        let (attempted, succeeded, errors) = failed_block_rollback_summary(Some(&failed));
        assert!(attempted);
        assert!(!succeeded);
        assert_eq!(errors, vec!["undo command failed".to_string()]);
    }

    #[test]
    fn failed_block_rollback_summary_defaults_when_not_attempted() {
        let failed = TxResult {
            block_name: "policy".to_string(),
            committed: false,
            failed_step: Some(1),
            executed_steps: 1,
            rollback_attempted: false,
            rollback_succeeded: false,
            rollback_steps: 0,
            failure_reason: Some("step failed".to_string()),
            rollback_errors: vec!["ignored".to_string()],
        };
        let (attempted, succeeded, errors) = failed_block_rollback_summary(Some(&failed));
        assert!(!attempted);
        assert!(succeeded);
        assert!(errors.is_empty());
    }
}
