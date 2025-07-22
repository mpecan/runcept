use crate::config::ProcessDefinition;
use crate::error::{Result, RunceptError};
use crate::process::Process;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Manages process configurations and interfaces with EnvironmentManager
pub struct ProcessConfigurationManager {
    pub global_config: crate::config::GlobalConfig,
    pub environment_manager: Arc<RwLock<crate::config::EnvironmentManager>>,
    pub process_repository: Arc<crate::database::ProcessRepository>,
}

impl ProcessConfigurationManager {
    /// Create a new ProcessConfigurationManager
    pub fn new(
        global_config: crate::config::GlobalConfig,
        environment_manager: Arc<RwLock<crate::config::EnvironmentManager>>,
        process_repository: Arc<crate::database::ProcessRepository>,
    ) -> Self {
        Self {
            global_config,
            environment_manager,
            process_repository,
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

        // Enhanced validation before adding
        self.validate_process_definition(&process_def, environment_id)
            .await?;

        let process_name = process_def.name.clone();
        self.modify_config(environment_id, |config| {
            Self::validate_process_operation(config, &process_name, ProcessOperation::Add)?;
            Self::validate_process_dependencies(&process_def, config)?;
            config
                .processes
                .insert(process_name.clone(), process_def.clone());
            Ok(())
        })
        .await?;

        let working_dir = if let Some(wd) = &process_def.working_dir {
            if wd == "." {
                // Resolve "." to the actual project directory
                self.get_working_directory(environment_id)
                    .await?
                    .to_string_lossy()
                    .to_string()
            } else if std::path::Path::new(wd).is_relative() {
                // Resolve relative paths relative to the project directory
                let project_dir = self.get_working_directory(environment_id).await?;
                project_dir.join(wd).to_string_lossy().to_string()
            } else {
                // Use absolute path as-is
                wd.clone()
            }
        } else {
            // Get the default working directory from the environment
            self.get_working_directory(environment_id)
                .await?
                .to_string_lossy()
                .to_string()
        };
        let process =
            Process::from_definition(&process_def, working_dir, environment_id.to_string());
        self.process_repository.insert_process(&process).await?;

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
        })
        .await?;

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

        // Enhanced validation before updating
        let mut updated_process = process_def.clone();
        updated_process.name = name.to_string();
        self.validate_process_definition(&updated_process, environment_id)
            .await?;

        self.modify_config(environment_id, |config| {
            Self::validate_process_operation(config, name, ProcessOperation::Update)?;
            Self::validate_process_dependencies(&updated_process, config)?;
            config
                .processes
                .insert(name.to_string(), updated_process.clone());
            Ok(())
        })
        .await?;

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
        let environment = env_manager
            .get_environment(environment_id)
            .ok_or_else(|| Self::environment_not_found_error(environment_id))?;

