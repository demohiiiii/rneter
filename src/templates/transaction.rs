use crate::error::ConnectError;
use crate::session::{Command, RollbackPolicy, TxBlock, TxStep};

/// Build a transaction-like block with an explicit rollback policy.
pub fn build_tx_block(
    block_name: &str,
    mode: &str,
    commands: &[String],
    timeout_secs: Option<u64>,
    rollback_policy: RollbackPolicy,
) -> Result<TxBlock, ConnectError> {
    if commands.is_empty() {
        return Err(ConnectError::InvalidTransaction(
            "cannot build tx block with empty commands".to_string(),
        ));
    }

    let steps = commands
        .iter()
        .map(|cmd| {
            TxStep::new(Command {
                mode: mode.to_string(),
                command: cmd.clone(),
                timeout: timeout_secs,
                ..Command::default()
            })
        })
        .collect();

    let block = TxBlock {
        name: block_name.to_string(),
        rollback_policy,
        steps,
        fail_fast: true,
    };
    block.validate()?;
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tx_block_uses_explicit_none_policy_without_command_inference() {
        let commands = vec!["delete everything".to_string()];
        let tx = build_tx_block(
            "explicit-none",
            "Enable",
            &commands,
            Some(30),
            RollbackPolicy::None,
        )
        .expect("build explicit no-rollback block");
        assert!(matches!(tx.rollback_policy, RollbackPolicy::None));
        assert!(tx.steps.iter().all(|s| s.rollback.is_none()));
    }

    #[test]
    fn build_tx_block_supports_whole_resource_rollback() {
        let commands = vec![
            "address-object host WEB01".to_string(),
            "host 10.0.0.10".to_string(),
        ];
        let tx = build_tx_block(
            "addr-create",
            "Config",
            &commands,
            Some(20),
            RollbackPolicy::WholeResource {
                rollback: Box::new(
                    Command {
                        mode: "Config".to_string(),
                        command: "no address-object host WEB01".to_string(),
                        timeout: Some(20),
                        ..Command::default()
                    }
                    .into(),
                ),
                trigger_step_index: 0,
            },
        )
        .expect("build config tx");
        assert!(matches!(
            tx.rollback_policy,
            RollbackPolicy::WholeResource { .. }
        ));
        assert!(tx.steps.iter().all(|s| s.rollback.is_none()));
    }

    #[test]
    fn build_tx_block_preserves_explicit_per_step_policy() {
        let commands = vec!["undo acl 3000".to_string()];
        let block = build_tx_block(
            "explicit-per-step",
            "Config",
            &commands,
            None,
            RollbackPolicy::PerStep,
        )
        .expect("build per-step block");
        assert!(matches!(block.rollback_policy, RollbackPolicy::PerStep));
    }
}
