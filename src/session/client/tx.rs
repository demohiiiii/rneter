use super::super::*;
use std::future::Future;
use std::pin::Pin;

pub(super) type OperationRunFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SessionOperationOutput, OperationRunError>> + Send + 'a>>;

#[derive(Debug)]
pub(crate) struct OperationRunError {
    pub error: ConnectError,
    pub partial_output: SessionOperationOutput,
}

impl OperationRunError {
    pub(crate) fn new(error: ConnectError, partial_output: SessionOperationOutput) -> Self {
        Self {
            error,
            partial_output,
        }
    }

    pub(crate) fn into_parts(self) -> (ConnectError, SessionOperationOutput) {
        (self.error, self.partial_output)
    }
}

impl std::fmt::Display for OperationRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for OperationRunError {}

impl From<ConnectError> for OperationRunError {
    fn from(error: ConnectError) -> Self {
        Self::new(
            error,
            SessionOperationOutput {
                success: false,
                steps: Vec::new(),
            },
        )
    }
}

pub(super) trait TxCommandRunner {
    fn recorder(&self) -> Option<&SessionRecorder>;

    fn run_operation<'a>(
        &'a mut self,
        operation: &'a SessionOperation,
        sys: Option<&'a String>,
    ) -> OperationRunFuture<'a>;
}

fn init_step_results(block: &TxBlock) -> Result<Vec<TxStepResult>, ConnectError> {
    block
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| TxStepResult::from_step(idx, step))
        .collect()
}

fn attempted_step_indices(executed_indices: &[usize], failed_step_indices: &[usize]) -> Vec<usize> {
    let mut indices = Vec::with_capacity(executed_indices.len() + failed_step_indices.len());
    for idx in executed_indices
        .iter()
        .copied()
        .chain(failed_step_indices.iter().copied())
    {
        if !indices.contains(&idx) {
            indices.push(idx);
        }
    }
    indices
}

fn annotate_step_results_for_rollback(
    block: &TxBlock,
    step_results: &mut [TxStepResult],
    executed_indices: &[usize],
    failed_step_indices: &[usize],
    rollback_failed_step: Option<usize>,
) -> Result<(), ConnectError> {
    match &block.rollback_policy {
        RollbackPolicy::None => {}
        RollbackPolicy::WholeResource { rollback, .. } => {
            let (_, rollback_operation_summary) = rollback.display_summary()?;
            for idx in attempted_step_indices(executed_indices, failed_step_indices) {
                if let Some(step_result) = step_results.get_mut(idx) {
                    step_result.rollback_operation_summary =
                        Some(rollback_operation_summary.clone());
                }
            }
        }
        RollbackPolicy::PerStep => {
            for &idx in failed_step_indices {
                if let (Some(step), Some(step_result)) =
                    (block.steps.get(idx), step_results.get_mut(idx))
                {
                    step_result.rollback_operation_summary = step
                        .rollback_operation()
                        .map(|rollback| rollback.display_summary())
                        .transpose()?
                        .map(|(_, rollback_operation_summary)| rollback_operation_summary);
                    if step.rollback_on_failure {
                        if step.rollback_operation().is_none() {
                            step_result.rollback_state = TxStepRollbackState::Skipped;
                            step_result.rollback_reason =
                                Some("rollback operation is missing".to_string());
                        } else if Some(idx) != rollback_failed_step {
                            step_result.rollback_state = TxStepRollbackState::Skipped;
                            step_result.rollback_reason = Some(
                                "failed step was not selected for rollback planning".to_string(),
                            );
                        }
                    } else {
                        step_result.rollback_state = TxStepRollbackState::Skipped;
                        step_result.rollback_reason = Some("rollback_on_failure=false".to_string());
                    }
                }
            }

            for &idx in executed_indices {
                if let (Some(step), Some(step_result)) =
                    (block.steps.get(idx), step_results.get_mut(idx))
                {
                    if let Some(rollback) = step.rollback_operation() {
                        let (_, rollback_operation_summary) = rollback.display_summary()?;
                        step_result.rollback_operation_summary = Some(rollback_operation_summary);
                    } else {
                        step_result.rollback_state = TxStepRollbackState::Skipped;
                        step_result.rollback_reason =
                            Some("rollback operation is missing".to_string());
                    }
                }
            }
        }
    }

    Ok(())
}

fn apply_step_rollback_outcome(
    step_results: &mut [TxStepResult],
    step_index: usize,
    operation_summary: &str,
    operation_steps: Vec<TxOperationStepResult>,
    rollback_state: TxStepRollbackState,
    rollback_reason: Option<String>,
) {
    if let Some(step_result) = step_results.get_mut(step_index) {
        step_result.rollback_operation_summary = Some(operation_summary.to_string());
        step_result.rollback_operation_steps = operation_steps;
        step_result.rollback_state = rollback_state;
        step_result.rollback_reason = rollback_reason;
    }
}

fn apply_block_rollback_outcome(
    step_results: &mut [TxStepResult],
    affected_step_indices: &[usize],
    operation_summary: &str,
    rollback_state: TxStepRollbackState,
    rollback_reason: Option<String>,
) {
    for &idx in affected_step_indices {
        if let Some(step_result) = step_results.get_mut(idx) {
            step_result.rollback_operation_summary = Some(operation_summary.to_string());
            step_result.rollback_state = rollback_state;
            step_result.rollback_reason = rollback_reason.clone();
        }
    }
}

fn operation_failure_output(output: &SessionOperationOutput) -> String {
    output
        .steps
        .last()
        .map(|last| {
            if last.content.is_empty() {
                last.all.clone()
            } else {
                last.content.clone()
            }
        })
        .unwrap_or_default()
}

