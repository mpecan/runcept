use crate::config::{ProcessDefinition, ProjectConfig};
use crate::error::{Result, RunceptError};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

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

        let env_manager = self.environment_manager.read().await;
        let environment = env_manager.get_environment(environment_id).ok_or_else(|| {
            RunceptError::EnvironmentError(format!("Environment '{}' not found", environment_id))
        })?;

        if !environment.is_active() {
            return Err(RunceptError::EnvironmentError(format!(
                "Environment '{}' is not active",
                environment_id
            )));
        }

        let process_def = environment
            .project_config
            .processes
            .get(name)
            .ok_or_else(|| {
                RunceptError::ProcessError(format!(
                    "Process '{}' not found in environment '{}'",
                    name, environment_id
                ))
            })?;

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

        // Validate environment exists and is active
        self.validate_environment(environment_id).await?;

        // Get the project config path
        let config_path = self.get_config_path(environment_id).await?;

        // Load current config
        let mut project_config = ProjectConfig::load_from_path(&config_path).await?;

        // Check if process already exists
        if project_config.processes.contains_key(&process_def.name) {
            return Err(RunceptError::ProcessError(format!(
                "Process '{}' already exists in environment '{}'",
                process_def.name, environment_id
            )));
        }

        // Add the new process
        project_config
            .processes
            .insert(process_def.name.clone(), process_def.clone());

        // Save the updated config
        project_config.save_to_path(&config_path).await?;

        // Reload the environment configuration
        self.reload_environment_config(environment_id).await?;

        info!(
            "Successfully added process '{}' to environment '{}'",
            process_def.name, environment_id
        );
        Ok(())
    }

    /// Remove a process from the specified environment's configuration
    pub async fn remove_process(&mut self, name: &str, environment_id: &str) -> Result<()> {
        info!(
            "Removing process '{}' from environment '{}'",
            name, environment_id
        );

        // Validate environment exists and is active
        self.validate_environment(environment_id).await?;

        // Get the project config path
        let config_path = self.get_config_path(environment_id).await?;

        // Load current config
        let mut project_config = ProjectConfig::load_from_path(&config_path).await?;

        // Check if process exists
        if !project_config.processes.contains_key(name) {
            return Err(RunceptError::ProcessError(format!(
                "Process '{}' not found in environment '{}'",
                name, environment_id
            )));
        }

        // Remove the process
        project_config.processes.remove(name);

        // Save the updated config
        project_config.save_to_path(&config_path).await?;

        // Reload the environment configuration
        self.reload_environment_config(environment_id).await?;

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

        // Validate environment exists and is active
        self.validate_environment(environment_id).await?;

        // Get the project config path
        let config_path = self.get_config_path(environment_id).await?;

        // Load current config
        let mut project_config = ProjectConfig::load_from_path(&config_path).await?;

        // Check if process exists
        if !project_config.processes.contains_key(name) {
            return Err(RunceptError::ProcessError(format!(
                "Process '{}' not found in environment '{}'",
                name, environment_id
            )));
        }

        // Update the process (ensure the name matches)
        let mut updated_process = process_def;
        updated_process.name = name.to_string();
        project_config
            .processes
            .insert(name.to_string(), updated_process);

        // Save the updated config
        project_config.save_to_path(&config_path).await?;

        // Reload the environment configuration
        self.reload_environment_config(environment_id).await?;

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

        let env_manager = self.environment_manager.read().await;
        let environment = env_manager.get_environment(environment_id).ok_or_else(|| {
            RunceptError::EnvironmentError(format!("Environment '{}' not found", environment_id))
        })?;

        if !environment.is_active() {
            return Err(RunceptError::EnvironmentError(format!(
                "Environment '{}' is not active",
                environment_id
            )));
        }

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

        let env_manager = self.environment_manager.read().await;
        let environment = env_manager.get_environment(environment_id).ok_or_else(|| {
            RunceptError::EnvironmentError(format!("Environment '{}' not found", environment_id))
        })?;

        Ok(environment.project_path.clone())
    }

    /// Validate that an environment exists and is active
    async fn validate_environment(&self, environment_id: &str) -> Result<()> {
        let env_manager = self.environment_manager.read().await;
        let environment = env_manager.get_environment(environment_id).ok_or_else(|| {
            RunceptError::EnvironmentError(format!("Environment '{}' not found", environment_id))
        })?;

        if !environment.is_active() {
            return Err(RunceptError::EnvironmentError(format!(
                "Environment '{}' is not active",
                environment_id
            )));
        }

        Ok(())
    }

    /// Get the config file path for the specified environment
    async fn get_config_path(&self, environment_id: &str) -> Result<PathBuf> {
        let working_dir = self.get_working_directory(environment_id).await?;
        Ok(working_dir.join(".runcept.toml"))
    }
}
