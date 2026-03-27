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
        /// Compensating operation for the whole block.
        rollback: Box<SessionOperation>,
        /// Only run whole-resource rollback when this step index has executed
        /// successfully. Defaults to first command (index 0).
        #[serde(default = "default_whole_resource_trigger_step_index")]
        trigger_step_index: usize,
    },
    /// Roll back each executed step in reverse order.
    PerStep,
}

fn default_whole_resource_trigger_step_index() -> usize {
    0
}

/// One step inside a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxStep {
    /// Forward operation executed for this step.
    pub run: SessionOperation,
    /// Compensating operation for `PerStep` rollback policy.
    pub rollback: Option<SessionOperation>,
    /// Whether to run this step's rollback operation even when the forward
    /// operation of this very step failed.
    ///
    /// Default is `false`: only previously successful steps are rolled back.
    #[serde(default)]
    pub rollback_on_failure: bool,
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

/// Planned rollback operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRollback {
    /// Step index associated with this rollback operation.
    ///
    /// `None` means the rollback is block-level (`WholeResource`).
    pub step_index: Option<usize>,
    /// Rollback operation to execute.
    pub operation: SessionOperation,
}

/// Final forward execution state of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TxStepExecutionState {
    /// Step was not attempted because execution stopped earlier.
    #[default]
    NotRun,
    /// Forward operation completed successfully.
    Succeeded,
    /// Forward operation was attempted and failed.
    Failed,
}

/// Final rollback state associated with one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TxStepRollbackState {
    /// No rollback was needed for this step.
    #[default]
    NotNeeded,
    /// Step-level rollback operation succeeded.
    Succeeded,
    /// Step-level rollback operation failed.
    Failed,
    /// Step-level rollback was skipped.
    Skipped,
    /// Block-level rollback succeeded and covered this step.
    BlockSucceeded,
    /// Block-level rollback failed and affected this step.
    BlockFailed,
    /// Block-level rollback was skipped and affected this step.
    BlockSkipped,
}

/// Detailed execution report for one block step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxOperationStepResult {
    /// Child step index inside one rendered operation.
    pub step_index: usize,
    /// Mode used for the child step.
    pub mode: String,
    /// Concrete child step summary, currently the rendered command text.
    pub operation_summary: String,
    /// Whether the child step succeeded.
    pub success: bool,
    /// Optional exit code captured from shell execution.
    pub exit_code: Option<i32>,
    /// Primary captured content for this child step.
    pub content: String,
    /// Full captured transcript for this child step.
    pub all: String,
    /// Prompt observed after the child step finished.
    pub prompt: Option<String>,
}

impl From<SessionOperationStepOutput> for TxOperationStepResult {
    fn from(value: SessionOperationStepOutput) -> Self {
        Self {
            step_index: value.step_index,
            mode: value.mode,
            operation_summary: value.operation_summary,
            success: value.success,
            exit_code: value.exit_code,
            content: value.content,
            all: value.all,
            prompt: value.prompt,
        }
    }
}

impl From<TxOperationStepResult> for SessionOperationStepOutput {
    fn from(value: TxOperationStepResult) -> Self {
        Self {
            step_index: value.step_index,
            mode: value.mode,
            operation_summary: value.operation_summary,
            success: value.success,
            exit_code: value.exit_code,
            content: value.content,
            all: value.all,
            prompt: value.prompt,
        }
    }
}

/// Detailed execution report for one block step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TxStepResult {
    /// Original step index inside the block.
    pub step_index: usize,
    /// Mode used for forward execution.
    pub mode: String,
    /// Forward operation summary text.
    pub operation_summary: String,
    /// Final forward execution state.
    pub execution_state: TxStepExecutionState,
    /// Forward failure summary when the step failed.
    pub failure_reason: Option<String>,
    /// Concrete child step results produced by the forward operation.
    #[serde(default)]
    pub forward_operation_steps: Vec<TxOperationStepResult>,
    /// Final rollback state related to this step.
    pub rollback_state: TxStepRollbackState,
    /// Rollback operation summary associated with this step, if any.
    pub rollback_operation_summary: Option<String>,
    /// Rollback failure or skip reason, if any.
    pub rollback_reason: Option<String>,
    /// Concrete child step results produced by the rollback operation.
    #[serde(default)]
    pub rollback_operation_steps: Vec<TxOperationStepResult>,
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
    /// Number of rollback operations attempted.
    pub rollback_steps: usize,
    /// First-level failure summary for the forward phase.
    pub failure_reason: Option<String>,
    /// Rollback phase errors (can contain multiple entries).
    pub rollback_errors: Vec<String>,
    /// Whole-resource rollback summary when a block-level rollback operation ran.
    pub block_rollback_operation_summary: Option<String>,
    /// Concrete child step results produced by a whole-resource rollback operation.
    #[serde(default)]
    pub block_rollback_steps: Vec<TxOperationStepResult>,
    /// Per-step execution and rollback details in block order.
    #[serde(default)]
    pub step_results: Vec<TxStepResult>,
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

