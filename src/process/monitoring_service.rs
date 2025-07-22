use crate::process::{ProcessLogger, ProcessRepositoryTrait};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast, mpsc};
use tracing::{debug, error, info};

/// Notification sent when a process exits
#[derive(Debug, Clone)]
pub struct ProcessExitNotification {
    pub process_name: String,
    pub environment_id: String,
    pub exit_status: std::process::ExitStatus,
}

/// Service for monitoring process exit events and updating database status
pub struct ProcessMonitoringService<R>
where
    R: ProcessRepositoryTrait,
{
    /// Process repository for database operations
    pub process_repository: Arc<R>,
    /// Channel for broadcasting process exit notifications
    pub exit_notification_tx: broadcast::Sender<ProcessExitNotification>,
}

impl<R> ProcessMonitoringService<R>
where
    R: ProcessRepositoryTrait + 'static,
{
    /// Create a new ProcessMonitoringService
    pub fn new(process_repository: Arc<R>) -> Self {
        let (exit_notification_tx, _) = broadcast::channel(100);

        let mut service = Self {
            process_repository,
            exit_notification_tx,
        };

        // Start the exit notification listener task
        service.start_exit_notification_listener();
        service
    }

    /// Get a receiver for exit notifications
    pub fn subscribe_to_exit_notifications(&self) -> broadcast::Receiver<ProcessExitNotification> {
        self.exit_notification_tx.subscribe()
    }

    /// Start a background task to listen for process exit notifications
    /// and update database status accordingly
    fn start_exit_notification_listener(&mut self) {
        let mut rx = self.exit_notification_tx.subscribe();
        let repository = self.process_repository.clone();

        tokio::spawn(async move {
            while let Ok(notification) = rx.recv().await {
                debug!(
                    "Received exit notification for process '{}' in environment '{}' with status: {}",
                    notification.process_name,
                    notification.environment_id,
                    notification.exit_status
                );

                // Determine the new status based on exit code and current status
                let new_status = match repository
                    .get_process_by_name(&notification.environment_id, &notification.process_name)
                    .await
                {
                    Ok(Some(process)) if process.status == "stopping" => {
                        // Process was deliberately stopped via stop command, mark as stopped
                        "stopped"
                    }
                    _ => {
                        // Process exited naturally or crashed
                        if notification.exit_status.success() {
                            "stopped"
                        } else {
                            "crashed"
                        }
                    }
                };

                // Update process status in database
                if let Err(e) = repository
                    .update_process_status(
                        &notification.environment_id,
                        &notification.process_name,
                        new_status,
                    )
                    .await
                {
                    error!(
                        "Failed to update process status for '{}:{}' to '{}': {}",
                        notification.environment_id, notification.process_name, new_status, e
                    );
                }

                // Clear the PID since process has exited
                if let Err(e) = repository
                    .clear_process_pid(&notification.environment_id, &notification.process_name)
                    .await
                {
                    error!(
                        "Failed to clear PID for process '{}:{}': {}",
                        notification.environment_id, notification.process_name, e
                    );
                }

                // Log the process exit event
                info!(
                    "Process '{}:{}' exited with code {:?}, new status: '{}'",
                    notification.environment_id,
                    notification.process_name,
                    notification.exit_status.code(),
                    new_status
                );

                info!(
                    "Updated process '{}:{}' status to '{}' after exit with code {:?}",
                    notification.environment_id,
                    notification.process_name,
                    new_status,
                    notification.exit_status.code()
                );
            }
        });
    }

    /// Spawn a task to monitor process completion and handle shutdown signals
    pub fn spawn_process_monitoring_task(
        child_handle: Arc<Mutex<Option<tokio::process::Child>>>,
        process_name: String,
        mut shutdown_rx: mpsc::Receiver<()>,
        environment_id: String,
        exit_notification_tx: broadcast::Sender<ProcessExitNotification>,
        working_dir: PathBuf,
    ) {
        tokio::spawn(async move {
            let mut child_guard = child_handle.lock().await;
            if let Some(mut child) = child_guard.take() {
                drop(child_guard); // Release the lock while waiting

                // Wait for the process to complete or receive shutdown signal
                let mut shutdown_received = false;
                tokio::select! {
                    result = child.wait() => {
                        match result {
                            Ok(exit_status) => {
                                info!(
                                    "Process '{}' in environment '{}' exited with status: {}",
                                    process_name, environment_id, exit_status
                                );

                                // Send exit notification
                                let notification = ProcessExitNotification {
                                    process_name: process_name.clone(),
                                    environment_id: environment_id.clone(),
                                    exit_status,
                                };

                                if let Ok(mut logger) =
                                    ProcessLogger::new(process_name.clone(), working_dir.to_path_buf()).await
                                {
                                    let _ = logger
                                        .log_lifecycle(&format!(
                                            "Process '{process_name}' exited with status {exit_status}"
                                        ))
                                        .await;
                                }

                                if let Err(e) = exit_notification_tx.send(notification) {
                                    error!(
                                        "Failed to send exit notification for process '{}' in environment '{}': {}",
                                        process_name, environment_id, e
                                    );
                                }
                            }
                            Err(e) => {
                                error!(
                                    "Error waiting for process '{}' in environment '{}': {}",
                                    process_name, environment_id, e
                                );
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        debug!(
                            "Received shutdown signal for process '{}' in environment '{}', but still waiting for actual exit",
                            process_name, environment_id
                        );
                        shutdown_received = true;
                    }
                }

                // If we received a shutdown signal, we still need to wait for the actual process exit
                if shutdown_received {
                    debug!(
                        "Waiting for process '{}' in environment '{}' to actually exit after shutdown signal",
                        process_name, environment_id
                    );

                    match child.wait().await {
                        Ok(exit_status) => {
                            info!(
                                "Process '{}' in environment '{}' exited after shutdown with status: {}",
                                process_name, environment_id, exit_status
                            );

                            // Send exit notification
                            let notification = ProcessExitNotification {
                                process_name: process_name.clone(),
                                environment_id: environment_id.clone(),
                                exit_status,
                            };

                            if let Ok(mut logger) =
                                ProcessLogger::new(process_name.clone(), working_dir.to_path_buf())
                                    .await
                            {
                                let _ = logger
                                    .log_lifecycle(&format!(
                                        "Process '{process_name}' exited after shutdown with status {exit_status}"
                                    ))
                                    .await;
                            }

                            if let Err(e) = exit_notification_tx.send(notification) {
                                error!(
                                    "Failed to send exit notification for process '{}' in environment '{}': {}",
                                    process_name, environment_id, e
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "Error waiting for process '{}' in environment '{}' to exit after shutdown: {}",
                                process_name, environment_id, e
                            );
                        }
                    }
                }
            }
        });
    }
}

// Type alias for the default concrete implementation
pub type DefaultProcessMonitoringService =
    ProcessMonitoringService<crate::database::ProcessRepository>;

impl DefaultProcessMonitoringService {
    /// Create a new ProcessMonitoringService with default implementations
    pub fn new_default(db_pool: Arc<sqlx::Pool<sqlx::Sqlite>>) -> Self {
        let process_repository = Arc::new(crate::database::ProcessRepository::new(db_pool));
        Self::new(process_repository)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{MockProcessRepository, ProcessRecordBuilder, TempDirFixture};
    use std::os::unix::process::ExitStatusExt;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_monitoring_service_creation() {
        let mock_repo = Arc::new(MockProcessRepository::new());
        let service = ProcessMonitoringService::new(mock_repo);

        // Receiver count is always >= 0, so just check that we can get the count
        let _receiver_count = service.exit_notification_tx.receiver_count();
    }

    #[tokio::test]
    async fn test_exit_notification_subscription() {
        let mock_repo = Arc::new(MockProcessRepository::new());
        let service = ProcessMonitoringService::new(mock_repo);

        let mut rx1 = service.subscribe_to_exit_notifications();
        let mut rx2 = service.subscribe_to_exit_notifications();

        // Send a test notification
        let notification = ProcessExitNotification {
            process_name: "test-process".to_string(),
            environment_id: "test-env".to_string(),
            exit_status: std::process::ExitStatus::from_raw(0),
        };

        service
            .exit_notification_tx
            .send(notification.clone())
            .unwrap();

        // Both receivers should get the notification
        let received1 = rx1.recv().await.unwrap();
        let received2 = rx2.recv().await.unwrap();

        assert_eq!(received1.process_name, notification.process_name);
        assert_eq!(received1.environment_id, notification.environment_id);
        assert_eq!(received2.process_name, notification.process_name);
        assert_eq!(received2.environment_id, notification.environment_id);
    }

    #[tokio::test]
    async fn test_successful_exit_status_update() {
        let mock_repo = Arc::new(MockProcessRepository::new());

        // Add a process that's not in "stopping" state using the builder
        let test_process = ProcessRecordBuilder::new("test-process")
            .with_environment("test-env")
            .with_status("running")
            .with_pid(1234)
            .build();
        mock_repo.add_process(test_process);

        let service = ProcessMonitoringService::new(mock_repo.clone());

        // Send an exit notification with successful status
        let notification = ProcessExitNotification {
            process_name: "test-process".to_string(),
            environment_id: "test-env".to_string(),
            exit_status: std::process::ExitStatus::from_raw(0), // Success
        };

        service.exit_notification_tx.send(notification).unwrap();

        // Wait a bit for the background task to process
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Check that status was updated to "stopped"
        let status_updates = mock_repo.get_status_updates();
        assert_eq!(status_updates.len(), 1);
        assert_eq!(
            status_updates[0],
            (
                "test-env".to_string(),
                "test-process".to_string(),
                "stopped".to_string()
            )
        );

        // Check that PID was cleared
        let pid_clears = mock_repo.get_pid_clears();
        assert_eq!(pid_clears.len(), 1);
        assert_eq!(
            pid_clears[0],
            ("test-env".to_string(), "test-process".to_string())
        );
    }

    #[tokio::test]
    async fn test_failed_exit_status_update() {
        let mock_repo = Arc::new(MockProcessRepository::new());

        // Add a process that's not in "stopping" state using the builder
        let test_process = ProcessRecordBuilder::new("test-process")
            .with_environment("test-env")
            .with_status("running")
            .with_pid(1234)
            .build();
        mock_repo.add_process(test_process);

        let service = ProcessMonitoringService::new(mock_repo.clone());

        // Send an exit notification with failure status
        let notification = ProcessExitNotification {
            process_name: "test-process".to_string(),
            environment_id: "test-env".to_string(),
            exit_status: std::process::ExitStatus::from_raw(256), // Failure (exit code 1)
        };

        service.exit_notification_tx.send(notification).unwrap();

        // Wait a bit for the background task to process
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Check that status was updated to "crashed"
        let status_updates = mock_repo.get_status_updates();
        assert_eq!(status_updates.len(), 1);
        assert_eq!(
            status_updates[0],
            (
                "test-env".to_string(),
                "test-process".to_string(),
                "crashed".to_string()
            )
        );
    }

    #[tokio::test]
    async fn test_stopping_process_marked_as_stopped() {
        let mock_repo = Arc::new(MockProcessRepository::new());

        // Add a process in "stopping" state using the builder
        let test_process = ProcessRecordBuilder::new("test-process")
            .with_environment("test-env")
            .with_status("stopping")
            .with_pid(1234)
            .build();
        mock_repo.add_process(test_process);

        let service = ProcessMonitoringService::new(mock_repo.clone());

        // Send an exit notification - doesn't matter if success or failure
        let notification = ProcessExitNotification {
            process_name: "test-process".to_string(),
            environment_id: "test-env".to_string(),
            exit_status: std::process::ExitStatus::from_raw(256), // Would normally be "crashed"
        };

        service.exit_notification_tx.send(notification).unwrap();

        // Wait a bit for the background task to process
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Check that status was updated to "stopped" (not "crashed") because it was stopping
        let status_updates = mock_repo.get_status_updates();
        assert_eq!(status_updates.len(), 1);
        assert_eq!(
            status_updates[0],
            (
                "test-env".to_string(),
                "test-process".to_string(),
                "stopped".to_string()
            )
        );
    }

    #[tokio::test]
    async fn test_spawn_monitoring_task() {
        let temp_dir_fixture = TempDirFixture::new().unwrap();
        let working_dir = temp_dir_fixture.path().to_path_buf();

        let (tx, _rx) = broadcast::channel(10);
        let (_shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);

        // Create a mock child process
        let child = tokio::process::Command::new("sleep")
            .arg("0.1")
            .spawn()
            .unwrap();

        let child_handle = Arc::new(Mutex::new(Some(child)));

        // Spawn monitoring task
        ProcessMonitoringService::<MockProcessRepository>::spawn_process_monitoring_task(
            child_handle,
            "test-process".to_string(),
            shutdown_rx,
            "test-env".to_string(),
            tx.clone(),
            working_dir,
        );

        // Subscribe to notifications
        let mut rx = tx.subscribe();

        // Wait for the process to exit and notification to be sent
        let notification = tokio::time::timeout(tokio::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(notification.process_name, "test-process");
        assert_eq!(notification.environment_id, "test-env");
        assert!(notification.exit_status.success());
    }
}
