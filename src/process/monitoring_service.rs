use crate::process::{ProcessLogger, ProcessRepositoryTrait};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
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
                    notification.process_name, notification.environment_id, notification.exit_status
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
                    .update_process_status(&notification.environment_id, &notification.process_name, new_status)
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
                    notification.environment_id, notification.process_name,
                    notification.exit_status.code(), new_status
                );

                info!(
                    "Updated process '{}:{}' status to '{}' after exit with code {:?}",
                    notification.environment_id, notification.process_name, new_status, notification.exit_status.code()
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
                                ProcessLogger::new(process_name.clone(), working_dir.to_path_buf()).await
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
pub type DefaultProcessMonitoringService = ProcessMonitoringService<crate::database::ProcessRepository>;

impl DefaultProcessMonitoringService {
    /// Create a new ProcessMonitoringService with default implementations
    pub fn new_default(db_pool: Arc<sqlx::Pool<sqlx::Sqlite>>) -> Self {
        let process_repository = Arc::new(crate::database::ProcessRepository::new(db_pool));
        Self::new(process_repository)
    }
}