fn operation_step_results(output: &SessionOperationOutput) -> Vec<TxOperationStepResult> {
    output
        .steps
        .iter()
        .cloned()
        .map(TxOperationStepResult::from)
        .collect()
}

fn recording_operation_steps(steps: &[TxOperationStepResult]) -> Vec<SessionOperationStepOutput> {
    steps
        .iter()
        .cloned()
        .map(SessionOperationStepOutput::from)
        .collect()
}

pub(super) async fn rollback_committed_block_with_runner<R: TxCommandRunner + ?Sized>(
    runner: &mut R,
    block: &TxBlock,
    sys: Option<&String>,
    result: &mut TxResult,
) -> Result<(), ConnectError> {
    if block.kind == CommandBlockKind::Show {
        return Ok(());
    }
    let executed = (0..block.steps.len()).collect::<Vec<_>>();
    let failed = Vec::new();
    annotate_step_results_for_rollback(block, &mut result.step_results, &executed, &failed, None)?;
    let plan = block.plan_rollback(&executed, None)?;
    result.rollback_attempted = !plan.is_empty();
    result.rollback_succeeded = result.rollback_attempted;
    result.rollback_steps = 0;
    result.rollback_errors.clear();
    result.block_rollback_operation_summary = None;
    result.block_rollback_steps.clear();
    let block_level_indices = attempted_step_indices(&executed, &failed);
    if !result.rollback_attempted {
        result.rollback_succeeded = false;
        result
            .rollback_errors
            .extend(block.explain_missing_rollback_plan(&executed, None));
        if let RollbackPolicy::WholeResource { rollback, .. } = &block.rollback_policy {
            let (_, rollback_operation_summary) = rollback.display_summary()?;
            result.block_rollback_operation_summary = Some(rollback_operation_summary.clone());
            apply_block_rollback_outcome(
                &mut result.step_results,
                &block_level_indices,
                &rollback_operation_summary,
                TxStepRollbackState::BlockSkipped,
                Some(result.rollback_errors.join("; ")),
            );
        }
        return Ok(());
    }
    if let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxRollbackStarted {
            block_name: block.name.clone(),
        });
    }
    for rollback in plan {
        let (rollback_mode, rollback_operation_summary) = rollback.operation.display_summary()?;
        result.rollback_steps += 1;
        match runner.run_operation(&rollback.operation, sys).await {
            Ok(output) if output.success => {
                let rollback_steps = operation_step_results(&output);
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut result.step_results,
                        step_idx,
                        &rollback_operation_summary,
                        rollback_steps.clone(),
                        TxStepRollbackState::Succeeded,
                        None,
                    );
                } else {
                    result.block_rollback_operation_summary =
                        Some(rollback_operation_summary.clone());
                    result.block_rollback_steps = rollback_steps.clone();
                    apply_block_rollback_outcome(
                        &mut result.step_results,
                        &block_level_indices,
                        &rollback_operation_summary,
                        TxStepRollbackState::BlockSucceeded,
                        None,
                    );
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepSucceeded {
                        block_name: block.name.clone(),
                        step_index: rollback.step_index,
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                        operation_steps: recording_operation_steps(&rollback_steps),
                    });
                }
            }
            Ok(output) => {
                let rollback_steps = operation_step_results(&output);
                result.rollback_succeeded = false;
                let reason = format!(
                    "workflow rollback operation failed for block '{}': '{}' output='{}'",
                    block.name,
                    rollback_operation_summary,
                    operation_failure_output(&output)
                );
                result.rollback_errors.push(reason.clone());
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut result.step_results,
                        step_idx,
                        &rollback_operation_summary,
                        rollback_steps.clone(),
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
                    result.block_rollback_operation_summary =
                        Some(rollback_operation_summary.clone());
                    result.block_rollback_steps = rollback_steps.clone();
                    apply_block_rollback_outcome(
                        &mut result.step_results,
                        &block_level_indices,
                        &rollback_operation_summary,
                        TxStepRollbackState::BlockFailed,
                        Some(reason.clone()),
                    );
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: rollback.step_index,
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                        operation_steps: recording_operation_steps(&rollback_steps),
                        reason,
                    });
                }
            }
            Err(run_err) => {
                let (err, partial_output) = run_err.into_parts();
                let rollback_steps = operation_step_results(&partial_output);
                result.rollback_succeeded = false;
                let reason = format!(
                    "workflow rollback operation error for block '{}': '{}' err={}",
                    block.name, rollback_operation_summary, err
                );
                result.rollback_errors.push(reason.clone());
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut result.step_results,
                        step_idx,
                        &rollback_operation_summary,
                        rollback_steps.clone(),
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
                    result.block_rollback_operation_summary =
                        Some(rollback_operation_summary.clone());
                    result.block_rollback_steps = rollback_steps.clone();
                    apply_block_rollback_outcome(
                        &mut result.step_results,
                        &block_level_indices,
                        &rollback_operation_summary,
                        TxStepRollbackState::BlockFailed,
                        Some(reason.clone()),
                    );
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: rollback.step_index,
                        mode: rollback_mode,
                        operation_summary: rollback_operation_summary,
                        operation_steps: recording_operation_steps(&rollback_steps),
                        reason,
                    });
                }
            }
        }
    }

    Ok(())
}