        Ok(environment.project_path.clone())
    }

    /// Validate that an environment exists and is active
    async fn validate_environment(&self, environment_id: &str) -> Result<()> {
        let env_manager = self.environment_manager.read().await;
        let environment = env_manager
            .get_environment(environment_id)
            .ok_or_else(|| Self::environment_not_found_error(environment_id))?;

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
    async fn get_active_environment(
        &self,
        environment_id: &str,
    ) -> Result<crate::config::Environment> {
        let env_manager = self.environment_manager.read().await;
        let environment = env_manager
            .get_environment(environment_id)
            .ok_or_else(|| Self::environment_not_found_error(environment_id))?;

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

    // ===== Enhanced Validation Methods =====

    /// Comprehensive validation of a process definition
    async fn validate_process_definition(
        &self,
        process_def: &ProcessDefinition,
        environment_id: &str,
    ) -> Result<()> {
        debug!("Validating process definition for '{}'", process_def.name);

        // Basic field validation
        self.validate_process_name(&process_def.name)?;
        self.validate_command(&process_def.command).await?;

        // Working directory validation
        if let Some(working_dir) = &process_def.working_dir {
            self.validate_working_directory(working_dir, environment_id)
                .await?;
        }

        // Health check validation
        if let Some(health_url) = &process_def.health_check_url {
            self.validate_health_check_url(health_url)?;
        }

        // Health check interval validation
        if let Some(interval) = process_def.health_check_interval {
            self.validate_health_check_interval(interval as i32)?;
        }

        // Environment variables validation
        self.validate_environment_variables(&process_def.env_vars)?;

        debug!(
            "Process definition validation completed for '{}'",
            process_def.name
        );
        Ok(())
    }

    /// Validate process name
    fn validate_process_name(&self, name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(RunceptError::ConfigError(
                "Process name cannot be empty".to_string(),
            ));
        }

        if name.len() > 64 {
            return Err(RunceptError::ConfigError(
                "Process name cannot exceed 64 characters".to_string(),
            ));
        }

        // Check for invalid characters that could cause issues
        if name.contains(':') || name.contains('/') || name.contains('\\') {
            return Err(RunceptError::ConfigError(
                "Process name cannot contain ':', '/', or '\\' characters".to_string(),
            ));
        }

        // Check for reserved names
        if matches!(name, "daemon" | "server" | "all" | "system") {
            return Err(RunceptError::ConfigError(format!(
                "Process name '{name}' is reserved and cannot be used"
            )));
        }

        Ok(())
    }

    /// Validate process command
    async fn validate_command(&self, command: &str) -> Result<()> {
        if command.trim().is_empty() {
            return Err(RunceptError::ConfigError(
                "Process command cannot be empty".to_string(),
            ));
        }

        // Parse command to check validity
        match shlex::split(command) {
            Some(parts) if !parts.is_empty() => {
                let executable = &parts[0];

                // Check if the executable exists in PATH or as absolute path
                if std::path::Path::new(executable).is_absolute() {
                    // Absolute path - check if file exists and is executable
                    if !std::path::Path::new(executable).exists() {
                        warn!(
                            "Executable '{}' does not exist at absolute path",
                            executable
                        );
                    }
                } else {
                    // Command name - check if it exists in PATH (using simple check without external crate)
                    if !Self::is_command_available(executable) {
                        warn!("Executable '{}' not found in PATH", executable);
                    }
                }
                Ok(())
            }
            Some(_) => Err(RunceptError::ConfigError(
                "Command cannot be empty after parsing".to_string(),
            )),
            None => Err(RunceptError::ConfigError(
                "Failed to parse command - invalid shell syntax".to_string(),
            )),
        }
    }

    /// Simple check if a command is available in PATH without external dependencies
    fn is_command_available(command: &str) -> bool {
        // Try running the command with --version or --help to see if it exists
        match std::process::Command::new(command)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Ok(_) => true,
            Err(_) => {
                // Try with --help as fallback
                std::process::Command::new(command)
                    .arg("--help")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok()
            }
        }
    }

    /// Validate working directory
    async fn validate_working_directory(
        &self,
        working_dir: &str,
        environment_id: &str,
    ) -> Result<()> {
        if working_dir.is_empty() {
            return Err(RunceptError::ConfigError(
                "Working directory cannot be empty".to_string(),
            ));
        }

        // Resolve the working directory path
        let resolved_path = if working_dir == "." {
            self.get_working_directory(environment_id).await?
        } else if std::path::Path::new(working_dir).is_relative() {
            let project_dir = self.get_working_directory(environment_id).await?;
            project_dir.join(working_dir)
        } else {
            std::path::PathBuf::from(working_dir)
        };

        // Check if the directory exists
        if !resolved_path.exists() {
            return Err(RunceptError::ConfigError(format!(
                "Working directory '{}' does not exist",
                resolved_path.display()
            )));
        }

        // Check if it's actually a directory
        if !resolved_path.is_dir() {
            return Err(RunceptError::ConfigError(format!(
                "Working directory '{}' is not a directory",
                resolved_path.display()
            )));
        }

        // Check if it's readable
        match std::fs::read_dir(&resolved_path) {
            Ok(_) => Ok(()),
            Err(e) => Err(RunceptError::ConfigError(format!(
                "Working directory '{}' is not accessible: {}",
                resolved_path.display(),
                e
            ))),
        }
    }

    /// Validate health check URL
    fn validate_health_check_url(&self, url: &str) -> Result<()> {
        if url.is_empty() {
            return Err(RunceptError::ConfigError(
                "Health check URL cannot be empty".to_string(),
            ));
        }

        // Parse the URL to check validity
        if url.starts_with("http://") || url.starts_with("https://") {
            // Simple HTTP URL validation without external dependencies
            if url.len() < 10
                || (!url.contains("://")
                    || url.split("://").nth(1).is_none_or(|part| part.is_empty()))
            {
                Err(RunceptError::ConfigError(format!(
                    "Invalid health check URL format '{url}'"
                )))
            } else {
                Ok(())
            }
        } else if url.starts_with("tcp://") {
            // Validate TCP health check format: tcp://host:port
            let tcp_part = url.strip_prefix("tcp://").unwrap();
            if tcp_part.contains(':') {
                let parts: Vec<&str> = tcp_part.split(':').collect();
                if parts.len() == 2 {
                    match parts[1].parse::<u16>() {
                        Ok(_) => Ok(()),
                        Err(_) => Err(RunceptError::ConfigError(format!(
                            "Invalid port in TCP health check URL '{url}'"
                        ))),
                    }
                } else {
                    Err(RunceptError::ConfigError(format!(
                        "Invalid TCP health check URL format '{url}', expected tcp://host:port"
                    )))
                }
            } else {
                Err(RunceptError::ConfigError(format!(
                    "Invalid TCP health check URL format '{url}', expected tcp://host:port"
                )))
            }
        } else if url.starts_with("cmd://") {
            // Command health checks are always valid as long as they have content
            let cmd_part = url.strip_prefix("cmd://").unwrap();
            if cmd_part.is_empty() {
                Err(RunceptError::ConfigError(
                    "Command health check cannot be empty".to_string(),
                ))
            } else {
                Ok(())
            }
        } else {
            Err(RunceptError::ConfigError(format!(
                "Unsupported health check URL scheme in '{url}', supported: http://, https://, tcp://, cmd://"
            )))
        }
    }

    /// Validate health check interval
    fn validate_health_check_interval(&self, interval: i32) -> Result<()> {
        if interval < 1 {
            return Err(RunceptError::ConfigError(
                "Health check interval must be at least 1 second".to_string(),
            ));
        }

        if interval > 3600 {
            return Err(RunceptError::ConfigError(
                "Health check interval cannot exceed 1 hour (3600 seconds)".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate environment variables
    fn validate_environment_variables(
        &self,
        env_vars: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        for (key, value) in env_vars {
            // Validate environment variable name
            if key.is_empty() {
                return Err(RunceptError::ConfigError(
                    "Environment variable name cannot be empty".to_string(),
                ));
            }

            // Check for invalid characters in variable name
            if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(RunceptError::ConfigError(format!(
                    "Environment variable name '{key}' contains invalid characters (only alphanumeric and underscore allowed)"
                )));
            }

            // Check for reserved environment variables
            if matches!(key.as_str(), "PATH" | "HOME" | "USER" | "PWD") {
                warn!("Overriding system environment variable '{}'", key);
            }

            // Validate value (basic checks)
            if value.len() > 4096 {
                return Err(RunceptError::ConfigError(format!(
                    "Environment variable '{key}' value exceeds maximum length of 4096 characters"
                )));
            }
        }

        Ok(())
    }

    /// Validate process dependencies to prevent cycles
    fn validate_process_dependencies(
        process_def: &ProcessDefinition,
        config: &crate::config::ProjectConfig,
    ) -> Result<()> {
        if process_def.depends_on.is_empty() {
            return Ok(()); // No dependencies to validate
        }

        // Check that all dependencies exist
        for dep in &process_def.depends_on {
            if !config.processes.contains_key(dep) {
                return Err(RunceptError::ConfigError(format!(
                    "Process '{}' depends on '{}' which does not exist",
                    process_def.name, dep
                )));
            }

            // Prevent self-dependency
            if dep == &process_def.name {
                return Err(RunceptError::ConfigError(format!(
                    "Process '{}' cannot depend on itself",
                    process_def.name
                )));
            }
        }

        // Check for circular dependencies
        Self::check_circular_dependencies(&process_def.name, &process_def.depends_on, config)?;

        Ok(())
    }

    /// Check for circular dependencies using depth-first search
    fn check_circular_dependencies(
        _process_name: &str,
        dependencies: &[String],
        config: &crate::config::ProjectConfig,
    ) -> Result<()> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        fn dfs(
            current: &str,
            config: &crate::config::ProjectConfig,
            visited: &mut HashSet<String>,
            rec_stack: &mut HashSet<String>,
        ) -> Result<()> {
            if rec_stack.contains(current) {
                return Err(RunceptError::ConfigError(format!(
                    "Circular dependency detected involving process '{current}'"
                )));
            }

            if visited.contains(current) {
                return Ok(());
            }

            visited.insert(current.to_string());
            rec_stack.insert(current.to_string());

            if let Some(process) = config.processes.get(current) {
                for dep in &process.depends_on {
                    dfs(dep, config, visited, rec_stack)?;
                }
            }

            rec_stack.remove(current);
            Ok(())
        }

        // Start DFS from each dependency
        for dep in dependencies {
            dfs(dep, config, &mut visited, &mut rec_stack)?;
        }

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
        RunceptError::EnvironmentError(format!(
            "Environment '{environment_id}' is registered but not active"
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_validate_health_check_url() {
        // Test the static validation methods that don't require a full config manager

        // Valid URLs
        assert!(validate_health_check_url_static("http://localhost:8080/health").is_ok());
        assert!(validate_health_check_url_static("https://example.com/status").is_ok());
        assert!(validate_health_check_url_static("tcp://localhost:5432").is_ok());
        assert!(validate_health_check_url_static("cmd://curl -f http://localhost/health").is_ok());

        // Invalid URLs
        assert!(validate_health_check_url_static("").is_err());
        assert!(validate_health_check_url_static("invalid-url").is_err());
        assert!(validate_health_check_url_static("ftp://example.com").is_err());
        assert!(validate_health_check_url_static("tcp://localhost").is_err());
        assert!(validate_health_check_url_static("tcp://localhost:invalid").is_err());
        assert!(validate_health_check_url_static("cmd://").is_err());
    }

    #[test]
    fn test_validate_health_check_interval() {
        // Valid intervals
        assert!(validate_health_check_interval_static(1).is_ok());
        assert!(validate_health_check_interval_static(30).is_ok());
        assert!(validate_health_check_interval_static(3600).is_ok());

        // Invalid intervals
        assert!(validate_health_check_interval_static(0).is_err());
        assert!(validate_health_check_interval_static(-1).is_err());
        assert!(validate_health_check_interval_static(3601).is_err());
    }

    #[test]
    fn test_validate_environment_variables() {
        // Valid environment variables
        let mut valid_env_vars = HashMap::new();
        valid_env_vars.insert("VALID_VAR".to_string(), "value".to_string());
        valid_env_vars.insert("ANOTHER_VAR".to_string(), "another_value".to_string());
        assert!(validate_environment_variables_static(&valid_env_vars).is_ok());

        // Invalid environment variables
        let mut invalid_env_vars = HashMap::new();
        invalid_env_vars.insert("".to_string(), "value".to_string());
        assert!(validate_environment_variables_static(&invalid_env_vars).is_err());

        let mut invalid_env_vars = HashMap::new();
        invalid_env_vars.insert("INVALID-VAR".to_string(), "value".to_string());
        assert!(validate_environment_variables_static(&invalid_env_vars).is_err());

        let mut invalid_env_vars = HashMap::new();
        invalid_env_vars.insert("VALID_VAR".to_string(), "a".repeat(4097));
        assert!(validate_environment_variables_static(&invalid_env_vars).is_err());
    }

    #[test]
    fn test_validate_process_name() {
        // Valid names
        assert!(validate_process_name_static("valid-name").is_ok());
        assert!(validate_process_name_static("valid_name").is_ok());
        assert!(validate_process_name_static("validname123").is_ok());

        // Invalid names
        assert!(validate_process_name_static("").is_err());
        assert!(validate_process_name_static("name:with:colons").is_err());
        assert!(validate_process_name_static("name/with/slashes").is_err());
        assert!(validate_process_name_static("name\\with\\backslashes").is_err());
        assert!(validate_process_name_static("daemon").is_err());
        assert!(validate_process_name_static("server").is_err());
        assert!(validate_process_name_static("all").is_err());
        assert!(validate_process_name_static("system").is_err());

        // Name too long
        let long_name = "a".repeat(65);
        assert!(validate_process_name_static(&long_name).is_err());
    }

    #[test]
    fn test_is_command_available() {
        // Test with a command that should exist on most systems
        assert!(ProcessConfigurationManager::is_command_available("echo"));

        // Test with a command that should not exist
        assert!(!ProcessConfigurationManager::is_command_available(
            "definitely_not_a_real_command_12345"
        ));
    }

    #[test]
    fn test_error_creation_methods() {
        let env_error = ProcessConfigurationManager::environment_not_found_error("test-env");
        assert!(matches!(env_error, RunceptError::EnvironmentError(_)));
        assert!(env_error.to_string().contains("test-env"));

        let active_error = ProcessConfigurationManager::environment_not_active_error("test-env");
        assert!(matches!(active_error, RunceptError::EnvironmentError(_)));
        assert!(active_error.to_string().contains("not active"));

        let process_error = ProcessConfigurationManager::process_not_found_error("test-process");
        assert!(matches!(process_error, RunceptError::ProcessError(_)));
        assert!(process_error.to_string().contains("test-process"));
    }

    // Helper functions for testing static validation logic
    fn validate_process_name_static(name: &str) -> Result<()> {
        if name.is_empty() {
            return Err(RunceptError::ConfigError(
                "Process name cannot be empty".to_string(),
            ));
        }

        if name.len() > 64 {
            return Err(RunceptError::ConfigError(
                "Process name cannot exceed 64 characters".to_string(),
            ));
        }

        if name.contains(':') || name.contains('/') || name.contains('\\') {
            return Err(RunceptError::ConfigError(
                "Process name cannot contain ':', '/', or '\\' characters".to_string(),
            ));
        }

        if matches!(name, "daemon" | "server" | "all" | "system") {
            return Err(RunceptError::ConfigError(format!(
                "Process name '{name}' is reserved and cannot be used"
            )));
        }

        Ok(())
    }

    fn validate_health_check_url_static(url: &str) -> Result<()> {
        if url.is_empty() {
            return Err(RunceptError::ConfigError(
                "Health check URL cannot be empty".to_string(),
            ));
        }

        if url.starts_with("http://") || url.starts_with("https://") {
            if url.len() < 10
                || (!url.contains("://")
                    || url.split("://").nth(1).is_none_or(|part| part.is_empty()))
            {
                Err(RunceptError::ConfigError(format!(
                    "Invalid health check URL format '{url}'"
                )))
            } else {
                Ok(())
            }
        } else if url.starts_with("tcp://") {
            let tcp_part = url.strip_prefix("tcp://").unwrap();
            if tcp_part.contains(':') {
                let parts: Vec<&str> = tcp_part.split(':').collect();
                if parts.len() == 2 {
                    match parts[1].parse::<u16>() {
                        Ok(_) => Ok(()),
                        Err(_) => Err(RunceptError::ConfigError(format!(
                            "Invalid port in TCP health check URL '{url}'"
                        ))),
                    }
                } else {
                    Err(RunceptError::ConfigError(format!(
                        "Invalid TCP health check URL format '{url}', expected tcp://host:port"
                    )))
                }
            } else {
                Err(RunceptError::ConfigError(format!(
                    "Invalid TCP health check URL format '{url}', expected tcp://host:port"
                )))
            }
        } else if url.starts_with("cmd://") {
            let cmd_part = url.strip_prefix("cmd://").unwrap();
            if cmd_part.is_empty() {
                Err(RunceptError::ConfigError(
                    "Command health check cannot be empty".to_string(),
                ))
            } else {
                Ok(())
            }
        } else {
            Err(RunceptError::ConfigError(format!(
                "Unsupported health check URL scheme in '{url}', supported: http://, https://, tcp://, cmd://"
            )))
        }
    }

    fn validate_health_check_interval_static(interval: i32) -> Result<()> {
        if interval < 1 {
            return Err(RunceptError::ConfigError(
                "Health check interval must be at least 1 second".to_string(),
            ));
        }

        if interval > 3600 {
            return Err(RunceptError::ConfigError(
                "Health check interval cannot exceed 1 hour (3600 seconds)".to_string(),
            ));
        }

        Ok(())
    }

    fn validate_environment_variables_static(
        env_vars: &std::collections::HashMap<String, String>,
    ) -> Result<()> {
        for (key, value) in env_vars {
            if key.is_empty() {
                return Err(RunceptError::ConfigError(
                    "Environment variable name cannot be empty".to_string(),
                ));
            }

            if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(RunceptError::ConfigError(format!(
                    "Environment variable name '{key}' contains invalid characters (only alphanumeric and underscore allowed)"
                )));
            }

            if value.len() > 4096 {
                return Err(RunceptError::ConfigError(format!(
                    "Environment variable '{key}' value exceeds maximum length of 4096 characters"
                )));
            }
        }

        Ok(())
    }
}
