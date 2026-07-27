use super::*;

/// Converts the shared init error produced by the single-flight cache entry
/// back into a plain [`ConnectError`].
///
/// When several callers awaited the same failed connection attempt, only one
/// of them can take ownership of the error; the others receive a copy that
/// preserves the original message.
fn unwrap_shared_connect_error(error: Arc<ConnectError>) -> ConnectError {
    Arc::try_unwrap(error)
        .unwrap_or_else(|shared| ConnectError::InternalServerError(shared.to_string()))
}

impl SshConnectionManager {
    /// Creates a new SSH connection manager with the default pool settings.
    pub fn new() -> Self {
        Self::with_pool_config(ConnectionPoolConfig::default())
    }

    /// Creates a new SSH connection manager with custom pool settings.
    ///
    /// Connection shutdown is owned by each connection's worker task: it
    /// gracefully closes the connection (running `before_disconnect` hooks
    /// and sending `exit`) once the last command sender is gone — i.e. the
    /// pool evicted its handle *and* no caller holds one. A connection that
    /// is still in use by a caller-held sender is therefore never closed
    /// underneath them by an idle eviction.
    pub fn with_pool_config(config: ConnectionPoolConfig) -> Self {
        let cache = Cache::builder()
            .max_capacity(config.max_connections)
            .time_to_idle(config.idle_timeout)
            .build();

        // Pace pending-task maintenance well below the idle timeout so
        // evictions (and thus graceful shutdown) are observed promptly.
        let maintenance_period =
            (config.idle_timeout / 4).clamp(Duration::from_secs(1), Duration::from_secs(60));

        Self {
            cache,
            maintenance_running: Arc::new(AtomicBool::new(false)),
            maintenance_period,
        }
    }

