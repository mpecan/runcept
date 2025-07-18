use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Manages process configurations and interfaces with EnvironmentManager
pub struct ProcessConfigurationManager {
    pub global_config: crate::config::GlobalConfig,
    pub environment_manager: Arc<RwLock<crate::config::EnvironmentManager>>,
}

impl ProcessConfigurationManager {
    /// Create a new ProcessConfigurationManager
    pub fn new(
        global_config: crate::config::GlobalConfig,
        environment_manager: Arc<RwLock<crate::config::EnvironmentManager>>,
    ) -> Self {
        Self {
            global_config,
            environment_manager,
        }
    }

    /// Get a process definition from the specified environment
    pub async fn get_process_definition(
        &self,
        name: &str,
        environment_id: &str,
    ) -> Result<ProcessDefinition> {
        debug!(
            "Getting process definition for '{}' in environment '{}'.",
            name, environment_id
        );

        let environment = self.get_active_environment(environment_id).await?;
        
        let process_def = environment
            .project_config
            .processes
            .get(name)
            .ok_or_else(|| Self::process_not_found_error(name))?;

        Ok(process_def.clone())
    }

    /// Add a process to the specified environment's configuration
    pub async fn add_process(
        &mut self,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        info!(
            "Adding process '{}' to environment '{}'",
            process_def.name, environment_id
        );

        let process_name = process_def.name.clone();
        self.modify_config(environment_id, |config| {
            Self::validate_process_operation(config, &process_name, ProcessOperation::Add)?;
            config.processes.insert(process_name.clone(), process_def.clone());
            Ok(())
        }).await?;

        info!(
            "Successfully added process '{}' to environment '{}'",
            process_name, environment_id
        );
        Ok(())
    }

    /// Remove a process from the specified environment's configuration
    pub async fn remove_process(&mut self, name: &str, environment_id: &str) -> Result<()> {
        info!(
            "Removing process '{}' from environment '{}'",
            name, environment_id
        );

        self.modify_config(environment_id, |config| {
            Self::validate_process_operation(config, name, ProcessOperation::Remove)?;
            config.processes.remove(name);
            Ok(())
        }).await?;

        info!(
            "Successfully removed process '{}' from environment '{}'",
            name, environment_id
        );
        Ok(())
    }

    /// Update a process in the specified environment's configuration
    pub async fn update_process(
        &mut self,
        name: &str,
        process_def: ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        info!(
            "Updating process '{}' in environment '{}'",
            name, environment_id
        );

        self.modify_config(environment_id, |config| {
            Self::validate_process_operation(config, name, ProcessOperation::Update)?;
            // Update the process (ensure the name matches)
            let mut updated_process = process_def.clone();
            updated_process.name = name.to_string();
            config.processes.insert(name.to_string(), updated_process);
            Ok(())
        }).await?;

        info!(
            "Successfully updated process '{}' in environment '{}'",
            name, environment_id
        );
        Ok(())
    }

    /// List all configured processes in the specified environment
    pub async fn list_configured_processes(
        &self,
        environment_id: &str,
    ) -> Result<Vec<ProcessDefinition>> {
        debug!(
            "Listing configured processes in environment '{}'",
            environment_id
        );

        let environment = self.get_active_environment(environment_id).await?;
        
        let processes: Vec<ProcessDefinition> = environment
            .project_config
            .processes
            .values()
            .cloned()
            .collect();

        Ok(processes)
    }

    /// Reload the environment configuration from disk
    pub async fn reload_environment_config(&self, environment_id: &str) -> Result<()> {
        debug!(
            "Reloading environment configuration for '{}'",
            environment_id
        );

        let mut env_manager = self.environment_manager.write().await;
        env_manager
            .reload_environment_config(environment_id)
            .await?;

        debug!(
            "Successfully reloaded environment configuration for '{}'",
            environment_id
        );
        Ok(())
    }

    /// Get the working directory for the specified environment
    pub async fn get_working_directory(&self, environment_id: &str) -> Result<PathBuf> {
        debug!(
            "Getting working directory for environment '{}'",
            environment_id
        );

        // Note: This method doesn't require active environment check for working directory access
        let env_manager = self.environment_manager.read().await;
        let environment = env_manager.get_environment(environment_id).ok_or_else(|| {
            Self::environment_not_found_error(environment_id)
        })?;

        Ok(environment.project_path.clone())
    }

    /// Validate that an environment exists and is active
    async fn validate_environment(&self, environment_id: &str) -> Result<()> {
        let env_manager = self.environment_manager.read().await;
        let environment = env_manager.get_environment(environment_id).ok_or_else(|| {
            Self::environment_not_found_error(environment_id)
        })?;

        if !environment.is_active() {
            return Err(Self::environment_not_active_error(environment_id));
        }

        Ok(())
    }

    /// Get the config file path for the specified environment
    async fn get_config_path(&self, environment_id: &str) -> Result<PathBuf> {
        let working_dir = self.get_working_directory(environment_id).await?;
        Ok(working_dir.join(".runcept.toml"))
    }

    /// Get an active environment (helper method to reduce duplication)
    async fn get_active_environment(&self, environment_id: &str) -> Result<crate::config::Environment> {
        let env_manager = self.environment_manager.read().await;
        let environment = env_manager.get_environment(environment_id).ok_or_else(|| {
            Self::environment_not_found_error(environment_id)
        })?;

        if !environment.is_active() {
            return Err(Self::environment_not_active_error(environment_id));
        }

        Ok(environment.clone())
    }

    /// Generic config modification helper to reduce duplication
    async fn modify_config<F>(&mut self, environment_id: &str, operation: F) -> Result<()>
    where
        F: FnOnce(&mut crate::config::ProjectConfig) -> Result<()>,
    {
        // Validate environment exists and is active
        self.validate_environment(environment_id).await?;

        // Get the project config path
        let config_path = self.get_config_path(environment_id).await?;

        // Load current config
        let mut project_config = crate::config::ProjectConfig::load_from_path(&config_path).await?;

        // Apply the operation
        operation(&mut project_config)?;

        // Save the updated config
        project_config.save_to_path(&config_path).await?;

        // Reload the environment configuration
        self.reload_environment_config(environment_id).await?;

        Ok(())
    }

    /// Validate process operation (helper method to reduce duplication)
    fn validate_process_operation(
        config: &crate::config::ProjectConfig,
        process_name: &str,
        operation: ProcessOperation,
    ) -> Result<()> {
        match operation {
            ProcessOperation::Add => {
                if config.processes.contains_key(process_name) {
                    return Err(RunceptError::ProcessError(format!(
                        "Process '{process_name}' already exists"
                    )));
                }
            }
            ProcessOperation::Remove | ProcessOperation::Update => {
                if !config.processes.contains_key(process_name) {
                    return Err(RunceptError::ProcessError(format!(
                        "Process '{process_name}' not found"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Centralized error creation methods
    fn environment_not_found_error(environment_id: &str) -> RunceptError {
        RunceptError::EnvironmentError(format!("Environment '{environment_id}' not found"))
    }

    fn environment_not_active_error(environment_id: &str) -> RunceptError {
        RunceptError::EnvironmentError(format!("Environment '{environment_id}' is not active"))
    }

    fn process_not_found_error(process_name: &str) -> RunceptError {
        RunceptError::ProcessError(format!("Process '{process_name}' not found"))
    }
}

/// Operation types for process configuration management
#[derive(Debug, Clone)]
enum ProcessOperation {
    Add,
    Remove,
    Update,
}
