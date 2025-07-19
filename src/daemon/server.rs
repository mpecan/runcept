use crate::cli::commands::{DaemonRequest, DaemonResponse};
use crate::config::{EnvironmentManager, GlobalConfig};
use crate::daemon::connection::ConnectionHandler;
use crate::daemon::handlers::{DaemonHandles, EnvironmentHandles, ProcessHandles};
use crate::database::Database;
use crate::error::{Result, RunceptError};
use crate::process::ProcessManager;
use crate::scheduler::InactivityScheduler;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::net::UnixListener;
use tokio::sync::{RwLock, mpsc};
use tracing::{error, info};

/// Configuration for the daemon server
pub struct ServerConfig {
    pub socket_path: PathBuf,
    pub global_config: GlobalConfig,
    pub database: Option<Database>,
}

/// Main daemon server that handles RPC requests
pub struct DaemonServer {
    pub config: ServerConfig,
    pub process_manager: Arc<RwLock<ProcessManager>>,
    pub environment_manager: Arc<RwLock<EnvironmentManager>>,
    pub inactivity_scheduler: Arc<RwLock<Option<InactivityScheduler>>>,
    pub start_time: SystemTime,
    pub shutdown_tx: Option<mpsc::Sender<()>>,
    pub current_environment_id: Arc<RwLock<Option<String>>>,
}

impl DaemonServer {
    /// Create a new daemon server
    pub async fn new(config: ServerConfig) -> Result<Self> {
        // Create environment manager with optional database
        let database_pool = config.database.as_ref().map(|db| db.get_pool().clone());
        let database_pool_arc = database_pool.as_ref().map(|pool| Arc::new(pool.clone()));
        let environment_manager = Arc::new(RwLock::new(
            EnvironmentManager::new_with_database(config.global_config.clone(), database_pool)
                .await?,
        ));

        let process_manager = Arc::new(RwLock::new(
            ProcessManager::new_with_environment_manager(
                config.global_config.clone(),
                environment_manager.clone(),
                database_pool_arc,
            )
            .await?,
        ));

        // Create inactivity scheduler
        let inactivity_scheduler = Arc::new(RwLock::new(None));

        // Start the config event loop for the environment manager
        EnvironmentManager::start_config_event_loop(environment_manager.clone());

        let server = Self {
            config,
            process_manager,
            environment_manager,
            inactivity_scheduler,
            start_time: SystemTime::now(),
            shutdown_tx: None,
            current_environment_id: Arc::new(RwLock::new(None)),
        };

        // Perform startup cleanup if database is available
        if let Some(database) = &server.config.database {
            // Only perform cleanup if database is properly initialized
            if let Err(e) = server.perform_startup_cleanup(database).await {
                error!("Startup cleanup failed: {}", e);
                // Continue with startup even if cleanup fails
            }
        }

        Ok(server)
    }

    /// Perform startup cleanup of stale processes and environments
    async fn perform_startup_cleanup(&self, database: &crate::database::Database) -> Result<()> {
        info!("Performing startup cleanup of stale processes and environments");

        // Create query manager for database operations
        let query_manager = crate::database::QueryManager::new(database.get_pool());

        // Cleanup stale environments first
        {
            let mut env_manager = self.environment_manager.write().await;
            if let Err(e) = env_manager.cleanup_stale_environments(&query_manager).await {
                error!("Failed to cleanup stale environments: {}", e);
                // Continue with startup even if environment cleanup fails
            }
        }

        // Cleanup stale processes via the runtime manager
        {
            let process_manager = self.process_manager.write().await;
            if let Err(e) = process_manager
                .runtime_manager
                .write()
                .await
                .cleanup_stale_processes()
                .await
            {
                error!("Failed to cleanup stale processes: {}", e);
                // Continue with startup even if process cleanup fails
            }
        }

        info!("Startup cleanup completed");
        Ok(())
    }

