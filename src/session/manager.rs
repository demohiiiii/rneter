use super::*;

impl SshConnectionManager {
    /// Creates a new SSH connection manager.
    pub fn new() -> Self {
        // Cache up to 100 connections. Evict after 5 minutes of inactivity.
        let cache = Cache::builder()
            .max_capacity(100)
            .time_to_idle(Duration::from_secs(5 * 60)) // Evict after 5 minutes idle
            .build();

        Self { cache }
    }

    /// Gets a cached SSH client using a structured request/context pair.
    pub async fn get_with_context(
        &self,
        request: ConnectionRequest,
        context: ExecutionContext,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        self.get_with_request_and_recording(request, context.security_options, None)
            .await
    }

    /// Execute a single command directly using a structured connection/context pair.
    pub async fn execute_command_with_context(
        &self,
        request: ConnectionRequest,
        command: Command,
        context: ExecutionContext,
    ) -> Result<Output, ConnectError> {
        let mut result = self
            .execute_operation_with_context(request, SessionOperation::from(command), context)
            .await?;
        match result.outputs.len() {
            1 => Ok(result.outputs.remove(0)),
            count => Err(ConnectError::InternalServerError(format!(
                "expected one output for command execution, got {count}"
            ))),
        }
    }

    /// Execute any supported session operation using a structured connection/context pair.
    pub async fn execute_operation_with_context(
        &self,
        request: ConnectionRequest,
        operation: SessionOperation,
        context: ExecutionContext,
    ) -> Result<CommandFlowOutput, ConnectError> {
        let device_addr = request.device_addr();
        let sys = context.sys.clone();
        self.get_with_request_and_recording(request, context.security_options, None)
            .await?;

        let (_sender, client) = self.cache.get(&device_addr).await.ok_or_else(|| {
            ConnectError::InternalServerError("connection cache miss".to_string())
        })?;

        let mut client_guard = client.write().await;
        client_guard
            .execute_operation(&operation, sys.as_ref())
            .await
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
    }

    /// Execute a transaction-like block with structured connection/context options.
    pub async fn execute_tx_block_with_context(
        &self,
        request: ConnectionRequest,
        block: TxBlock,
        context: ExecutionContext,
    ) -> Result<TxResult, ConnectError> {
        let device_addr = request.device_addr();
        let sys = context.sys.clone();
        self.get_with_request_and_recording(request, context.security_options, None)
            .await?;

        let (_sender, client) = self.cache.get(&device_addr).await.ok_or_else(|| {
            ConnectError::InternalServerError("connection cache miss".to_string())
        })?;

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
        let device_addr = request.device_addr();
        let sys = context.sys.clone();
        self.get_with_request_and_recording(request, context.security_options, None)
            .await?;

        let (_sender, client) = self.cache.get(&device_addr).await.ok_or_else(|| {
            ConnectError::InternalServerError("connection cache miss".to_string())
        })?;

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
        let device_addr = request.device_addr();
        self.get_with_request_and_recording(request, context.security_options, None)
            .await?;

        let (_sender, client) = self.cache.get(&device_addr).await.ok_or_else(|| {
            ConnectError::InternalServerError("connection cache miss".to_string())
        })?;

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
        let recorder = SessionRecorder::new(level);
        let sender = self
            .get_with_request_and_recording(
                request,
                context.security_options,
                Some(recorder.clone()),
            )
            .await?;
        Ok((sender, recorder))
    }

    async fn get_with_request_and_recording(
        &self,
        request: ConnectionRequest,
        security_options: ConnectionSecurityOptions,
        recorder: Option<SessionRecorder>,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        let device_addr = request.device_addr();
        let ConnectionRequest {
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
        } = request;

        // Check if a healthy, usable connection exists in the cache
        if let Some((sender, client)) = self.cache.get(&device_addr).await {
            debug!("Cache hit: {}", device_addr);

            let client_guard = client.read().await;
            if client_guard.is_connected() {
                // Check if connection parameters match
                if client_guard.matches_connection_params(
                    &password,
                    &enable_password,
                    &handler,
                    &security_options,
                ) {
                    debug!("Cached connection params match, reusing: {}", device_addr);
                    if recorder.is_some() {
                        drop(client_guard);
                        let mut client_guard = client.write().await;
                        client_guard.recorder = recorder.clone();
                    }
                    return Ok(sender);
                } else {
                    debug!(
                        "Cached connection params mismatch, recreating: {}",
                        device_addr
                    );
                    // Release read lock
                    drop(client_guard);

                    // Safely disconnect the old connection
                    match self
                        .safely_disconnect_cached_connection(&device_addr, client.clone())
                        .await
                    {
                        Ok(_) => debug!("Old connection safely disconnected: {}", device_addr),
                        Err(e) => debug!(
                            "Error disconnecting old connection: {} - {}",
                            device_addr, e
                        ),
                    }

                    // Remove from cache
                    self.cache.invalidate(&device_addr).await;
                }
            } else {
                // If connection is closed, remove from cache
                debug!("Cached connection {} is closed. Removing.", device_addr);
                self.cache.invalidate(&device_addr).await;
            }
        } else {
            debug!("Cache miss, creating new connection for {}...", device_addr);
        }

        // Create a new client. `new` automatically detects prompt and ensures shell is ready.
        let ssh_client = SharedSshClient::new(
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
            security_options,
            recorder,
        )
        .await?;
        let client_arc = Arc::new(RwLock::new(ssh_client));

        let (tx, mut rx) = mpsc::channel::<CmdJob>(32);

        let client_clone = client_arc.clone();
        let worker_device_addr = device_addr.clone();

        tokio::spawn(async move {
            loop {
                if let Some(job) = rx.recv().await {
                    if !client_clone.read().await.is_connected() {
                        let _ = job.responder.send(Err(ConnectError::ConnectClosedError));
                        break;
                    }
                    let res = {
                        let mut client_guard = client_clone.write().await;
                        let Command {
                            mode,
                            command,
                            timeout,
                            dyn_params,
                            interaction,
                        } = job.data;
                        let timeout = Duration::from_secs(timeout.unwrap_or(60));
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
                    };

                    let _ = job.responder.send(res);
                } else {
                    debug!(
                        "Command channel closed for {}, stopping worker.",
                        worker_device_addr
                    );
                    break;
                }
            }
        });

        self.cache
            .insert(device_addr.clone(), (tx.clone(), client_arc))
            .await;
        debug!("New connection for {} has been cached.", device_addr);

        Ok(tx)
    }

    /// Safely disconnects a cached connection.
    async fn safely_disconnect_cached_connection(
        &self,
        device_addr: &str,
        client_arc: Arc<RwLock<SharedSshClient>>,
    ) -> Result<(), ConnectError> {
        debug!("Safely disconnecting cached connection: {}", device_addr);

        // Get write lock to ensure exclusive access
        let mut client_guard = client_arc.write().await;

        // Check if connection is still active
        if !client_guard.is_connected() {
            debug!("Connection {} already disconnected, skipping", device_addr);
            return Ok(());
        }

        // Safely close connection
        match client_guard.close().await {
            Ok(_) => {
                debug!("Connection {} safely closed", device_addr);
                Ok(())
            }
            Err(e) => {
                debug!("Error closing connection {}: {}", device_addr, e);
                // Consider success even on error as connection will be dropped
                Ok(())
            }
        }
    }
}

impl Default for SshConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}