pub(super) async fn execute_tx_block_with_runner<R: TxCommandRunner + ?Sized>(
    runner: &mut R,
    block: &TxBlock,
    sys: Option<&String>,
) -> Result<TxResult, ConnectError> {
    block.validate()?;
    if let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxBlockStarted {
            block_name: block.name.clone(),
            block_kind: block.kind,
        });
    }

    let mut executed_indices = Vec::new();
    let mut failed_step_indices = Vec::new();
    let mut failure_reason = None;
    let mut failed_step = None;
    let mut rollback_failed_step = None;
    let mut step_results = init_step_results(block)?;

    for (idx, step) in block.steps.iter().enumerate() {
        let (step_mode, step_operation_summary) = step.run.display_summary()?;
        match runner.run_operation(&step.run, sys).await {
            Ok(output) if output.success => {
                let forward_steps = operation_step_results(&output);
                executed_indices.push(idx);
                if let Some(step_result) = step_results.get_mut(idx) {
                    step_result.execution_state = TxStepExecutionState::Succeeded;
                    step_result.forward_operation_steps = forward_steps.clone();
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepSucceeded {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step_mode.clone(),
                        operation_summary: step_operation_summary.clone(),
                        operation_steps: recording_operation_steps(&forward_steps),
                    });
                }
            }
            Ok(output) => {
                let forward_steps = operation_step_results(&output);
                let reason = format!(
                    "step[{idx}] operation failed: '{}' output='{}'",
                    step_operation_summary,
                    operation_failure_output(&output)
                );
                if failed_step.is_none() {
                    failed_step = Some(idx);
                    failure_reason = Some(reason.clone());
                }
                rollback_failed_step = Some(idx);
                failed_step_indices.push(idx);
                if let Some(step_result) = step_results.get_mut(idx) {
                    step_result.execution_state = TxStepExecutionState::Failed;
                    step_result.failure_reason = Some(reason.clone());
                    step_result.forward_operation_steps = forward_steps.clone();
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepFailed {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step_mode.clone(),
                        operation_summary: step_operation_summary.clone(),
                        operation_steps: recording_operation_steps(&forward_steps),
                        reason,
                    });
                }
                if block.fail_fast {
                    break;
                }
            }
            Err(run_err) => {
                let (err, partial_output) = run_err.into_parts();
                let forward_steps = operation_step_results(&partial_output);
                let reason = format!("step[{idx}] operation error: {err}");
                if failed_step.is_none() {
                    failed_step = Some(idx);
                    failure_reason = Some(reason.clone());
                }
                rollback_failed_step = Some(idx);
                failed_step_indices.push(idx);
                if let Some(step_result) = step_results.get_mut(idx) {
                    step_result.execution_state = TxStepExecutionState::Failed;
                    step_result.failure_reason = Some(reason.clone());
                    step_result.forward_operation_steps = forward_steps.clone();
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepFailed {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step_mode,
                        operation_summary: step_operation_summary,
                        operation_steps: recording_operation_steps(&forward_steps),
                        reason,
                    });
                }
                if block.fail_fast {
                    break;
                }
            }
        }
    }

    if failed_step.is_none() {
        let result = TxResult::committed(block.name.clone(), executed_indices.len())
            .with_step_results(step_results);
        if let Some(recorder) = runner.recorder() {
            let _ = recorder.record_event(SessionEvent::TxBlockFinished {
                block_name: block.name.clone(),
                committed: true,
                rollback_attempted: false,
                rollback_succeeded: false,
            });
        }
        return Ok(result);
    }

    if block.kind == CommandBlockKind::Show {
        let result = TxResult {
            block_name: block.name.clone(),
            committed: false,
            failed_step,
            executed_steps: executed_indices.len(),
            rollback_attempted: false,
            rollback_succeeded: false,
            rollback_steps: 0,
            failure_reason,
            rollback_errors: Vec::new(),
            block_rollback_operation_summary: None,
            block_rollback_steps: Vec::new(),
            step_results,
        };
        if let Some(recorder) = runner.recorder() {
            let _ = recorder.record_event(SessionEvent::TxBlockFinished {
                block_name: block.name.clone(),
                committed: false,
                rollback_attempted: false,
                rollback_succeeded: false,
            });
        }
        return Ok(result);
    }

    annotate_step_results_for_rollback(
        block,
        &mut step_results,
        &executed_indices,
        &failed_step_indices,
        rollback_failed_step,
    )?;
    let rollback_plan = block.plan_rollback(&executed_indices, rollback_failed_step)?;
    let rollback_attempted = !rollback_plan.is_empty();
    if rollback_attempted && let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxRollbackStarted {
            block_name: block.name.clone(),
        });
    }
    let mut rollback_succeeded = rollback_attempted;
    let mut rollback_errors = Vec::new();
    let mut rollback_steps = 0;
    let mut block_rollback_operation_summary = None;
    let mut block_rollback_steps = Vec::new();
    let missing_rollback_reasons =
        block.explain_missing_rollback_plan(&executed_indices, rollback_failed_step);
    let block_level_indices = attempted_step_indices(&executed_indices, &failed_step_indices);
    if !rollback_attempted {
        rollback_errors.extend(missing_rollback_reasons.clone());
        rollback_errors.push(format!(
            "forward_failure={}",
            failure_reason
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        ));
        if let RollbackPolicy::WholeResource { rollback, .. } = &block.rollback_policy {
            let (_, rollback_operation_summary) = rollback.display_summary()?;
            block_rollback_operation_summary = Some(rollback_operation_summary.clone());
            apply_block_rollback_outcome(
                &mut step_results,
                &block_level_indices,
                &rollback_operation_summary,
                TxStepRollbackState::BlockSkipped,
                Some(missing_rollback_reasons.join("; ")),
            );
        }
    }

    for rollback in rollback_plan {
        let (rollback_mode, rollback_operation_summary) = rollback.operation.display_summary()?;
        rollback_steps += 1;
        match runner.run_operation(&rollback.operation, sys).await {
            Ok(output) if output.success => {
                let rollback_steps_output = operation_step_results(&output);
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut step_results,
                        step_idx,
                        &rollback_operation_summary,
                        rollback_steps_output.clone(),
                        TxStepRollbackState::Succeeded,
                        None,
                    );
                } else {
                    block_rollback_operation_summary = Some(rollback_operation_summary.clone());
                    block_rollback_steps = rollback_steps_output.clone();
                    apply_block_rollback_outcome(
                        &mut step_results,
                        &block_level_indices,
                        &rollback_operation_summary,
                        TxStepRollbackState::BlockSucceeded,
                        None,
                    );
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepSucceeded {
                        block_name: block.name.clone(),
                        step_index: rollback.step_index,
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                        operation_steps: recording_operation_steps(&rollback_steps_output),
                    });
                }
            }
            Ok(output) => {
                let rollback_steps_output = operation_step_results(&output);
                rollback_succeeded = false;
                let reason = format!(
                    "rollback operation failed: '{}' output='{}'",
                    rollback_operation_summary,
                    operation_failure_output(&output)
                );
                rollback_errors.push(reason.clone());
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut step_results,
                        step_idx,
                        &rollback_operation_summary,
                        rollback_steps_output.clone(),
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
                    block_rollback_operation_summary = Some(rollback_operation_summary.clone());
                    block_rollback_steps = rollback_steps_output.clone();
                    apply_block_rollback_outcome(
                        &mut step_results,
                        &block_level_indices,
                        &rollback_operation_summary,
                        TxStepRollbackState::BlockFailed,
                        Some(reason.clone()),
                    );
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: rollback.step_index,
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                        operation_steps: recording_operation_steps(&rollback_steps_output),
                        reason,
                    });
                }
            }
            Err(run_err) => {
                let (err, partial_output) = run_err.into_parts();
                let rollback_steps_output = operation_step_results(&partial_output);
                rollback_succeeded = false;
                let reason = format!(
                    "rollback operation error: '{}' err={}",
                    rollback_operation_summary, err
                );
                rollback_errors.push(reason.clone());
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut step_results,
                        step_idx,
                        &rollback_operation_summary,
                        rollback_steps_output.clone(),
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
                    block_rollback_operation_summary = Some(rollback_operation_summary.clone());
                    block_rollback_steps = rollback_steps_output.clone();
                    apply_block_rollback_outcome(
                        &mut step_results,
                        &block_level_indices,
                        &rollback_operation_summary,
                        TxStepRollbackState::BlockFailed,
                        Some(reason.clone()),
                    );
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: rollback.step_index,
                        mode: rollback_mode,
                        operation_summary: rollback_operation_summary,
                        operation_steps: recording_operation_steps(&rollback_steps_output),
                        reason,
                    });
                }
            }
        }
    }

    let result = TxResult {
        block_name: block.name.clone(),
        committed: false,
        failed_step,
        executed_steps: executed_indices.len(),
        rollback_attempted,
        rollback_succeeded,
        rollback_steps,
        failure_reason,
        rollback_errors,
        block_rollback_operation_summary,
        block_rollback_steps,
        step_results,
    };

    if let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxBlockFinished {
            block_name: block.name.clone(),
            committed: false,
            rollback_attempted: result.rollback_attempted,
            rollback_succeeded: result.rollback_succeeded,
        });
    }

    Ok(result)
}

