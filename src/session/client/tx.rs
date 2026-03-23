use super::super::*;
use std::future::Future;
use std::pin::Pin;

pub(super) type CommandRunFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Output, ConnectError>> + Send + 'a>>;

pub(super) trait TxCommandRunner {
    fn recorder(&self) -> Option<&SessionRecorder>;

    fn run_command<'a>(
        &'a mut self,
        command: &'a str,
        mode: &'a str,
        sys: Option<&'a String>,
        timeout: Duration,
    ) -> CommandRunFuture<'a>;
}

pub(super) async fn rollback_committed_block_with_runner<R: TxCommandRunner + ?Sized>(
    runner: &mut R,
    block: &TxBlock,
    sys: Option<&String>,
) -> Result<(bool, Vec<String>), ConnectError> {
    if block.kind == CommandBlockKind::Show {
        return Ok((true, Vec::new()));
    }
    let executed = (0..block.steps.len()).collect::<Vec<_>>();
    let plan = block.plan_rollback(&executed, None)?;
    if let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxRollbackStarted {
            block_name: block.name.clone(),
        });
    }
    let mut rollback_succeeded = true;
    let mut rollback_errors = Vec::new();
    for (plan_idx, rollback) in plan.into_iter().enumerate() {
        let timeout = Duration::from_secs(rollback.timeout_secs.unwrap_or(60));
        match runner
            .run_command(&rollback.command, &rollback.mode, sys, timeout)
            .await
        {
            Ok(output) if output.success => {
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepSucceeded {
                        block_name: block.name.clone(),
                        step_index: Some(plan_idx),
                        mode: rollback.mode.clone(),
                        command: rollback.command.clone(),
                    });
                }
            }
            Ok(output) => {
                rollback_succeeded = false;
                let reason = format!(
                    "workflow rollback command failed for block '{}': '{}' output='{}'",
                    block.name, rollback.command, output.content
                );
                rollback_errors.push(reason.clone());
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: Some(plan_idx),
                        mode: rollback.mode.clone(),
                        command: rollback.command.clone(),
                        reason,
                    });
                }
            }
            Err(err) => {
                rollback_succeeded = false;
                let reason = format!(
                    "workflow rollback command error for block '{}': '{}' err={}",
                    block.name, rollback.command, err
                );
                rollback_errors.push(reason.clone());
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: Some(plan_idx),
                        mode: rollback.mode.clone(),
                        command: rollback.command.clone(),
                        reason,
                    });
                }
            }
        }
    }

    Ok((rollback_succeeded, rollback_errors))
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
    let mut failure_reason = None;
    let mut failed_step = None;

    for (idx, step) in block.steps.iter().enumerate() {
        let timeout = Duration::from_secs(step.timeout_secs.unwrap_or(60));
        match runner
            .run_command(&step.command, &step.mode, sys, timeout)
            .await
        {
            Ok(output) if output.success => {
                executed_indices.push(idx);
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepSucceeded {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step.mode.clone(),
                        command: step.command.clone(),
                    });
                }
            }
            Ok(output) => {
                failed_step = Some(idx);
                failure_reason = Some(format!(
                    "step[{idx}] command failed: '{}' output='{}'",
                    step.command, output.content
                ));
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepFailed {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step.mode.clone(),
                        command: step.command.clone(),
                        reason: failure_reason.clone().unwrap_or_default(),
                    });
                }
                if block.fail_fast {
                    break;
                }
            }
            Err(err) => {
                failed_step = Some(idx);
                failure_reason = Some(format!("step[{idx}] command error: {err}"));
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxStepFailed {
                        block_name: block.name.clone(),
                        step_index: idx,
                        mode: step.mode.clone(),
                        command: step.command.clone(),
                        reason: failure_reason.clone().unwrap_or_default(),
                    });
                }
                if block.fail_fast {
                    break;
                }
            }
        }
    }

    if failed_step.is_none() {
        let result = TxResult::committed(block.name.clone(), executed_indices.len());
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

    let rollback_plan = block.plan_rollback(&executed_indices, failed_step)?;
    let rollback_attempted = !rollback_plan.is_empty();
    if rollback_attempted && let Some(recorder) = runner.recorder() {
        let _ = recorder.record_event(SessionEvent::TxRollbackStarted {
            block_name: block.name.clone(),
        });
    }
    let mut rollback_succeeded = rollback_attempted;
    let mut rollback_errors = Vec::new();
    let mut rollback_steps = 0;
    if !rollback_attempted {
        let reason = format!(
            "rollback not attempted: no rollback commands for executed steps; forward_failure={}",
            failure_reason
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        );
        rollback_errors.push(reason);
    }

    for (plan_idx, rollback) in rollback_plan.into_iter().enumerate() {
        let timeout = Duration::from_secs(rollback.timeout_secs.unwrap_or(60));
        match runner
            .run_command(&rollback.command, &rollback.mode, sys, timeout)
            .await
        {
            Ok(output) if output.success => {
                rollback_steps += 1;
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepSucceeded {
                        block_name: block.name.clone(),
                        step_index: Some(plan_idx),
                        mode: rollback.mode.clone(),
                        command: rollback.command.clone(),
                    });
                }
            }
            Ok(output) => {
                rollback_succeeded = false;
                rollback_steps += 1;
                let reason = format!(
                    "rollback command failed: '{}' output='{}'",
                    rollback.command, output.content
                );
                rollback_errors.push(reason.clone());
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: Some(plan_idx),
                        mode: rollback.mode.clone(),
                        command: rollback.command.clone(),
                        reason,
                    });
                }
            }
            Err(err) => {
                rollback_succeeded = false;
                let reason = format!("rollback command error: '{}' err={}", rollback.command, err);
                rollback_errors.push(reason.clone());
                if let Some(recorder) = runner.recorder() {
                    let _ = recorder.record_event(SessionEvent::TxRollbackStepFailed {
                        block_name: block.name.clone(),
                        step_index: Some(plan_idx),
                        mode: rollback.mode.clone(),
                        command: rollback.command.clone(),
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
        if let Some(block) = workflow.blocks.get(block_idx) {
            let (ok, errors) = rollback_committed_block_with_runner(runner, block, sys).await?;
            if !ok {
                rollback_succeeded = false;
            }
            rollback_errors.extend(errors);
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

    struct ScriptedCommand {
        command: String,
        mode: String,
        result: Result<Output, ConnectError>,
    }

    struct FakeRunner {
        scripted: VecDeque<ScriptedCommand>,
        recorder: Option<SessionRecorder>,
    }

    impl FakeRunner {
        fn new(scripted: Vec<ScriptedCommand>) -> Self {
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

        fn run_command<'a>(
            &'a mut self,
            command: &'a str,
            mode: &'a str,
            _sys: Option<&'a String>,
            _timeout: Duration,
        ) -> CommandRunFuture<'a> {
            Box::pin(async move {
                let scripted = self.scripted.pop_front().ok_or_else(|| {
                    ConnectError::InternalServerError(
                        "unexpected scripted command exhaustion".to_string(),
                    )
                })?;
                assert_eq!(scripted.command, command);
                assert_eq!(scripted.mode, mode);
                scripted.result
            })
        }
    }

    fn ok_output(content: &str) -> Output {
        Output {
            success: true,
            content: content.to_string(),
            all: content.to_string(),
            prompt: None,
        }
    }

    fn failed_output(content: &str) -> Output {
        Output {
            success: false,
            content: content.to_string(),
            all: content.to_string(),
            prompt: None,
        }
    }

    fn per_step_block(rollback_on_failure: bool) -> TxBlock {
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
                    rollback_on_failure: false,
                },
                TxStep {
                    mode: "Config".to_string(),
                    command: "set addr 2".to_string(),
                    timeout_secs: None,
                    rollback_command: Some("unset addr 2".to_string()),
                    rollback_on_failure,
                },
            ],
            fail_fast: true,
        }
    }

    #[tokio::test]
    async fn execute_tx_block_skips_failed_step_rollback_by_default() {
        let mut runner = FakeRunner::new(vec![
            ScriptedCommand {
                command: "set addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(ok_output("ok")),
            },
            ScriptedCommand {
                command: "set addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(failed_output("invalid input")),
            },
            ScriptedCommand {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(ok_output("rollback ok")),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &per_step_block(false), None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(1));
        assert!(result.rollback_attempted);
        assert!(result.rollback_succeeded);
        assert_eq!(result.rollback_steps, 1);
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_can_rollback_failed_step_when_enabled() {
        let mut runner = FakeRunner::new(vec![
            ScriptedCommand {
                command: "set addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(ok_output("ok")),
            },
            ScriptedCommand {
                command: "set addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(failed_output("invalid input")),
            },
            ScriptedCommand {
                command: "unset addr 2".to_string(),
                mode: "Config".to_string(),
                result: Ok(ok_output("failed-step rollback ok")),
            },
            ScriptedCommand {
                command: "unset addr 1".to_string(),
                mode: "Config".to_string(),
                result: Ok(ok_output("rollback ok")),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &per_step_block(true), None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(1));
        assert!(result.rollback_attempted);
        assert!(result.rollback_succeeded);
        assert_eq!(result.rollback_steps, 2);
        assert!(runner.scripted.is_empty());
    }

    #[tokio::test]
    async fn execute_tx_block_whole_resource_waits_for_trigger_step() {
        let block = TxBlock {
            name: "policy-create".to_string(),
            kind: CommandBlockKind::Config,
            rollback_policy: RollbackPolicy::WholeResource {
                mode: "Config".to_string(),
                undo_command: "delete policy P1".to_string(),
                timeout_secs: None,
                trigger_step_index: 1,
            },
            steps: vec![
                TxStep {
                    mode: "Config".to_string(),
                    command: "set addr A".to_string(),
                    timeout_secs: None,
                    rollback_command: None,
                    rollback_on_failure: false,
                },
                TxStep {
                    mode: "Config".to_string(),
                    command: "set policy P1".to_string(),
                    timeout_secs: None,
                    rollback_command: None,
                    rollback_on_failure: false,
                },
            ],
            fail_fast: true,
        };
        let mut runner = FakeRunner::new(vec![
            ScriptedCommand {
                command: "set addr A".to_string(),
                mode: "Config".to_string(),
                result: Ok(ok_output("ok")),
            },
            ScriptedCommand {
                command: "set policy P1".to_string(),
                mode: "Config".to_string(),
                result: Ok(failed_output("invalid input")),
            },
        ]);

        let result = execute_tx_block_with_runner(&mut runner, &block, None)
            .await
            .expect("execute block");

        assert_eq!(result.failed_step, Some(1));
        assert!(!result.rollback_attempted);
        assert!(!result.rollback_succeeded);
        assert_eq!(result.rollback_steps, 0);
        assert_eq!(result.rollback_errors.len(), 1);
        assert!(runner.scripted.is_empty());
    }
}
