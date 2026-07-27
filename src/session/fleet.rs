use super::*;
use tokio::task::JoinSet;

/// Default maximum number of fleet targets executed concurrently.
pub const DEFAULT_FLEET_CONCURRENCY_LIMIT: usize = 16;

/// One independently configured target in a fleet execution.
pub struct FleetTarget {
    pub request: ConnectionRequest,
    pub context: ExecutionContext,
}

impl FleetTarget {
    /// Build a fleet target from its connection request and execution context.
    pub fn new(request: ConnectionRequest, context: ExecutionContext) -> Self {
        Self { request, context }
    }

    /// Build a fleet target with the secure default execution context.
    pub fn with_default_context(request: ConnectionRequest) -> Self {
        Self::new(request, ExecutionContext::default())
    }

    /// Stable textual device address used to identify this target's result.
    pub fn device_addr(&self) -> String {
        self.request.device_addr()
    }
}

/// Controls bounded concurrent execution across a fleet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FleetOptions {
    /// Maximum number of targets with an operation in flight at once.
    pub concurrency_limit: usize,
}

impl FleetOptions {
    /// Build fleet options with an explicit concurrency limit.
    pub const fn new(concurrency_limit: usize) -> Self {
        Self { concurrency_limit }
    }

    fn validate(&self) -> Result<(), ConnectError> {
        if self.concurrency_limit == 0 {
            return Err(ConnectError::InvalidFleetOptions(
                "concurrency_limit must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for FleetOptions {
    fn default() -> Self {
        Self::new(DEFAULT_FLEET_CONCURRENCY_LIMIT)
    }
}

/// Result of executing one operation against one fleet target.
#[derive(Debug)]
pub struct FleetExecutionResult {
    /// Position of the target in the input fleet.
    pub index: usize,
    /// Stable `user@host:port` identifier captured before execution.
    pub device_addr: String,
    /// Target-local result. Failures never cancel other targets.
    pub result: Result<SessionOperationOutput, SessionOperationExecutionError>,
}

impl FleetExecutionResult {
    /// Whether the target completed without a connection or execution error.
    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }

    /// Borrow the successful operation output, when available.
    pub fn output(&self) -> Option<&SessionOperationOutput> {
        self.result.as_ref().ok()
    }

    /// Borrow the target-local operation error, when present.
    pub fn error(&self) -> Option<&SessionOperationExecutionError> {
        self.result.as_ref().err()
    }
}

fn spawn_target(
    tasks: &mut JoinSet<FleetExecutionResult>,
    manager: SshConnectionManager,
    index: usize,
    target: FleetTarget,
    operation: SessionOperation,
) {
    tasks.spawn(async move {
        let device_addr = target.device_addr();
        let result = manager
            .execute_operation_with_context(target.request, operation, target.context)
            .await;
        FleetExecutionResult {
            index,
            device_addr,
            result,
        }
    });
}

impl SshConnectionManager {
    /// Execute one operation independently across a fleet with bounded concurrency.
    ///
    /// Every target runs to completion even when another target fails. Returned
    /// results preserve the input target order, and each error retains any partial
    /// child-step output produced for that target.
    pub async fn execute_on_fleet(
        &self,
        targets: Vec<FleetTarget>,
        operation: SessionOperation,
        options: FleetOptions,
    ) -> Result<Vec<FleetExecutionResult>, ConnectError> {
        options.validate()?;
        operation.summary()?;

        let target_count = targets.len();
        if target_count == 0 {
            return Ok(Vec::new());
        }

        let mut pending = targets.into_iter().enumerate();
        let mut tasks = JoinSet::new();
        for _ in 0..options.concurrency_limit.min(target_count) {
            let Some((index, target)) = pending.next() else {
                break;
            };
            spawn_target(&mut tasks, self.clone(), index, target, operation.clone());
        }

        let mut results = Vec::with_capacity(target_count);
        while let Some(joined) = tasks.join_next().await {
            let result = joined.map_err(|error| {
                ConnectError::InternalServerError(format!("fleet task failed: {error}"))
            })?;
            results.push(result);

            if let Some((index, target)) = pending.next() {
                spawn_target(&mut tasks, self.clone(), index, target, operation.clone());
            }
        }

        results.sort_unstable_by_key(|result| result.index);
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_options_default_to_a_positive_limit() {
        assert_eq!(
            FleetOptions::default().concurrency_limit,
            DEFAULT_FLEET_CONCURRENCY_LIMIT
        );
    }

    #[test]
    fn zero_concurrency_is_rejected() {
        let error = FleetOptions::new(0).validate().expect_err("invalid limit");
        assert!(matches!(error, ConnectError::InvalidFleetOptions(_)));
    }
}
