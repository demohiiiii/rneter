use super::super::*;

impl SharedSshClient {
    /// Uploads a local file to the remote host using the SSH `sftp` subsystem.
    pub async fn upload_file(&mut self, upload: &FileUploadRequest) -> Result<(), ConnectError> {
        let local_path = upload.local_path.clone();
        let remote_path = upload.remote_path.clone();

        if let Some(recorder) = self.recorder.as_ref() {
            let _ = recorder.record_event(SessionEvent::FileUploadStarted {
                local_path: local_path.clone(),
                remote_path: remote_path.clone(),
            });
        }

        let result = self
            .client
            .upload_file(
                local_path.as_str(),
                remote_path.clone(),
                upload.timeout_secs,
                upload.buffer_size,
                upload.show_progress,
            )
            .await;

        match result {
            Ok(()) => {
                if let Some(recorder) = self.recorder.as_ref() {
                    let _ = recorder.record_event(SessionEvent::FileUploadFinished {
                        local_path,
                        remote_path,
                        success: true,
                        error: None,
                    });
                }
                Ok(())
            }
            Err(err) => {
                let reason = err.to_string();
                if let Some(recorder) = self.recorder.as_ref() {
                    let _ = recorder.record_event(SessionEvent::FileUploadFinished {
                        local_path,
                        remote_path,
                        success: false,
                        error: Some(reason),
                    });
                }
                Err(err.into())
            }
        }
    }
}