pub(super) async fn execute_tx_workflow_with_runner<R: TxCommandRunner + ?Sized>(
    runner: &mut R,
    workflow: &TxWorkflow,
    sys: Option<&String>,
) -> Result<TxWorkflowResult, ConnectError> {
    workflow.validate()?;
    if let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxWorkflowStarted {
            workflow_name: workflow.name.clone(),
            total_blocks: workflow.blocks.len(),
        });
    }

    let mut block_results = Vec::with_capacity(workflow.blocks.len());
    let mut committed_block_indices = Vec::new();
    let mut failed_block = None;

    for (idx, block) in workflow.blocks.iter().enumerate() {
        let result = execute_tx_block_with_runner(runner, block, sys).await?;
        let committed = result.committed;
        block_results.push(result);
        if committed {
            committed_block_indices.push(idx);
            continue;
        }
        failed_block = Some(idx);
        if workflow.fail_fast {
            break;
        }
    }

    if failed_block.is_none() {
        if let Some(recorder) = runner.recorder() {
            let _ = recorder.record_event(SessionEvent::TxWorkflowFinished {
                workflow_name: workflow.name.clone(),
                committed: true,
                rollback_attempted: false,
                rollback_succeeded: false,
            });
        }
        return Ok(TxWorkflowResult {
            workflow_name: workflow.name.clone(),
            committed: true,
            failed_block: None,
            block_results,
            rollback_attempted: false,
            rollback_succeeded: false,
            rollback_errors: Vec::new(),
        });
    }

    let failed_idx = failed_block.unwrap_or(0);
    let (mut rollback_attempted, mut rollback_succeeded, mut rollback_errors) =
        failed_block_rollback_summary(block_results.get(failed_idx));

    for block_idx in workflow_rollback_order(&committed_block_indices, failed_idx) {
        rollback_attempted = true;
        if let (Some(block), Some(block_result)) = (
            workflow.blocks.get(block_idx),
            block_results.get_mut(block_idx),
        ) {
            rollback_committed_block_with_runner(runner, block, sys, block_result).await?;
            if !block_result.rollback_succeeded {
                rollback_succeeded = false;
            }
            rollback_errors.extend(block_result.rollback_errors.clone());
        }
    }

    if let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxWorkflowFinished {
            workflow_name: workflow.name.clone(),
            committed: false,
            rollback_attempted,
            rollback_succeeded,
        });
    }

    Ok(TxWorkflowResult {
        workflow_name: workflow.name.clone(),
        committed: false,
        failed_block,
        block_results,
        rollback_attempted,
        rollback_succeeded,
        rollback_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct ScriptedOperation {
        command: String,
        mode: String,
        result: Result<SessionOperationOutput, OperationRunError>,
    }

    struct FakeRunner {
        scripted: VecDeque<ScriptedOperation>,
        recorder: Option<SessionRecorder>,
    }

    impl FakeRunner {
        fn new(scripted: Vec<ScriptedOperation>) -> Self {
            Self {
                scripted: scripted.into(),
                recorder: None,
            }
        }

        fn with_recorder(mut self, recorder: SessionRecorder) -> Self {
            self.recorder = Some(recorder);
            self
        }
    }

    impl TxCommandRunner for FakeRunner {
        fn recorder(&self) -> Option<&SessionRecorder> {
            self.recorder.as_ref()
        }

        fn run_operation<'a>(
            &'a mut self,
            operation: &'a SessionOperation,
            _sys: Option<&'a String>,
        ) -> OperationRunFuture<'a> {
            Box::pin(async move {
                let scripted = self.scripted.pop_front().ok_or_else(|| {
                    OperationRunError::from(ConnectError::InternalServerError(
                        "unexpected scripted command exhaustion".to_string(),
                    ))
                })?;
                let (mode, command) = operation.display_summary()?;
                assert_eq!(scripted.command, command);
                assert_eq!(scripted.mode, mode);
                scripted.result
            })
        }
    }

    fn ok_output(content: &str) -> Output {
        Output {
            success: true,
            exit_code: None,
            content: content.to_string(),
            all: content.to_string(),
            prompt: None,
        }
    }

    fn failed_output(content: &str) -> Output {
        Output {
            success: false,
            exit_code: None,
            content: content.to_string(),
            all: content.to_string(),
            prompt: None,
        }
    }

    fn step_output(
        step_index: usize,
        mode: &str,
        operation_summary: &str,
        output: Output,
    ) -> SessionOperationStepOutput {
        SessionOperationStepOutput {
            step_index,
            mode: mode.to_string(),
            operation_summary: operation_summary.to_string(),
            success: output.success,
            exit_code: output.exit_code,
            content: output.content,
            all: output.all,
            prompt: output.prompt,
        }
    }

    fn single_output(command: &str, mode: &str, output: Output) -> SessionOperationOutput {
        SessionOperationOutput {
            success: output.success,
            steps: vec![step_output(0, mode, command, output)],
        }
    }

    fn flow_output(steps: Vec<SessionOperationStepOutput>) -> SessionOperationOutput {
        let success = steps.iter().all(|step| step.success);
        SessionOperationOutput { success, steps }
    }

    fn partial_run_error(
        error: ConnectError,
        steps: Vec<SessionOperationStepOutput>,
    ) -> Result<SessionOperationOutput, OperationRunError> {
        Err(OperationRunError::new(
            error,
            SessionOperationOutput {
                success: false,
                steps,
            },
        ))
    }

    fn per_step_block(rollback_on_failure: bool) -> TxBlock {
        TxBlock {
            name: "addr-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::PerStep,
            steps: vec![
                TxStep::new(Command {
                    mode: "Config".to_string(),
                    command: "set addr 1".to_string(),
                    ..Command::default()
                })
                .with_rollback(Command {
                    mode: "Config".to_string(),
                    command: "unset addr 1".to_string(),
                    ..Command::default()
                }),
                TxStep::new(Command {
                    mode: "Config".to_string(),
                    command: "set addr 2".to_string(),
                    ..Command::default()
                })
                .with_rollback(Command {
                    mode: "Config".to_string(),
                    command: "unset addr 2".to_string(),
                    ..Command::default()
                })
                .with_rollback_on_failure(rollback_on_failure),
            ],
            fail_fast: true,
        }
    }

    #[tokio::test]
    async fn execute_tx_block_skips_failed_step_rollback_by_default() {
        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "set addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output("set addr 1", "Config", ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "set addr 2",
                    "Config",
                    failed_output("invalid input"),
                )),
            },
            ScriptedOperation {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "unset addr 1",
                    "Config",
                    ok_output("rollback ok"),
                )),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &per_step_block(false), None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(1));
        assert!(result.rollback_attempted);
        assert!(result.rollback_succeeded);
        assert_eq!(result.rollback_steps, 1);
        assert_eq!(result.step_results.len(), 2);
        assert_eq!(
            result.step_results[0].execution_state,
            TxStepExecutionState::Succeeded
        );
        assert_eq!(
            result.step_results[0].rollback_state,
            TxStepRollbackState::Succeeded
        );
        assert_eq!(
            result.step_results[1].execution_state,
            TxStepExecutionState::Failed
        );
        assert_eq!(
            result.step_results[1].rollback_state,
            TxStepRollbackState::Skipped
        );
        assert_eq!(
            result.step_results[1].rollback_reason.as_deref(),
            Some("rollback_on_failure=false")
        );
        assert_eq!(result.step_results[0].forward_operation_steps.len(), 1);
        assert_eq!(
            result.step_results[0].forward_operation_steps[0].operation_summary,
            "set addr 1"
        );
        assert_eq!(result.step_results[0].rollback_operation_steps.len(), 1);
        assert_eq!(
            result.step_results[0].rollback_operation_steps[0].operation_summary,
            "unset addr 1"
        );
        assert_eq!(result.step_results[1].forward_operation_steps.len(), 1);
        assert_eq!(
            result.step_results[1].forward_operation_steps[0].operation_summary,
            "set addr 2"
        );
        assert!(result.step_results[1].rollback_operation_steps.is_empty());
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_can_rollback_failed_step_when_enabled() {
        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "set addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output("set addr 1", "Config", ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "set addr 2",
                    "Config",
                    failed_output("invalid input"),
                )),
            },
            ScriptedOperation {
                command: "unset addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "unset addr 2",
                    "Config",
                    ok_output("failed-step rollback ok"),
                )),
            },
            ScriptedOperation {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "unset addr 1",
                    "Config",
                    ok_output("rollback ok"),
                )),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &per_step_block(true), None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(1));
        assert!(result.rollback_attempted);
        assert!(result.rollback_succeeded);
        assert_eq!(result.rollback_steps, 2);
        assert_eq!(
            result.step_results[0].rollback_state,
            TxStepRollbackState::Succeeded
        );
        assert_eq!(
            result.step_results[1].rollback_state,
            TxStepRollbackState::Succeeded
        );
        assert_eq!(
            result.step_results[0].rollback_operation_steps[0].operation_summary,
            "unset addr 1"
        );
        assert_eq!(
            result.step_results[1].rollback_operation_steps[0].operation_summary,
            "unset addr 2"
        );
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_whole_resource_waits_for_trigger_step() {
        let block = TxBlock {
            name: "policy-create".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                rollback: Box::new(
                    Command {
                        mode: "Config".to_string(),
                        command: "delete policy P1".to_string(),
                        ..Command::default()
                    }
                    .into(),
                ),
                trigger_step_index: 1,
            },
            steps: vec![
                TxStep::new(Command {
                    mode: "Config".to_string(),
                    command: "set addr A".to_string(),
                    ..Command::default()
                }),
                TxStep::new(Command {
                    mode: "Config".to_string(),
                    command: "set policy P1".to_string(),
                    ..Command::default()
                }),
            ],
            fail_fast: true,
        };
        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "set addr A".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output("set addr A", "Config", ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set policy P1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "set policy P1",
                    "Config",
                    failed_output("invalid input"),
                )),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(1));
        assert!(!result.rollback_attempted);
        assert!(!result.rollback_succeeded);
        assert_eq!(result.rollback_steps, 0);
        assert_eq!(result.rollback_errors.len(), 2);
        assert_eq!(
            result.block_rollback_operation_summary.as_deref(),
            Some("delete policy P1")
        );
        assert!(result.block_rollback_steps.is_empty());
        assert_eq!(
            result.step_results[0].rollback_state,
            TxStepRollbackState::BlockSkipped
        );
        assert_eq!(
            result.step_results[1].rollback_state,
            TxStepRollbackState::BlockSkipped
        );
        assert!(
            result.rollback_errors[0]
                .contains("trigger_step_index=1 was not executed successfully")
        );
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_workflow_updates_committed_block_step_results_after_global_rollback() {
        let workflow = TxWorkflow {
            name: "policy-publish".to_string(),
            blocks: vec![
                TxBlock {
                    name: "addr-create".to_string(),
                    kind: CommandBlockKind::Config,
                    rollback_policy: RollbackPolicy::PerStep,
                    steps: vec![
                        TxStep::new(Command {
                            mode: "Config".to_string(),
                            command: "set addr 1".to_string(),
                            ..Command::default()
                        })
                        .with_rollback(Command {
                            mode: "Config".to_string(),
                            command: "unset addr 1".to_string(),
                            ..Command::default()
                        }),
                    ],
                    fail_fast: true,
                },
                TxBlock {
                    name: "policy-create".to_string(),
                    kind: CommandBlockKind::Config,
                    rollback_policy: RollbackPolicy::PerStep,
                    steps: vec![
                        TxStep::new(Command {
                            mode: "Config".to_string(),
                            command: "set policy 1".to_string(),
                            ..Command::default()
                        })
                        .with_rollback(Command {
                            mode: "Config".to_string(),
                            command: "unset policy 1".to_string(),
                            ..Command::default()
                        })
                        .with_rollback_on_failure(true),
                    ],
                    fail_fast: true,
                },
            ],
            fail_fast: true,
        };

        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "set addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output("set addr 1", "Config", ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set policy 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "set policy 1",
                    "Config",
                    failed_output("invalid input"),
                )),
            },
            ScriptedOperation {
                command: "unset policy 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "unset policy 1",
                    "Config",
                    ok_output("rollback ok"),
                )),
            },
            ScriptedOperation {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "unset addr 1",
                    "Config",
                    ok_output("rollback ok"),
                )),
            },
        ]);

        let result = execute_tx_workflow_with_runner(&mut runner, &workflow, None)
            .await
            .expect("execute workflow");

        assert_eq!(result.failed_block, Some(1));
        assert!(result.rollback_attempted);
        assert!(result.rollback_succeeded);
        assert_eq!(result.block_results.len(), 2);
        assert!(result.block_results[0].committed);
        assert!(result.block_results[0].rollback_attempted);
        assert_eq!(result.block_results[0].rollback_steps, 1);
        assert_eq!(
            result.block_results[0].step_results[0]
                .forward_operation_steps
                .len(),
            1
        );
        assert_eq!(
            result.block_results[0].step_results[0].forward_operation_steps[0].operation_summary,
            "set addr 1"
        );
        assert_eq!(
            result.block_results[0].step_results[0]
                .rollback_operation_steps
                .len(),
            1
        );
        assert_eq!(
            result.block_results[0].step_results[0].rollback_operation_steps[0].operation_summary,
            "unset addr 1"
        );
        assert_eq!(
            result.block_results[0].step_results[0].rollback_state,
            TxStepRollbackState::Succeeded
        );
        assert_eq!(
            result.block_results[1].step_results[0]
                .forward_operation_steps
                .len(),
            1
        );
        assert_eq!(
            result.block_results[1].step_results[0].forward_operation_steps[0].operation_summary,
            "set policy 1"
        );
        assert_eq!(
            result.block_results[1].step_results[0].execution_state,
            TxStepExecutionState::Failed
        );
        assert_eq!(
            result.block_results[1].step_results[0].rollback_state,
            TxStepRollbackState::Succeeded
        );
        assert_eq!(
            result.block_results[1].step_results[0]
                .rollback_operation_steps
                .len(),
            1
        );
        assert_eq!(
            result.block_results[1].step_results[0].rollback_operation_steps[0].operation_summary,
            "unset policy 1"
        );
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_accepts_flow_operations() {
        let block = TxBlock {
            name: "precheck".to_string(),
            kind: CommandBlockKind::Show,
            rollback_policy: RollbackPolicy::None,
            steps: vec![TxStep::new(CommandFlow::new(vec![
                Command {
                    mode: "Enable".to_string(),
                    command: "terminal length 0".to_string(),
                    ..Command::default()
                },
                Command {
                    mode: "Enable".to_string(),
                    command: "show version".to_string(),
                    ..Command::default()
                },
            ]))],
            fail_fast: true,
        };

        let mut runner = FakeRunner::new(vec![ScriptedOperation {
            command: "<flow:2 steps>".to_string(),
            mode: "Enable".to_string(),
            result: Ok(flow_output(vec![
                step_output(
                    0,
                    "Enable",
                    "terminal length 0",
                    ok_output("paging disabled"),
                ),
                step_output(1, "Enable", "show version", ok_output("version output")),
            ])),
        }]);

        let result = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        assert!(result.committed);
        assert_eq!(result.executed_steps, 1);
        assert_eq!(result.step_results.len(), 1);
        assert_eq!(result.step_results[0].operation_summary, "<flow:2 steps>");
        assert_eq!(result.step_results[0].forward_operation_steps.len(), 2);
        assert_eq!(
            result.step_results[0].forward_operation_steps[0].operation_summary,
            "terminal length 0"
        );
        assert_eq!(
            result.step_results[0].forward_operation_steps[1].operation_summary,
            "show version"
        );
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_records_whole_resource_rollback_child_steps() {
        let block = TxBlock {
            name: "image-import".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                rollback: Box::new(
                    CommandFlow::new(vec![
                        Command {
                            mode: "Enable".to_string(),
                            command: "delete flash:/image.bin".to_string(),
                            ..Command::default()
                        },
                        Command {
                            mode: "Enable".to_string(),
                            command: "verify /md5 flash:/image.bin".to_string(),
                            ..Command::default()
                        },
                    ])
                    .into(),
                ),
                trigger_step_index: 0,
            },
            steps: vec![
                TxStep::new(Command {
                    mode: "Enable".to_string(),
                    command: "copy tftp: flash:/image.bin".to_string(),
                    ..Command::default()
                }),
                TxStep::new(Command {
                    mode: "Enable".to_string(),
                    command: "verify /md5 flash:/image.bin".to_string(),
                    ..Command::default()
                }),
            ],
            fail_fast: true,
        };

        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "copy tftp: flash:/image.bin".to_string(),
                mode: "Enable".to_string(),
                result: Ok(single_output(
                    "copy tftp: flash:/image.bin",
                    "Enable",
                    ok_output("copy ok"),
                )),
            },
            ScriptedOperation {
                command: "verify /md5 flash:/image.bin".to_string(),
                mode: "Enable".to_string(),
                result: Ok(single_output(
                    "verify /md5 flash:/image.bin",
                    "Enable",
                    failed_output("verify failed"),
                )),
            },
            ScriptedOperation {
                command: "<flow:2 steps>".to_string(),
                mode: "Enable".to_string(),
                result: Ok(flow_output(vec![
                    step_output(0, "Enable", "delete flash:/image.bin", ok_output("deleted")),
                    step_output(
                        1,
                        "Enable",
                        "verify /md5 flash:/image.bin",
                        ok_output("verified"),
                    ),
                ])),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(1));
        assert!(result.rollback_attempted);
        assert!(result.rollback_succeeded);
        assert_eq!(result.step_results[0].forward_operation_steps.len(), 1);
        assert_eq!(
            result.step_results[0].forward_operation_steps[0].operation_summary,
            "copy tftp: flash:/image.bin"
        );
        assert_eq!(result.step_results[1].forward_operation_steps.len(), 1);
        assert_eq!(
            result.step_results[1].forward_operation_steps[0].operation_summary,
            "verify /md5 flash:/image.bin"
        );
        assert_eq!(
            result.block_rollback_operation_summary.as_deref(),
            Some("<flow:2 steps>")
        );
        assert_eq!(result.block_rollback_steps.len(), 2);
        assert_eq!(
            result.block_rollback_steps[0].operation_summary,
            "delete flash:/image.bin"
        );
        assert_eq!(
            result.block_rollback_steps[1].operation_summary,
            "verify /md5 flash:/image.bin"
        );
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_records_forward_partial_steps_on_operation_error() {
        let recorder = SessionRecorder::new(SessionRecordLevel::KeyEventsOnly);
        let block = TxBlock {
            name: "precheck".to_string(),
            kind: CommandBlockKind::Show,
            rollback_policy: RollbackPolicy::None,
            steps: vec![TxStep::new(CommandFlow::new(vec![
                Command {
                    mode: "Enable".to_string(),
                    command: "terminal length 0".to_string(),
                    ..Command::default()
                },
                Command {
                    mode: "Enable".to_string(),
                    command: "show version".to_string(),
                    ..Command::default()
                },
            ]))],
            fail_fast: true,
        };

        let mut runner = FakeRunner::new(vec![ScriptedOperation {
            command: "<flow:2 steps>".to_string(),
            mode: "Enable".to_string(),
            result: partial_run_error(
                ConnectError::ExecTimeout("show version".to_string()),
                vec![step_output(
                    0,
                    "Enable",
                    "terminal length 0",
                    ok_output("paging disabled"),
                )],
            ),
        }])
        .with_recorder(recorder.clone());

        let result = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(0));
        assert_eq!(result.step_results[0].forward_operation_steps.len(), 1);
        assert_eq!(
            result.step_results[0].forward_operation_steps[0].operation_summary,
            "terminal length 0"
        );

        let entries = recorder.entries().expect("entries");
        assert!(entries.iter().any(|entry| matches!(
            &entry.event,
            SessionEvent::TxStepFailed {
                step_index: 0,
                operation_steps,
                ..
            } if operation_steps.len() == 1
                && operation_steps[0].operation_summary == "terminal length 0"
        )));
    }

    #[tokio::test]
    async fn execute_tx_block_records_block_rollback_event_with_original_step_index() {
        let recorder = SessionRecorder::new(SessionRecordLevel::KeyEventsOnly);
        let block = TxBlock {
            name: "image-import".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                rollback: Box::new(
                    CommandFlow::new(vec![
                        Command {
                            mode: "Enable".to_string(),
                            command: "delete flash:/image.bin".to_string(),
                            ..Command::default()
                        },
                        Command {
                            mode: "Enable".to_string(),
                            command: "verify /md5 flash:/image.bin".to_string(),
                            ..Command::default()
                        },
                    ])
                    .into(),
                ),
                trigger_step_index: 0,
            },
            steps: vec![
                TxStep::new(Command {
                    mode: "Enable".to_string(),
                    command: "copy tftp: flash:/image.bin".to_string(),
                    ..Command::default()
                }),
                TxStep::new(Command {
                    mode: "Enable".to_string(),
                    command: "verify /md5 flash:/image.bin".to_string(),
                    ..Command::default()
                }),
            ],
            fail_fast: true,
        };

        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "copy tftp: flash:/image.bin".to_string(),
                mode: "Enable".to_string(),
                result: Ok(single_output(
                    "copy tftp: flash:/image.bin",
                    "Enable",
                    ok_output("copy ok"),
                )),
            },
            ScriptedOperation {
                command: "verify /md5 flash:/image.bin".to_string(),
                mode: "Enable".to_string(),
                result: Ok(single_output(
                    "verify /md5 flash:/image.bin",
                    "Enable",
                    failed_output("verify failed"),
                )),
            },
            ScriptedOperation {
                command: "<flow:2 steps>".to_string(),
                mode: "Enable".to_string(),
                result: Ok(flow_output(vec![
                    step_output(0, "Enable", "delete flash:/image.bin", ok_output("deleted")),
                    step_output(
                        1,
                        "Enable",
                        "verify /md5 flash:/image.bin",
                        ok_output("verified"),
                    ),
                ])),
            },
        ])
        .with_recorder(recorder.clone());

        let _ = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        let entries = recorder.entries().expect("entries");
        assert!(entries.iter().any(|entry| matches!(
            &entry.event,
            SessionEvent::TxRollbackStepSucceeded {
                step_index: None,
                operation_steps,
                ..
            } if operation_steps.len() == 2
                && operation_steps[0].operation_summary == "delete flash:/image.bin"
                && operation_steps[1].operation_summary == "verify /md5 flash:/image.bin"
        )));
    }

    #[tokio::test]
    async fn execute_tx_block_preserves_partial_block_rollback_steps_on_operation_error() {
        let block = TxBlock {
            name: "policy-update".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                rollback: Box::new(
                    CommandFlow::new(vec![
                        Command {
                            mode: "Config".to_string(),
                            command: "delete policy P1".to_string(),
                            ..Command::default()
                        },
                        Command {
                            mode: "Config".to_string(),
                            command: "clear policy-cache".to_string(),
                            ..Command::default()
                        },
                    ])
                    .into(),
                ),
                trigger_step_index: 0,
            },
            steps: vec![
                TxStep::new(Command {
                    mode: "Config".to_string(),
                    command: "set policy P1".to_string(),
                    ..Command::default()
                }),
                TxStep::new(Command {
                    mode: "Config".to_string(),
                    command: "commit".to_string(),
                    ..Command::default()
                }),
            ],
            fail_fast: true,
        };

        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "set policy P1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output("set policy P1", "Config", ok_output("ok"))),
            },
            ScriptedOperation {
                command: "commit".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(
                    "commit",
                    "Config",
                    failed_output("commit failed"),
                )),
            },
            ScriptedOperation {
                command: "<flow:2 steps>".to_string(),
                mode: "Config".to_string(),
                result: partial_run_error(
                    ConnectError::ChannelDisconnectError,
                    vec![step_output(
                        0,
                        "Config",
                        "delete policy P1",
                        ok_output("delete ok"),
                    )],
                ),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        assert!(result.rollback_attempted);
        assert!(!result.rollback_succeeded);
        assert_eq!(result.block_rollback_steps.len(), 1);
        assert_eq!(
            result.block_rollback_steps[0].operation_summary,
            "delete policy P1"
        );
    }
}