impl TxStep {
    /// Build a transaction step from any supported session operation.
    pub fn new<T>(run: T) -> Self
    where
        T: Into<SessionOperation>,
    {
        Self {
            run: run.into(),
            rollback: None,
            rollback_on_failure: false,
        }
    }

    /// Attach an optional rollback operation to the step.
    pub fn with_rollback<T>(mut self, rollback: T) -> Self
    where
        T: Into<SessionOperation>,
    {
        self.rollback = Some(rollback.into());
        self
    }

    /// Control whether a failed forward step should run its own rollback.
    pub fn with_rollback_on_failure(mut self, rollback_on_failure: bool) -> Self {
        self.rollback_on_failure = rollback_on_failure;
        self
    }

    pub(crate) fn rollback_operation(&self) -> Option<&SessionOperation> {
        self.rollback.as_ref()
    }
}

impl TxStepResult {
    pub fn from_step(step_index: usize, step: &TxStep) -> Result<Self, ConnectError> {
        let summary = step.run.summary()?;
        Ok(Self {
            step_index,
            mode: summary.mode,
            operation_summary: summary.description,
            execution_state: TxStepExecutionState::NotRun,
            failure_reason: None,
            forward_operation_steps: Vec::new(),
            rollback_state: TxStepRollbackState::NotNeeded,
            rollback_operation_summary: None,
            rollback_reason: None,
            rollback_operation_steps: Vec::new(),
        })
    }
}

impl SessionOperation {
    pub fn to_command_flow(&self) -> Result<CommandFlow, ConnectError> {
        match self {
            SessionOperation::Command(command) => {
                validate_command(command, "session operation command")?;
                Ok(CommandFlow::new(vec![command.clone()]))
            }
            SessionOperation::Flow(flow) => {
                validate_command_flow(flow, "session operation flow")?;
                Ok(flow.clone())
            }
            SessionOperation::Template { template, runtime } => {
                let flow = template.to_command_flow(runtime)?;
                validate_command_flow(&flow, "session operation template")?;
                Ok(flow)
            }
        }
    }

    pub(crate) fn summary_impl(&self) -> Result<SessionOperationSummary, ConnectError> {
        match self {
            SessionOperation::Command(command) => Ok(SessionOperationSummary {
                kind: "command".to_string(),
                mode: command.mode.clone(),
                description: command.command.clone(),
                step_count: 1,
            }),
            SessionOperation::Flow(flow) => {
                validate_command_flow(flow, "session operation flow")?;
                let (mode, description) = summarize_command_flow(flow, None);
                Ok(SessionOperationSummary {
                    kind: "flow".to_string(),
                    mode,
                    description,
                    step_count: flow.steps.len(),
                })
            }
            SessionOperation::Template { template, runtime } => {
                let flow = template.to_command_flow(runtime)?;
                validate_command_flow(&flow, "session operation template")?;
                let (mode, description) =
                    summarize_command_flow(&flow, Some(template.name.as_str()));
                Ok(SessionOperationSummary {
                    kind: "template".to_string(),
                    mode,
                    description,
                    step_count: flow.steps.len(),
                })
            }
        }
    }

    pub(crate) fn display_summary(&self) -> Result<(String, String), ConnectError> {
        let summary = self.summary_impl()?;
        Ok((summary.mode, summary.description))
    }

    pub(crate) fn validate(&self, context: &str) -> Result<(), ConnectError> {
        match self {
            SessionOperation::Command(command) => validate_command(command, context),
            SessionOperation::Flow(flow) => validate_command_flow(flow, context),
            SessionOperation::Template { template, runtime } => {
                let flow = template.to_command_flow(runtime)?;
                validate_command_flow(&flow, context)
            }
        }
    }
}