    /// Starts the pool maintenance task if it is not already running.
    ///
    /// The moka future cache has no background threads: expired entries are
    /// only evicted while the cache is being used. This task keeps evictions
    /// flowing during quiet periods so idle connections actually get closed,
    /// and stops itself once the pool is empty (it is restarted on demand by
    /// the next pool access).
    fn spawn_maintenance_if_needed(&self) {
        if self
            .maintenance_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let cache = self.cache.clone();
        let running = self.maintenance_running.clone();
        let period = self.maintenance_period;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                cache.run_pending_tasks().await;
                if cache.entry_count() == 0 {
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
        });
    }

    /// Gets a cached SSH client using a structured request/context pair.
    pub async fn get_with_context(
        &self,
        request: ConnectionRequest,
        context: ExecutionContext,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        self.get_client_with_request_and_recording(
            request,
            context.security_options,
            context.connect_timeout,
            None,
        )
        .await
        .map(|(sender, _client)| sender)
    }

    /// Execute a single command directly using a structured connection/context pair.
    pub async fn execute_command_with_context(
        &self,
        request: ConnectionRequest,
        command: Command,
        context: ExecutionContext,
    ) -> Result<Output, ConnectError> {
        let flow = command.into_flow()?;
        if flow.steps.len() != 1 {
            return Err(ConnectError::InvalidCommandFlow(
                "multiline command produces multiple outputs; use execute_multiline_command_with_context"
                    .to_string(),
            ));
        }
        let result = self
            .execute_operation_with_context(request, SessionOperation::from(flow), context)
            .await
            .map_err(|err| err.into_parts().0)?;
        match result.steps.len() {
            1 => Ok(result
                .steps
                .into_iter()
                .next()
                .expect("single step output should exist")
                .into_output()),
            count => Err(ConnectError::InternalServerError(format!(
                "expected one output for command execution, got {count}"
            ))),
        }
    }

    /// Execute newline-separated command text using its configured multiline strategy.
    ///
    /// The result always retains one child output per concrete command. In
    /// [`MultilineMode::Whole`] mode the result therefore contains one step.
    pub async fn execute_multiline_command_with_context(
        &self,
        request: ConnectionRequest,
        command: Command,
        context: ExecutionContext,
    ) -> Result<SessionOperationOutput, SessionOperationExecutionError> {
        self.execute_operation_with_context(request, SessionOperation::from(command), context)
            .await
    }

    /// Execute any supported session operation using a structured connection/context pair.
    ///
    /// Returns the generic operation-level result model so future operation kinds
    /// do not need to be flattened into the legacy command-flow shape.
    pub async fn execute_operation_with_context(
        &self,
        request: ConnectionRequest,
        operation: SessionOperation,
        context: ExecutionContext,
    ) -> Result<SessionOperationOutput, SessionOperationExecutionError> {
        let sys = context.sys.clone();
        let (_sender, client) = self
            .get_client_with_request_and_recording(
                request,
                context.security_options,
                context.connect_timeout,
                None,
            )
            .await
            .map_err(|err| {
                SessionOperationExecutionError::new(
                    err,
                    SessionOperationOutput {
                        success: false,
                        steps: Vec::new(),
                    },
                )
            })?;

        let mut client_guard = client.write().await;
        client_guard
            .execute_operation_detailed(&operation, sys.as_ref())
            .await
            .map_err(|err| {
                let (error, partial_output) = err.into_parts();
                SessionOperationExecutionError::new(error, partial_output)
            })
    }

    /// Execute a multi-step command flow on one live connection.
    pub async fn execute_command_flow_with_context(
        &self,
        request: ConnectionRequest,
        flow: CommandFlow,
        context: ExecutionContext,
    ) -> Result<CommandFlowOutput, ConnectError> {
        self.execute_operation_with_context(request, SessionOperation::from(flow), context)
            .await
            .map(|output| output.into_command_flow_output())
            .map_err(|err| err.into_parts().0)
    }

    /// Execute a transaction-like block with structured connection/context options.
    pub async fn execute_tx_block_with_context(
        &self,
        request: ConnectionRequest,
        block: TxBlock,
        context: ExecutionContext,
    ) -> Result<TxResult, ConnectError> {
        let sys = context.sys.clone();
        let (_sender, client) = self
            .get_client_with_request_and_recording(
                request,
                context.security_options,
                context.connect_timeout,
                None,
            )
            .await?;

        let mut client_guard = client.write().await;
        client_guard.execute_tx_block(&block, sys.as_ref()).await
    }

    /// Execute a workflow with structured connection/context options.
    pub async fn execute_tx_workflow_with_context(
        &self,
        request: ConnectionRequest,
        workflow: TxWorkflow,
        context: ExecutionContext,
    ) -> Result<TxWorkflowResult, ConnectError> {
        let sys = context.sys.clone();
        let (_sender, client) = self
            .get_client_with_request_and_recording(
                request,
                context.security_options,
                context.connect_timeout,
                None,
            )
            .await?;

        let mut client_guard = client.write().await;
        client_guard
            .execute_tx_workflow(&workflow, sys.as_ref())
            .await
    }

    /// Upload a local file to the remote host over SFTP using a structured request/context pair.
    pub async fn upload_file_with_context(
        &self,
        request: ConnectionRequest,
        upload: FileUploadRequest,
        context: ExecutionContext,
    ) -> Result<(), ConnectError> {
        let (_sender, client) = self
            .get_client_with_request_and_recording(
                request,
                context.security_options,
                context.connect_timeout,
                None,
            )
            .await?;

        let mut client_guard = client.write().await;
        client_guard.upload_file(&upload).await
    }

    /// Gets a cached SSH client with recording using a structured request/context pair.
    ///
    /// Use this when you want full recording output.
    pub async fn get_with_recording_and_context(
        &self,
        request: ConnectionRequest,
        context: ExecutionContext,
    ) -> Result<(mpsc::Sender<CmdJob>, SessionRecorder), ConnectError> {
        self.get_with_recording_level_and_context(request, context, SessionRecordLevel::Full)
            .await
    }

    /// Gets a cached SSH client with recording using a structured request/context pair.
    pub async fn get_with_recording_level_and_context(
        &self,
        request: ConnectionRequest,
        context: ExecutionContext,
        level: SessionRecordLevel,
    ) -> Result<(mpsc::Sender<CmdJob>, SessionRecorder), ConnectError> {
        self.get_with_recorder_and_context(request, context, SessionRecorder::new(level))
            .await
    }

    /// Gets a cached SSH client bound to a caller-provided recorder.
    ///
    /// Use this to attach a pre-configured recorder, e.g. one carrying a
    /// redactor ([`SessionRecorder::with_redactor`]) that scrubs secrets
    /// before events are stored.
    pub async fn get_with_recorder_and_context(
        &self,
        request: ConnectionRequest,
        context: ExecutionContext,
        recorder: SessionRecorder,
    ) -> Result<(mpsc::Sender<CmdJob>, SessionRecorder), ConnectError> {
        let (sender, _client) = self
            .get_client_with_request_and_recording(
                request,
                context.security_options,
                context.connect_timeout,
                Some(recorder.clone()),
            )
            .await?;
        Ok((sender, recorder))
    }

    /// Returns a healthy pooled connection, creating one when necessary.
    ///
    /// Concurrent callers for the same device share one connection attempt
    /// (single-flight) instead of racing to create duplicate connections.
    /// The pooled client is returned directly so callers never need a
    /// second cache lookup.
    async fn get_client_with_request_and_recording(
        &self,
        request: ConnectionRequest,
        security_options: ConnectionSecurityOptions,
        connect_timeout: Duration,
        recorder: Option<SessionRecorder>,
    ) -> Result<(mpsc::Sender<CmdJob>, Arc<RwLock<SharedSshClient>>), ConnectError> {
        self.spawn_maintenance_if_needed();

        let device_addr = request.device_addr();
        let ConnectionRequest {
            user,
            addr,
            port,
            auth,
            enable_password,
            handler,
        } = request;
        let auth_digest = auth.fingerprint().await?;

        // One retry: the first pass may find a stale or mismatched entry,
        // invalidate it, and create a fresh connection on the second pass.
        for _attempt in 0..2 {
            let entry = self
                .cache
                .entry(device_addr.clone())
                .or_try_insert_with(Self::create_connection(
                    device_addr.clone(),
                    user.clone(),
                    addr.clone(),
                    port,
                    auth.clone(),
                    auth_digest,
                    enable_password.clone(),
                    handler.clone(),
                    security_options.clone(),
                    connect_timeout,
                    recorder.clone(),
                ))
                .await
                .map_err(unwrap_shared_connect_error)?;

            let is_fresh = entry.is_fresh();
            let (sender, client) = entry.into_value();

            if is_fresh {
                debug!("New connection for {} has been cached.", device_addr);
                return Ok((sender, client));
            }

            debug!("Cache hit: {}", device_addr);
            let reusable = {
                let client_guard = client.read().await;
                client_guard.is_connected()
                    && client_guard.matches_connection_params(
                        &auth_digest,
                        &enable_password,
                        &handler,
                        &security_options,
                    )
            };

            if reusable {
                debug!("Cached connection params match, reusing: {}", device_addr);
                if recorder.is_some() {
                    client.write().await.recorder = recorder.clone();
                }
                return Ok((sender, client));
            }

            debug!(
                "Cached connection {} is stale or params mismatch, recreating.",
                device_addr
            );
            // Eviction drops the pool's sender; the old connection's worker
            // closes it once the last sender is gone.
            self.cache.invalidate(&device_addr).await;
        }

        Err(ConnectError::InternalServerError(format!(
            "failed to establish a usable pooled connection for {device_addr}"
        )))
    }

    /// Establishes a new SSH connection and spawns its command worker.
    #[allow(clippy::too_many_arguments)]
    async fn create_connection(
        device_addr: String,
        user: String,
        addr: String,
        port: u16,
        auth: SshAuthMethod,
        auth_digest: [u8; 32],
        enable_password: Option<String>,
        handler: DeviceHandler,
        security_options: ConnectionSecurityOptions,
        connect_timeout: Duration,
        recorder: Option<SessionRecorder>,
    ) -> Result<(mpsc::Sender<CmdJob>, Arc<RwLock<SharedSshClient>>), ConnectError> {
        debug!("Creating new connection for {}...", device_addr);

        // `new` automatically detects prompt and ensures shell is ready.
        let ssh_client = SharedSshClient::new(
            user,
            addr,
            port,
            auth,
            auth_digest,
            enable_password,
            handler,
            security_options,
            connect_timeout,
            recorder,
        )
        .await?;
        let client_arc = Arc::new(RwLock::new(ssh_client));

        let (tx, mut rx) = mpsc::channel::<CmdJob>(32);

        let client_clone = client_arc.clone();
        let worker_device_addr = device_addr;

        tokio::spawn(async move {
            loop {
                if let Some(job) = rx.recv().await {
                    if !client_clone.read().await.is_connected() {
                        let _ = job.responder.send(Err(ConnectError::ConnectClosedError));
                        break;
                    }
                    let res = match job.data.into_flow() {
                        Err(error) => Err(error),
                        Ok(flow) if flow.steps.len() != 1 => Err(ConnectError::InvalidCommandFlow(
                            "CmdJob supports one concrete command; use the multiline manager API"
                                .to_string(),
                        )),
                        Ok(mut flow) => {
                            let command = flow
                                .steps
                                .pop()
                                .expect("single-command flow should contain one step");
                            let Command {
                                mode,
                                command,
                                timeout,
                                dyn_params,
                                interaction,
                                ..
                            } = command;
                            let timeout = Duration::from_secs(timeout.unwrap_or(60));
                            let mut client_guard = client_clone.write().await;
                            client_guard
                                .write_with_mode_and_timeout_using_command(
                                    &command,
                                    &mode,
                                    job.sys.as_ref(),
                                    timeout,
                                    &dyn_params,
                                    &interaction,
                                )
                                .await
                        }
                    };

                    let _ = job.responder.send(res);
                } else {
                    // All senders are gone: the pool evicted its handle and
                    // no caller holds one. This is the single owner of
                    // connection shutdown — close gracefully so
                    // `before_disconnect` hooks run and the device-side VTY
                    // session is released instead of leaking.
                    debug!(
                        "Command channel closed for {}, closing connection and stopping worker.",
                        worker_device_addr
                    );
                    let mut client_guard = client_clone.write().await;
                    if client_guard.is_connected()
                        && let Err(error) = client_guard.close().await
                    {
                        debug!("Error closing connection {}: {}", worker_device_addr, error);
                    }
                    break;
                }
            }
        });

        Ok((tx, client_arc))
    }
}

impl Default for SshConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}