    /// Start the daemon server
    pub async fn run(&mut self) -> Result<()> {
        info!(
            "Starting daemon server at {}",
            self.config.socket_path.display()
        );

        // Set up shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Remove existing socket file if it exists
        if self.config.socket_path.exists() {
            std::fs::remove_file(&self.config.socket_path).map_err(|e| {
                RunceptError::IoError(std::io::Error::other(
                    format!(
                        "Failed to remove existing socket {}: {}",
                        self.config.socket_path.display(),
                        e
                    ),
                ))
            })?;
        }

        // Create Unix socket listener
        let listener = UnixListener::bind(&self.config.socket_path).map_err(|e| {
            RunceptError::IoError(std::io::Error::other(
                format!(
                    "Failed to bind to socket {}: {}",
                    self.config.socket_path.display(),
                    e
                ),
            ))
        })?;

        info!(
            "Daemon server listening on {}",
            self.config.socket_path.display()
        );

        // Create handler instances
        let process_handles = ProcessHandles::new(
            self.process_manager.clone(),
            self.current_environment_id.clone(),
        );

        let environment_handles = EnvironmentHandles::new(
            process_handles.clone(),
            self.environment_manager.clone(),
            self.inactivity_scheduler.clone(),
            self.current_environment_id.clone(),
            self.config.global_config.clone(),
        );

        let daemon_handles = DaemonHandles::new(
            self.process_manager.clone(),
            self.environment_manager.clone(),
            self.inactivity_scheduler.clone(),
            self.current_environment_id.clone(),
            self.config.socket_path.clone(),
            self.start_time,
            self.shutdown_tx.clone(),
        );

        // Create request processor
        let request_processor = RequestProcessor {
            process_handles,
            environment_handles,
            daemon_handles,
        };

        // Main server loop
        loop {
            tokio::select! {
                // Handle new connections
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let processor = request_processor.clone();
                            tokio::spawn(async move {
                                if let Err(e) = ConnectionHandler::handle_connection(
                                    stream,
                                    |request| processor.process_request(request.clone())
                                ).await {
                                    error!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }

                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal, stopping server");
                    break;
                }
            }
        }

        // Cleanup
        self.shutdown().await?;
        Ok(())
    }

    /// Shutdown the server
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down daemon server");

        // Stop all running processes gracefully before shutting down
        {
            info!("Stopping all running processes before daemon shutdown");
            let process_manager = self.process_manager.write().await;

            // Get all active environments and stop their processes
            let env_manager = self.environment_manager.read().await;
            let active_environments = env_manager.get_active_environments();

            for (env_id, _) in active_environments {
                info!("Stopping processes in environment '{}'", env_id);

                // Stop all processes in this environment
                if let Err(e) = process_manager
                    .runtime_manager
                    .write()
                    .await
                    .stop_all_processes(env_id)
                    .await
                {
                    error!(
                        "Failed to stop processes in environment '{}': {}",
                        env_id, e
                    );
                }
            }

            // Give processes time to stop gracefully
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

            info!("All processes have been stopped");
        }

        // Deactivate all environments before shutting down
        {
            info!("Deactivating all environments before daemon shutdown");
            let mut env_manager = self.environment_manager.write().await;
            if let Err(e) = env_manager.deactivate_all_environments().await {
                error!("Failed to deactivate all environments: {}", e);
            } else {
                info!("All environments have been deactivated");
            }
        }

        // Stop inactivity scheduler
        {
            let mut scheduler_guard = self.inactivity_scheduler.write().await;
            if let Some(mut scheduler) = scheduler_guard.take() {
                let _ = scheduler.stop().await;
            }
        }

        // Remove socket file
        if self.config.socket_path.exists() {
            if let Err(e) = std::fs::remove_file(&self.config.socket_path) {
                error!("Failed to remove socket file: {}", e);
            }
        }

        Ok(())
    }

    /// Request shutdown
    pub async fn request_shutdown(&self) -> Result<()> {
        if let Some(tx) = &self.shutdown_tx {
            tx.send(()).await.map_err(|e| {
                RunceptError::EnvironmentError(format!("Failed to send shutdown signal: {e}"))
            })?;
        }
        Ok(())
    }
}

