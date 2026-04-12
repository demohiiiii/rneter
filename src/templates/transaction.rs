use crate::error::ConnectError;
use crate::session::{Command, RollbackPolicy, TxBlock, TxStep};

use super::catalog::template_metadata;
use super::linux::{LinuxCommandType, classify_linux_command};

fn is_read_only_command(template: &str, command: &str) -> Result<bool, ConnectError> {
    let template_key = template.to_ascii_lowercase();
    let _ = template_metadata(&template_key)?;

    if template_key == "linux" {
        return Ok(matches!(
            classify_linux_command(command),
            LinuxCommandType::ReadOnly
        ));
    }

    let cmd = command.trim().to_ascii_lowercase();
    let show_prefixes = ["show ", "display ", "ping ", "traceroute "];
    if show_prefixes.iter().any(|prefix| cmd.starts_with(prefix)) {
        return Ok(true);
    }
    Ok(false)
}

/// Build a transaction-like block from template + command list.
///
/// Behavior:
/// - If all commands are classified as read-only, use `RollbackPolicy::None`.
/// - Otherwise use `RollbackPolicy::WholeResource`.
/// - Users must provide `resource_rollback_command` for mutating blocks.
pub fn build_tx_block(
    template: &str,
    block_name: &str,
    mode: &str,
    commands: &[String],
    timeout_secs: Option<u64>,
    resource_rollback_command: Option<String>,
) -> Result<TxBlock, ConnectError> {
    let template_key = template.to_ascii_lowercase();
    let _ = template_metadata(&template_key)?;

    if commands.is_empty() {
        return Err(ConnectError::InvalidTransaction(
            "cannot build tx block with empty commands".to_string(),
        ));
    }

    let read_only_flags = commands
        .iter()
        .map(|cmd| is_read_only_command(&template_key, cmd))
        .collect::<Result<Vec<_>, _>>()?;
    let all_read_only = read_only_flags.iter().all(|is_read_only| *is_read_only);

    if all_read_only {
        return Ok(TxBlock {
            name: block_name.to_string(),
            rollback_policy: RollbackPolicy::None,
            steps: commands
                .iter()
                .map(|cmd| {
                    TxStep::new(Command {
                        mode: mode.to_string(),
                        command: cmd.clone(),
                        timeout: timeout_secs,
                        ..Command::default()
                    })
                })
                .collect(),
            fail_fast: true,
        });
    }

    let Some(undo) = resource_rollback_command else {
        return Err(ConnectError::InvalidTransaction(
            "mutating blocks require resource_rollback_command; automatic rollback inference has been removed".to_string(),
        ));
    };

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

    Ok(TxBlock {
        name: block_name.to_string(),
        rollback_policy: RollbackPolicy::WholeResource {
            rollback: Box::new(
                Command {
                    mode: mode.to_string(),
                    command: undo,
                    timeout: timeout_secs,
                    ..Command::default()
                }
                .into(),
            ),
            trigger_step_index: 0,
        },
        steps,
        fail_fast: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_show_command_returns_read_only() {
        let read_only = is_read_only_command("cisco", "show version").expect("classify");
        assert!(read_only);
    }

    #[test]
    fn build_tx_block_for_read_only_uses_none_rollback() {
        let commands = vec!["show version".to_string(), "show clock".to_string()];
        let tx = build_tx_block("cisco", "show-block", "Enable", &commands, Some(30), None)
            .expect("build show tx");
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
            "cisco",
            "addr-create",
            "Config",
            &commands,
            Some(20),
            Some("no address-object host WEB01".to_string()),
        )
        .expect("build config tx");
        assert!(matches!(
            tx.rollback_policy,
            RollbackPolicy::WholeResource { .. }
        ));
        assert!(tx.steps.iter().all(|s| s.rollback.is_none()));
    }

    #[test]
    fn build_tx_block_requires_explicit_rollback_for_mutating_commands() {
        let commands = vec!["undo acl 3000".to_string()];
        let err = build_tx_block("huawei", "bad", "Config", &commands, None, None)
            .expect_err("should fail");
        assert!(matches!(err, ConnectError::InvalidTransaction(_)));
        assert!(
            err.to_string()
                .contains("mutating blocks require resource_rollback_command")
        );
    }
}
