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

    /// Gets a cached SSH client or creates a new one.
    ///
    /// This method first checks the cache for an existing healthy connection.
    /// If found and the connection parameters match, it reuses the connection.
    /// Otherwise, it creates a new connection, caches it, and returns the sender.
    pub async fn get(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        self.get_with_security(
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
            ConnectionSecurityOptions::default(),
        )
        .await
    }

    /// Execute a transaction-like block on a managed connection.
    ///
    /// This API keeps backward compatibility with `CmdJob` sender-based execution while
    /// providing block-level commit/rollback semantics for configuration command groups.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_tx_block(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
        block: TxBlock,
        sys: Option<String>,
    ) -> Result<TxResult, ConnectError> {
        // Ensure connection exists/reused in cache.
        self.get(
            user.clone(),
            addr.clone(),
            port,
            password,
            enable_password,
            handler,
        )
        .await?;

        let device_addr = format!("{user}@{addr}:{port}");
        let (_sender, client) = self.cache.get(&device_addr).await.ok_or_else(|| {
            ConnectError::InternalServerError("connection cache miss".to_string())
        })?;

        let mut client_guard = client.write().await;
        client_guard.execute_tx_block(&block, sys.as_ref()).await
    }

    /// Execute a multi-block workflow with global rollback semantics.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_tx_workflow(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
        workflow: TxWorkflow,
        sys: Option<String>,
    ) -> Result<TxWorkflowResult, ConnectError> {
        self.get(
            user.clone(),
            addr.clone(),
            port,
            password,
            enable_password,
            handler,
        )
        .await?;

        let device_addr = format!("{user}@{addr}:{port}");
        let (_sender, client) = self.cache.get(&device_addr).await.ok_or_else(|| {
            ConnectError::InternalServerError("connection cache miss".to_string())
        })?;

        let mut client_guard = client.write().await;
        client_guard
            .execute_tx_workflow(&workflow, sys.as_ref())
            .await
    }

    /// Gets a cached SSH client and enables full session recording.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_with_recording(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
    ) -> Result<(mpsc::Sender<CmdJob>, SessionRecorder), ConnectError> {
        self.get_with_recording_level(
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
            SessionRecordLevel::Full,
        )
        .await
    }

    /// Gets a cached SSH client and enables session recording with custom level.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_with_recording_level(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
        level: SessionRecordLevel,
    ) -> Result<(mpsc::Sender<CmdJob>, SessionRecorder), ConnectError> {
        let recorder = SessionRecorder::new(level);
        let sender = self
            .get_with_security_and_recording(
                user,
                addr,
                port,
                password,
                enable_password,
                handler,
                ConnectionSecurityOptions::default(),
                Some(recorder.clone()),
            )
            .await?;
        Ok((sender, recorder))
    }

    /// Gets a cached SSH client or creates a new one with explicit security options.
    #[allow(clippy::too_many_arguments)]
    pub async fn get_with_security(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
        security_options: ConnectionSecurityOptions,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        self.get_with_security_and_recording(
            user,
            addr,
            port,
            password,
            enable_password,
            handler,
            security_options,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_with_security_and_recording(
        &self,
        user: String,
        addr: String,
        port: u16,
        password: String,
        enable_password: Option<String>,
        handler: DeviceHandler,
        security_options: ConnectionSecurityOptions,
        recorder: Option<SessionRecorder>,
    ) -> Result<mpsc::Sender<CmdJob>, ConnectError> {
        let device_addr = format!("{user}@{addr}:{port}");

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
                        let timeout = Duration::from_secs(job.data.timeout.unwrap_or(60));
                        client_guard
                            .write_with_mode_and_timeout(
                                &job.data.command,
                                &job.data.mode,
                                job.sys.as_ref(),
                                timeout,
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