/// Request processor that routes requests to appropriate handlers
#[derive(Clone)]
struct RequestProcessor {
    process_handles: ProcessHandles,
    environment_handles: EnvironmentHandles,
    daemon_handles: DaemonHandles,
}

impl RequestProcessor {
    /// Process a daemon request and return response
    async fn process_request(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        match request {
            // Environment commands
            DaemonRequest::ActivateEnvironment { path } => {
                self.environment_handles.activate_environment(path).await
            }
            DaemonRequest::DeactivateEnvironment => {
                self.environment_handles.deactivate_environment().await
            }
            DaemonRequest::GetEnvironmentStatus => {
                self.environment_handles.get_environment_status().await
            }
            DaemonRequest::InitProject { path, force } => {
                self.environment_handles.init_project(path, force).await
            }

            // Process commands
            DaemonRequest::StartProcess { name, environment } => {
                self.process_handles.start_process(name, environment).await
            }
            DaemonRequest::StopProcess { name, environment } => {
                self.process_handles.stop_process(name, environment).await
            }
            DaemonRequest::RestartProcess { name, environment } => {
                self.process_handles
                    .restart_process(name, environment)
                    .await
            }
            DaemonRequest::ListProcesses { environment } => {
                self.process_handles.list_processes(environment).await
            }
            DaemonRequest::ListAllProcesses => self.process_handles.list_all_processes().await,
            DaemonRequest::KillAllProcesses => self.process_handles.kill_all_processes().await,

            // Daemon commands
            DaemonRequest::GetDaemonStatus => self.daemon_handles.get_daemon_status().await,
            DaemonRequest::RecordEnvironmentActivity { environment_id } => {
                self.daemon_handles
                    .record_environment_activity(environment_id)
                    .await
            }
            DaemonRequest::Shutdown => {
                // Delegate to daemon handles to actually trigger shutdown
                self.daemon_handles.request_shutdown().await
            }

            // Process management commands
            DaemonRequest::GetProcessLogs {
                name,
                lines,
                environment,
            } => {
                self.process_handles
                    .get_process_logs(name, lines, environment)
                    .await
            }
            DaemonRequest::AddProcess {
                process,
                environment,
            } => self.process_handles.add_process(process, environment).await,
            DaemonRequest::RemoveProcess { name, environment } => {
                self.process_handles.remove_process(name, environment).await
            }
            DaemonRequest::UpdateProcess {
                name,
                process,
                environment,
            } => {
                self.process_handles
                    .update_process(name, process, environment)
                    .await
            }
        }
    }
}

// Keep the original tests for compatibility
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::{DaemonRequest, DaemonResponse};
    use crate::config::GlobalConfig;
    use crate::database::Database;
    use tempfile::TempDir;

    /// Create an in-memory database for testing
    async fn create_test_database() -> Database {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        db
    }

    #[tokio::test]
    async fn test_daemon_server_creation() {
        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");

        // Create test database
        let database = create_test_database().await;

        let config = ServerConfig {
            socket_path: socket_path.clone(),
            global_config: GlobalConfig::default(),
            database: Some(database),
        };

        let server = DaemonServer::new(config).await.unwrap();
        assert_eq!(server.config.socket_path, socket_path);
    }

    #[test]
    fn test_request_processing() {
        let request = DaemonRequest::GetDaemonStatus;
        let json = serde_json::to_string(&request).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();

        match parsed {
            DaemonRequest::GetDaemonStatus => {}
            _ => panic!("Expected GetDaemonStatus"),
        }
    }

    #[test]
    fn test_response_serialization() {
        let response = DaemonResponse::Success {
            message: "Test message".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();

        match parsed {
            DaemonResponse::Success { message } => {
                assert_eq!(message, "Test message");
            }
            _ => panic!("Expected Success response"),
        }
    }
}