fn validate_command(command: &Command, context: &str) -> Result<(), ConnectError> {
    if command.mode.trim().is_empty() {
        return Err(ConnectError::InvalidTransaction(format!(
            "{context}: command mode is empty"
        )));
    }
    if command.command.trim().is_empty() {
        return Err(ConnectError::InvalidTransaction(format!(
            "{context}: command text is empty"
        )));
    }
    Ok(())
}

fn validate_command_flow(flow: &CommandFlow, context: &str) -> Result<(), ConnectError> {
    if flow.steps.is_empty() {
        return Err(ConnectError::InvalidTransaction(format!(
            "{context}: flow has no steps"
        )));
    }

    for (index, command) in flow.steps.iter().enumerate() {
        validate_command(command, &format!("{context}: flow step[{index}]"))?;
    }

    Ok(())
}

fn summarize_command_flow(flow: &CommandFlow, template_name: Option<&str>) -> (String, String) {
    let first_mode = flow
        .steps
        .first()
        .map(|step| step.mode.clone())
        .unwrap_or_default();

    if flow.steps.len() == 1 {
        let command = flow
            .steps
            .first()
            .map(|step| step.command.clone())
            .unwrap_or_default();
        return (first_mode, command);
    }

    let label = match template_name {
        Some(name) => format!("<template:{name} {} steps>", flow.steps.len()),
        None => format!("<flow:{} steps>", flow.steps.len()),
    };
    (first_mode, label)
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
    if let Some(result) = failed_block_result {
        if result.rollback_attempted {
            return (
                true,
                result.rollback_succeeded,
                result.rollback_errors.clone(),
            );
        }
        if !result.rollback_errors.is_empty() {
            return (false, false, result.rollback_errors.clone());
        }
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
            step.run.validate(&format!("step[{i}] forward operation"))?;
            if let Some(rollback) = step.rollback.as_ref() {
                rollback.validate(&format!("step[{i}] rollback operation"))?;
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
                    rollback,
                    trigger_step_index,
                },
            ) => {
                rollback.validate("whole_resource rollback operation")?;
                if *trigger_step_index >= self.steps.len() {
                    return Err(ConnectError::InvalidTransaction(format!(
                        "whole_resource trigger_step_index out of range: {}",
                        trigger_step_index
                    )));
                }
            }
            (CommandBlockKind::Config, RollbackPolicy::PerStep) => {}
        }

        Ok(())
    }

    pub fn plan_rollback(
        &self,
        executed_step_indices: &[usize],
        failed_step_index: Option<usize>,
    ) -> Result<Vec<PlannedRollback>, ConnectError> {
        match &self.rollback_policy {
            RollbackPolicy::None => Ok(Vec::new()),
            RollbackPolicy::WholeResource {
                rollback,
                trigger_step_index,
            } => {
                if executed_step_indices.contains(trigger_step_index) {
                    Ok(vec![PlannedRollback {
                        step_index: None,
                        operation: rollback.as_ref().clone(),
                    }])
                } else {
                    Ok(Vec::new())
                }
            }
            RollbackPolicy::PerStep => {
                let mut commands = Vec::new();
                if let Some(failed_idx) = failed_step_index {
                    let failed_step = self.steps.get(failed_idx).ok_or_else(|| {
                        ConnectError::InvalidTransaction(format!(
                            "failed step index out of range: {failed_idx}"
                        ))
                    })?;
                    if failed_step.rollback_on_failure
                        && let Some(rollback) = failed_step.rollback_operation()
                    {
                        commands.push(PlannedRollback {
                            step_index: Some(failed_idx),
                            operation: rollback.clone(),
                        });
                    }
                }
                // Roll back in reverse execution order to mirror stack unwind behavior.
                for idx in executed_step_indices.iter().rev() {
                    let step = self.steps.get(*idx).ok_or_else(|| {
                        ConnectError::InvalidTransaction(format!(
                            "executed step index out of range: {idx}"
                        ))
                    })?;
                    if let Some(rollback) = step.rollback_operation() {
                        commands.push(PlannedRollback {
                            step_index: Some(*idx),
                            operation: rollback.clone(),
                        });
                    }
                }
                Ok(commands)
            }
        }
    }

    pub fn explain_missing_rollback_plan(
        &self,
        executed_step_indices: &[usize],
        failed_step_index: Option<usize>,
    ) -> Vec<String> {
        match &self.rollback_policy {
            RollbackPolicy::None => {
                vec!["rollback not configured for this block".to_string()]
            }
            RollbackPolicy::WholeResource {
                trigger_step_index, ..
            } => vec![format!(
                "whole_resource rollback skipped: trigger_step_index={} was not executed successfully",
                trigger_step_index
            )],
            RollbackPolicy::PerStep => {
                let mut reasons = Vec::new();

                if let Some(failed_idx) = failed_step_index
                    && let Some(step) = self.steps.get(failed_idx)
                {
                    if !step.rollback_on_failure {
                        reasons.push(format!(
                            "step[{failed_idx}] rollback skipped: rollback_on_failure=false"
                        ));
                    } else if step.rollback_operation().is_none() {
                        reasons.push(format!(
                            "step[{failed_idx}] rollback skipped: rollback operation is missing"
                        ));
                    }
                }

                for idx in executed_step_indices.iter().rev() {
                    if let Some(step) = self.steps.get(*idx)
                        && step.rollback_operation().is_none()
                    {
                        reasons.push(format!(
                            "step[{idx}] rollback skipped: rollback operation is missing"
                        ));
                    }
                }

                if reasons.is_empty() {
                    reasons.push(
                        "rollback not attempted: no per-step rollback operations were planned"
                            .to_string(),
                    );
                }

                reasons
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
            block_rollback_operation_summary: None,
            block_rollback_steps: Vec::new(),
            step_results: Vec::new(),
        }
    }

    pub fn with_step_results(mut self, step_results: Vec<TxStepResult>) -> Self {
        self.step_results = step_results;
        self
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

    fn command(mode: &str, command: &str) -> Command {
        Command {
            mode: mode.to_string(),
            command: command.to_string(),
            ..Command::default()
        }
    }

    fn per_step_block() -> TxBlock {
        TxBlock {
            name: "addr-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![
                TxStep::new(command("Config", "set addr 1"))
                    .with_rollback(command("Config", "unset addr 1")),
                TxStep::new(command("Config", "set addr 2"))
                    .with_rollback(command("Config", "unset addr 2")),
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
        let plan = block.plan_rollback(&[0, 1], None).expect("plan rollback");
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].step_index, Some(1));
        assert_eq!(
            plan[0].operation.summary().expect("summary").description,
            "unset addr 2"
        );
        assert_eq!(plan[1].step_index, Some(0));
        assert_eq!(
            plan[1].operation.summary().expect("summary").description,
            "unset addr 1"
        );
    }

    #[test]
    fn whole_resource_plan_is_single_operation() {
        let block = TxBlock {
            name: "addr-create".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                rollback: Box::new(
                    Command {
                        timeout: Some(30),
                        ..command("Config", "no address-object A")
                    }
                    .into(),
                ),
                trigger_step_index: 0,
            },
            steps: vec![TxStep::new(command("Config", "address-object A"))],
            fail_fast: true,
        };
        let plan = block.plan_rollback(&[0], None).expect("plan rollback");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].step_index, None);
        assert_eq!(
            plan[0].operation.summary().expect("summary").description,
            "no address-object A"
        );
    }

    #[test]
    fn whole_resource_plan_requires_trigger_step_success() {
        let block = TxBlock {
            name: "addr-create".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                rollback: Box::new(
                    Command {
                        timeout: Some(30),
                        ..command("Config", "no address-object A")
                    }
                    .into(),
                ),
                trigger_step_index: 0,
            },
            steps: vec![TxStep::new(command("Config", "address-object A"))],
            fail_fast: true,
        };

        let plan = block.plan_rollback(&[], Some(0)).expect("plan rollback");
        assert!(plan.is_empty());
    }

    #[test]
    fn whole_resource_plan_supports_custom_trigger_step() {
        let block = TxBlock {
            name: "policy-create".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                rollback: Box::new(command("Config", "delete policy P1").into()),
                trigger_step_index: 1,
            },
            steps: vec![
                TxStep::new(command("Config", "set addr A")),
                TxStep::new(command("Config", "set policy P1")),
            ],
            fail_fast: true,
        };

        let before_trigger = block.plan_rollback(&[0], Some(1)).expect("plan rollback");
        assert!(before_trigger.is_empty());

        let after_trigger = block
            .plan_rollback(&[0, 1], Some(1))
            .expect("plan rollback");
        assert_eq!(after_trigger.len(), 1);
        assert_eq!(
            after_trigger[0]
                .operation
                .summary()
                .expect("summary")
                .description,
            "delete policy P1"
        );
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
            steps: vec![TxStep::new(command("", "set x"))],
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
    fn per_step_rollback_plan_skips_steps_without_rollback_operation() {
        let block = TxBlock {
            name: "addr-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![
                TxStep::new(command("Config", "set addr 1"))
                    .with_rollback(command("Config", "unset addr 1")),
                TxStep::new(command("Config", "set addr 2")),
            ],
            fail_fast: true,
        };
        let plan = block.plan_rollback(&[0, 1], None).expect("plan rollback");
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].operation.summary().expect("summary").description,
            "unset addr 1"
        );
    }

    #[test]
    fn validation_rejects_empty_rollback_operation() {
        let block = TxBlock {
            name: "addr-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![
                TxStep::new(command("Config", "set addr 1")).with_rollback(command("Config", "")),
            ],
            fail_fast: true,
        };

        let err = block.validate().expect_err("empty rollback must fail");
        assert!(matches!(err, ConnectError::InvalidTransaction(_)));
        assert!(err.to_string().contains("rollback operation"));
    }

    #[test]
    fn per_step_plan_can_include_failed_step_rollback_when_enabled() {
        let block = TxBlock {
            name: "obj-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![
                TxStep::new(command("Config", "set a")).with_rollback(command("Config", "unset a")),
                TxStep::new(command("Config", "set b"))
                    .with_rollback(command("Config", "unset b"))
                    .with_rollback_on_failure(true),
            ],
            fail_fast: true,
        };

        let plan = block.plan_rollback(&[0], Some(1)).expect("plan rollback");
        assert_eq!(plan.len(), 2);
        assert_eq!(
            plan[0].operation.summary().expect("summary").description,
            "unset b"
        );
        assert_eq!(
            plan[1].operation.summary().expect("summary").description,
            "unset a"
        );
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
            rollback_errors: vec!["undo operation failed".to_string()],
            block_rollback_operation_summary: None,
            block_rollback_steps: Vec::new(),
            step_results: Vec::new(),
        };
        let (attempted, succeeded, errors) = failed_block_rollback_summary(Some(&failed));
        assert!(attempted);
        assert!(!succeeded);
        assert_eq!(errors, vec!["undo operation failed".to_string()]);
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
            block_rollback_operation_summary: None,
            block_rollback_steps: Vec::new(),
            step_results: Vec::new(),
        };
        let (attempted, succeeded, errors) = failed_block_rollback_summary(Some(&failed));
        assert!(!attempted);
        assert!(!succeeded);
        assert_eq!(errors, vec!["ignored".to_string()]);
    }

    #[test]
    fn missing_rollback_plan_reasons_explain_missing_commands() {
        let block = TxBlock {
            name: "addr-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![
                TxStep::new(command("Config", "set addr 1")),
                TxStep::new(command("Config", "set addr 2")),
            ],
            fail_fast: true,
        };

        let reasons = block.explain_missing_rollback_plan(&[0], Some(1));
        assert_eq!(
            reasons,
            vec![
                "step[1] rollback skipped: rollback_on_failure=false".to_string(),
                "step[0] rollback skipped: rollback operation is missing".to_string()
            ]
        );
    }

    #[test]
    fn tx_step_result_is_initialized_from_step() {
        let step = TxStep::new(Command {
            timeout: Some(30),
            ..command("Config", "set addr 1")
        })
        .with_rollback(command("Config", "unset addr 1"));

        let result = TxStepResult::from_step(3, &step).expect("step result");
        assert_eq!(result.step_index, 3);
        assert_eq!(result.mode, "Config");
        assert_eq!(result.operation_summary, "set addr 1");
        assert_eq!(result.execution_state, TxStepExecutionState::NotRun);
        assert_eq!(result.rollback_state, TxStepRollbackState::NotNeeded);
        assert!(result.failure_reason.is_none());
        assert!(result.rollback_operation_summary.is_none());
        assert!(result.rollback_reason.is_none());
    }

    #[test]
    fn tx_step_result_summarizes_flow_operations() {
        let flow = CommandFlow::new(vec![
            command("Enable", "terminal length 0"),
            command("Enable", "show version"),
        ]);
        let step = TxStep::new(flow);

        let result = TxStepResult::from_step(0, &step).expect("step result");
        assert_eq!(result.mode, "Enable");
        assert_eq!(result.operation_summary, "<flow:2 steps>");
    }
}
