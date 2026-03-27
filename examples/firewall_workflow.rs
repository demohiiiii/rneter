use rneter::session::{
    ConnectionRequest, ExecutionContext, MANAGER, RollbackPolicy, TxWorkflow, TxWorkflowResult,
};
use rneter::templates;
use std::error::Error;

fn validate_template(template: &str) -> Result<(), Box<dyn Error>> {
    let diagnostics = templates::diagnose_template(template)?;
    if diagnostics.has_issues() {
        return Err(format!(
            "template '{template}' has diagnostics issues: missing_sources={:?}, missing_targets={:?}, unreachable={:?}, dead_ends={:?}",
            diagnostics.missing_edge_sources,
            diagnostics.missing_edge_targets,
            diagnostics.unreachable_states,
            diagnostics.dead_end_states
        )
        .into());
    }
    Ok(())
}

fn print_workflow_report(result: &TxWorkflowResult) {
    println!(
        "workflow={} committed={} failed_block={:?} rollback_attempted={} rollback_succeeded={}",
        result.workflow_name,
        result.committed,
        result.failed_block,
        result.rollback_attempted,
        result.rollback_succeeded
    );

    for (idx, block) in result.block_results.iter().enumerate() {
        println!(
            "  block[{idx}] name={} committed={} failed_step={:?} executed_steps={} rollback_attempted={} rollback_succeeded={}",
            block.block_name,
            block.committed,
            block.failed_step,
            block.executed_steps,
            block.rollback_attempted,
            block.rollback_succeeded
        );
        if let Some(reason) = &block.failure_reason {
            println!("    failure_reason={reason}");
        }
        if !block.rollback_errors.is_empty() {
            println!("    rollback_errors={:?}", block.rollback_errors);
        }
        if let Some(operation_summary) = &block.block_rollback_operation_summary {
            println!("    block_rollback_operation_summary={operation_summary}");
        }
        for child in &block.block_rollback_steps {
            println!(
                "      block_rollback_step[{}] mode={} op={} success={}",
                child.step_index, child.mode, child.operation_summary, child.success
            );
        }
        for step in &block.step_results {
            println!(
                "    step[{}] mode={} execution_state={:?} rollback_state={:?}",
                step.step_index, step.mode, step.execution_state, step.rollback_state
            );
            println!("      operation_summary={}", step.operation_summary);
            for child in &step.forward_operation_steps {
                println!(
                    "      forward_step[{}] mode={} op={} success={}",
                    child.step_index, child.mode, child.operation_summary, child.success
                );
            }
            if let Some(reason) = &step.failure_reason {
                println!("      failure_reason={reason}");
            }
            if let Some(operation_summary) = &step.rollback_operation_summary {
                println!("      rollback_operation_summary={operation_summary}");
            }
            for child in &step.rollback_operation_steps {
                println!(
                    "      rollback_step[{}] mode={} op={} success={}",
                    child.step_index, child.mode, child.operation_summary, child.success
                );
            }
            if let Some(reason) = &step.rollback_reason {
                println!("      rollback_reason={reason}");
            }
        }
    }

    if !result.rollback_errors.is_empty() {
        println!("workflow_rollback_errors={:?}", result.rollback_errors);
    }
}

fn print_workflow_plan(workflow: &TxWorkflow) -> Result<(), Box<dyn Error>> {
    println!(
        "dry-run workflow={} blocks={} fail_fast={}",
        workflow.name,
        workflow.blocks.len(),
        workflow.fail_fast
    );
    for (block_idx, block) in workflow.blocks.iter().enumerate() {
        println!(
            "  block[{block_idx}] name={} kind={:?} rollback_policy={:?} steps={}",
            block.name,
            block.kind,
            block.rollback_policy,
            block.steps.len()
        );
        if let RollbackPolicy::WholeResource {
            rollback,
            trigger_step_index,
        } = &block.rollback_policy
        {
            let rollback_summary = rollback.summary()?;
            println!(
                "    whole_resource_rollback kind={} mode={} steps={} desc={} trigger_step_index={}",
                rollback_summary.kind,
                rollback_summary.mode,
                rollback_summary.step_count,
                rollback_summary.description,
                trigger_step_index
            );
        }
        for (step_idx, step) in block.steps.iter().enumerate() {
            let run_summary = step.run.summary()?;
            let rollback_summary = step.rollback.as_ref().map(|operation| operation.summary());
            println!(
                "    step[{step_idx}] kind={} mode={} steps={} desc={} rollback={:?} rollback_on_failure={}",
                run_summary.kind,
                run_summary.mode,
                run_summary.step_count,
                run_summary.description,
                rollback_summary
                    .as_ref()
                    .and_then(|summary| summary.as_ref().ok())
                    .map(|summary| summary.description.as_str()),
                step.rollback_on_failure
            );
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let dry_run = std::env::args().any(|arg| arg == "--dry-run");

    // Fail early if template graph quality is problematic.
    validate_template("cisco")?;

    // 1) Build address-object block (whole-resource rollback).
    let addr_cmds = vec![
        "object network WEB01".to_string(),
        "host 10.10.10.10".to_string(),
    ];
    let addr_block = templates::build_tx_block(
        "cisco",
        "addr-objects",
        "Config",
        &addr_cmds,
        Some(30),
        Some("no object network WEB01".to_string()),
    )?;

    // 2) Build service-object block (whole-resource rollback).
    let svc_cmds = vec![
        "object service WEB01-SVC".to_string(),
        "service tcp destination eq 443".to_string(),
    ];
    let svc_block = templates::build_tx_block(
        "cisco",
        "service-objects",
        "Config",
        &svc_cmds,
        Some(30),
        Some("no object service WEB01-SVC".to_string()),
    )?;

    // 3) Build policy block (whole-resource rollback).
    let policy_cmds = vec![
        "access-list OUTSIDE_IN extended permit tcp object WEB01 object WEB01-SVC".to_string(),
    ];
    let policy_block = templates::build_tx_block(
        "cisco",
        "policy-rules",
        "Config",
        &policy_cmds,
        Some(30),
        Some(
            "no access-list OUTSIDE_IN extended permit tcp object WEB01 object WEB01-SVC"
                .to_string(),
        ),
    )?;

    // 4) Compose multi-block workflow: all blocks succeed or rollback previously committed blocks.
    let workflow = TxWorkflow {
        name: "fw-policy-publish".to_string(),
        blocks: vec![addr_block, svc_block, policy_block],
        fail_fast: true,
    };

    if dry_run {
        print_workflow_plan(&workflow)?;
        return Ok(());
    }

    let result = MANAGER
        .execute_tx_workflow_with_context(
            ConnectionRequest::new(
                "admin".to_string(),
                "192.168.1.1".to_string(),
                22,
                "password".to_string(),
                None,
                templates::cisco()?,
            ),
            workflow,
            ExecutionContext::default(),
        )
        .await?;

    print_workflow_report(&result);
    if !result.committed {
        return Err("firewall workflow failed; inspect block and rollback report above".into());
    }
    Ok(())
}
