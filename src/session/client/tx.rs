use super::super::*;
use std::future::Future;
use std::pin::Pin;

pub(super) type OperationRunFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CommandFlowOutput, ConnectError>> + Send + 'a>>;

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
    rollback_state: TxStepRollbackState,
    rollback_reason: Option<String>,
) {
    if let Some(step_result) = step_results.get_mut(step_index) {
        step_result.rollback_operation_summary = Some(operation_summary.to_string());
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

fn operation_failure_output(output: &CommandFlowOutput) -> String {
    output
        .outputs
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
    let block_level_indices = attempted_step_indices(&executed, &failed);
    if !result.rollback_attempted {
        result.rollback_succeeded = false;
        result
            .rollback_errors
            .extend(block.explain_missing_rollback_plan(&executed, None));
        if let RollbackPolicy::WholeResource { rollback, .. } = &block.rollback_policy {
            let (_, rollback_operation_summary) = rollback.display_summary()?;
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
    for (plan_idx, rollback) in plan.into_iter().enumerate() {
        let (rollback_mode, rollback_operation_summary) = rollback.operation.display_summary()?;
        result.rollback_steps += 1;
        match runner.run_operation(&rollback.operation, sys).await {
            Ok(output) if output.success => {
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut result.step_results,
                        step_idx,
                        &rollback_operation_summary,
                        TxStepRollbackState::Succeeded,
                        None,
                    );
                } else {
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
                        step_index: Some(plan_idx),
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                    });
                }
            }
            Ok(output) => {
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
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
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
                        step_index: Some(plan_idx),
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                        reason,
                    });
                }
            }
            Err(err) => {
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
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
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
                        step_index: Some(plan_idx),
                        mode: rollback_mode,
                        operation_summary: rollback_operation_summary,
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
                executed_indices.push(idx);
                if let Some(step_result) = step_results.get_mut(idx) {
                    step_result.execution_state = TxStepExecutionState::Succeeded;
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepSucceeded {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step_mode.clone(),
                        operation_summary: step_operation_summary.clone(),
                    });
                }
            }
            Ok(output) => {
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
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepFailed {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step_mode.clone(),
                        operation_summary: step_operation_summary.clone(),
                        reason,
                    });
                }
                if block.fail_fast {
                    break;
                }
            }
            Err(err) => {
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
                }
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepFailed {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step_mode,
                        operation_summary: step_operation_summary,
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
            apply_block_rollback_outcome(
                &mut step_results,
                &block_level_indices,
                &rollback_operation_summary,
                TxStepRollbackState::BlockSkipped,
                Some(missing_rollback_reasons.join("; ")),
            );
        }
    }

    for (plan_idx, rollback) in rollback_plan.into_iter().enumerate() {
        let (rollback_mode, rollback_operation_summary) = rollback.operation.display_summary()?;
        rollback_steps += 1;
        match runner.run_operation(&rollback.operation, sys).await {
            Ok(output) if output.success => {
                if let Some(step_idx) = rollback.step_index {
                    apply_step_rollback_outcome(
                        &mut step_results,
                        step_idx,
                        &rollback_operation_summary,
                        TxStepRollbackState::Succeeded,
                        None,
                    );
                } else {
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
                        step_index: Some(plan_idx),
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                    });
                }
            }
            Ok(output) => {
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
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
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
                        step_index: Some(plan_idx),
                        mode: rollback_mode.clone(),
                        operation_summary: rollback_operation_summary.clone(),
                        reason,
                    });
                }
            }
            Err(err) => {
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
                        TxStepRollbackState::Failed,
                        Some(reason.clone()),
                    );
                } else {
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
                        step_index: Some(plan_idx),
                        mode: rollback_mode,
                        operation_summary: rollback_operation_summary,
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
        result: Result<CommandFlowOutput, ConnectError>,
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
                    ConnectError::InternalServerError(
                        "unexpected scripted command exhaustion".to_string(),
                    )
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

    fn single_output(output: Output) -> CommandFlowOutput {
        CommandFlowOutput {
            success: output.success,
            outputs: vec![output],
        }
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
                result: Ok(single_output(ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(failed_output("invalid input"))),
            },
            ScriptedOperation {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(ok_output("rollback ok"))),
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
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_can_rollback_failed_step_when_enabled() {
        let mut runner = FakeRunner::new(vec![
            ScriptedOperation {
                command: "set addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(failed_output("invalid input"))),
            },
            ScriptedOperation {
                command: "unset addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(ok_output("failed-step rollback ok"))),
            },
            ScriptedOperation {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(ok_output("rollback ok"))),
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
                result: Ok(single_output(ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set policy P1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(failed_output("invalid input"))),
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
                result: Ok(single_output(ok_output("ok"))),
            },
            ScriptedOperation {
                command: "set policy 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(failed_output("invalid input"))),
            },
            ScriptedOperation {
                command: "unset policy 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(ok_output("rollback ok"))),
            },
            ScriptedOperation {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(single_output(ok_output("rollback ok"))),
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
            result.block_results[0].step_results[0].rollback_state,
            TxStepRollbackState::Succeeded
        );
        assert_eq!(
            result.block_results[1].step_results[0].execution_state,
            TxStepExecutionState::Failed
        );
        assert_eq!(
            result.block_results[1].step_results[0].rollback_state,
            TxStepRollbackState::Succeeded
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
            result: Ok(CommandFlowOutput {
                success: true,
                outputs: vec![ok_output("paging disabled"), ok_output("version output")],
            }),
        }]);

        let result = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        assert!(result.committed);
        assert_eq!(result.executed_steps, 1);
        assert_eq!(result.step_results.len(), 1);
        assert_eq!(result.step_results[0].operation_summary, "<flow:2 steps>");
        assert!(runner.scripted.is_empty());
    }
}
