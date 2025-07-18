#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalConfig, ProjectConfig};
    use std::collections::HashMap;
    use tempfile::TempDir;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_environment_creation() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("test-project");
        tokio::fs::create_dir_all(&project_path).await.unwrap();

        let mut project_config = ProjectConfig::default();
        project_config.environment.name = "test-env".to_string();

        let global_config = GlobalConfig::default();

        let env = Environment::new(
            "test-env".to_string(),
            project_path.clone(),
            project_config,
            global_config,
        );

        assert_eq!(env.name, "test-env");
        assert_eq!(env.project_path, project_path);
        assert_eq!(env.status, EnvironmentStatus::Inactive);
        assert!(env.processes.is_empty());
        assert!(!env.id.is_empty());
    }

    #[test]
    fn test_environment_status_display() {
        assert_eq!(EnvironmentStatus::Inactive.to_string(), "inactive");
        assert_eq!(EnvironmentStatus::Activating.to_string(), "activating");
        assert_eq!(EnvironmentStatus::Active.to_string(), "active");
        assert_eq!(EnvironmentStatus::Deactivating.to_string(), "deactivating");
        assert_eq!(EnvironmentStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_environment_status_transitions() {
        let mut env = create_test_environment();

        // Test activating
        assert!(env.can_transition_to(&EnvironmentStatus::Activating));
        env.transition_to(EnvironmentStatus::Activating);
        assert_eq!(env.status, EnvironmentStatus::Activating);

        // Test active
        assert!(env.can_transition_to(&EnvironmentStatus::Active));
        env.transition_to(EnvironmentStatus::Active);
        assert_eq!(env.status, EnvironmentStatus::Active);

        // Test deactivating
        assert!(env.can_transition_to(&EnvironmentStatus::Deactivating));
        env.transition_to(EnvironmentStatus::Deactivating);
        assert_eq!(env.status, EnvironmentStatus::Deactivating);

        // Test inactive
        assert!(env.can_transition_to(&EnvironmentStatus::Inactive));
        env.transition_to(EnvironmentStatus::Inactive);
        assert_eq!(env.status, EnvironmentStatus::Inactive);
    }

    #[test]
    fn test_invalid_environment_transitions() {
        let env = create_test_environment();

        // Can't go from Inactive to Active directly
        assert!(!env.can_transition_to(&EnvironmentStatus::Active));

        // Can't go from Inactive to Deactivating
        assert!(!env.can_transition_to(&EnvironmentStatus::Deactivating));
    }

    #[test]
    fn test_environment_is_active() {
        let mut env = create_test_environment();

        assert!(!env.is_active());

        env.transition_to(EnvironmentStatus::Activating);
        assert!(!env.is_active());

        env.transition_to(EnvironmentStatus::Active);
        assert!(env.is_active());
    }

    #[test]
    fn test_environment_add_process() {
        let mut env = create_test_environment();
        let process_id = Uuid::new_v4().to_string();

        env.add_process(process_id.clone());

        assert_eq!(env.processes.len(), 1);
        assert!(env.processes.contains(&process_id));
        assert!(env.has_process(&process_id));
    }

    #[test]
    fn test_environment_remove_process() {
        let mut env = create_test_environment();
        let process_id = Uuid::new_v4().to_string();

        env.add_process(process_id.clone());
        assert!(env.has_process(&process_id));

        env.remove_process(&process_id);
        assert!(!env.has_process(&process_id));
        assert!(env.processes.is_empty());
    }

    #[test]
    fn test_environment_serialization() {
        let env = create_test_environment();

        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains("test-env"));

        let deserialized: Environment = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, env.name);
        assert_eq!(deserialized.status, env.status);
        assert_eq!(deserialized.project_path, env.project_path);
    }

    #[tokio::test]
    async fn test_environment_manager_creation() {
        let global_config = GlobalConfig::default();
        let manager = EnvironmentManager::new(global_config).await.unwrap();

        assert!(manager.environments.is_empty());
        assert!(manager.database_pool.is_none());
    }

    #[tokio::test]
    async fn test_environment_manager_register_project() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("test-project");
        tokio::fs::create_dir_all(&project_path).await.unwrap();

        // Create a project config
        let mut project_config = ProjectConfig::default();
        project_config.environment.name = "test-project-env".to_string();

        let config_path = project_path.join(".runcept.toml");
        project_config.save_to_path(&config_path).await.unwrap();

        let global_config = GlobalConfig::default();
        let mut manager = EnvironmentManager::new(global_config).await.unwrap();

        let env_id = manager.register_project(&project_path).await.unwrap();

        assert_eq!(manager.environments.len(), 1);
        assert!(manager.environments.contains_key(&env_id));

        let env = manager.environments.get(&env_id).unwrap();
        assert_eq!(env.name, "test-project-env");
        // Compare canonical paths since register_project now canonicalizes paths
        assert_eq!(env.project_path, project_path.canonicalize().unwrap());
    }

    #[tokio::test]
    async fn test_environment_manager_activate_environment() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("test-project");
        tokio::fs::create_dir_all(&project_path).await.unwrap();

        let mut project_config = ProjectConfig::default();
        project_config.environment.name = "test-env".to_string();

        // Add a simple process
        let mut processes = HashMap::new();
        processes.insert(
            "test-process".to_string(),
            crate::config::ProcessDefinition {
                name: "test-process".to_string(),
                command: "echo hello".to_string(),
                working_dir: None,
                auto_restart: Some(false),
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec![],
                env_vars: HashMap::new(),
            },
        );
        project_config.processes = processes;

        let config_path = project_path.join(".runcept.toml");
        project_config.save_to_path(&config_path).await.unwrap();

        let global_config = GlobalConfig::default();
        let mut manager = EnvironmentManager::new(global_config).await.unwrap();

        let env_id = manager.register_project(&project_path).await.unwrap();

        // Test activation
        let result = manager.activate_environment(&env_id).await;
        assert!(result.is_ok());

        let env = manager.environments.get(&env_id).unwrap();
        assert_eq!(env.status, EnvironmentStatus::Active);
    }

    #[tokio::test]
    async fn test_environment_manager_deactivate_environment() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("test-project");
        tokio::fs::create_dir_all(&project_path).await.unwrap();

        let mut project_config = ProjectConfig::default();
        project_config.environment.name = "test-env".to_string();

        let config_path = project_path.join(".runcept.toml");
        project_config.save_to_path(&config_path).await.unwrap();

        let global_config = GlobalConfig::default();
        let mut manager = EnvironmentManager::new(global_config).await.unwrap();

        let env_id = manager.register_project(&project_path).await.unwrap();

        // Activate first
        manager.activate_environment(&env_id).await.unwrap();

        // Test deactivation
        let result = manager.deactivate_environment(&env_id).await;
        assert!(result.is_ok());

        let env = manager.environments.get(&env_id).unwrap();
        assert_eq!(env.status, EnvironmentStatus::Inactive);
    }

    #[tokio::test]
    async fn test_environment_manager_discover_projects() {
        let temp_dir = TempDir::new().unwrap();

        // Create multiple projects
        let project1_path = temp_dir.path().join("project1");
        let project2_path = temp_dir.path().join("nested").join("project2");
        tokio::fs::create_dir_all(&project1_path).await.unwrap();
        tokio::fs::create_dir_all(&project2_path).await.unwrap();

        // Create configs
        let mut config1 = ProjectConfig::default();
        config1.environment.name = "project1-env".to_string();
        config1
            .save_to_path(&project1_path.join(".runcept.toml"))
            .await
            .unwrap();

        let mut config2 = ProjectConfig::default();
        config2.environment.name = "project2-env".to_string();
        config2
            .save_to_path(&project2_path.join(".runcept.toml"))
            .await
            .unwrap();

        let global_config = GlobalConfig::default();
        let mut manager = EnvironmentManager::new(global_config).await.unwrap();

        let discovered = manager.discover_projects(temp_dir.path()).await.unwrap();

        assert_eq!(discovered.len(), 2);
        assert_eq!(manager.environments.len(), 2);
    }

    #[tokio::test]
    async fn test_environment_manager_get_active_environments() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("test-project");
        tokio::fs::create_dir_all(&project_path).await.unwrap();

        let mut project_config = ProjectConfig::default();
        project_config.environment.name = "test-env".to_string();

        let config_path = project_path.join(".runcept.toml");
        project_config.save_to_path(&config_path).await.unwrap();

        let global_config = GlobalConfig::default();
        let mut manager = EnvironmentManager::new(global_config).await.unwrap();

        let env_id = manager.register_project(&project_path).await.unwrap();

        // No active environments initially
        let active = manager.get_active_environments();
        assert!(active.is_empty());

        // Activate environment
        manager.activate_environment(&env_id).await.unwrap();

        let active = manager.get_active_environments();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, &env_id);
    }

    fn create_test_environment() -> Environment {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().join("test");

        let project_config = ProjectConfig::default();
        let global_config = GlobalConfig::default();

        Environment::new(
            "test-env".to_string(),
            project_path,
            project_config,
            global_config,
        )
    }
}

use crate::config::{GlobalConfig, ProjectConfig, find_project_config};
use crate::database::QueryManager;
use crate::error::{Result, RunceptError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EnvironmentStatus {
    Inactive,
    Activating,
    Active,
    Deactivating,
    Failed,
}

impl fmt::Display for EnvironmentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status_str = match self {
            EnvironmentStatus::Inactive => "inactive",
            EnvironmentStatus::Activating => "activating",
            EnvironmentStatus::Active => "active",
            EnvironmentStatus::Deactivating => "deactivating",
            EnvironmentStatus::Failed => "failed",
        };
        write!(f, "{status_str}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub project_path: PathBuf,
    pub status: EnvironmentStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity: Option<DateTime<Utc>>,
    pub processes: Vec<String>, // Process IDs
    pub project_config: ProjectConfig,
    pub merged_config: ProjectConfig, // After merging with global config
}

impl Environment {
    pub fn new(
        name: String,
        project_path: PathBuf,
        project_config: ProjectConfig,
        global_config: GlobalConfig,
    ) -> Self {
        let now = Utc::now();
        let merged_config = project_config.merge_with_global(&global_config);

        // Use the absolute path as the environment ID for uniqueness across different paths
        let id = project_path.to_string_lossy().to_string();

        Self {
            id,
            name,
            project_path,
            status: EnvironmentStatus::Inactive,
            created_at: now,
            updated_at: now,
            last_activity: None,
            processes: Vec::new(),
            project_config,
            merged_config,
        }
    }

    pub fn transition_to(&mut self, new_status: EnvironmentStatus) {
        if self.can_transition_to(&new_status) {
            self.status = new_status;
            self.updated_at = Utc::now();
        }
    }

    pub fn can_transition_to(&self, new_status: &EnvironmentStatus) -> bool {
        use EnvironmentStatus::*;

        match (&self.status, new_status) {
            // From Inactive
            (Inactive, Activating) => true,
            (Inactive, Failed) => true,

            // From Activating
            (Activating, Active) => true,
            (Activating, Failed) => true,
            (Activating, Inactive) => true, // Cancel activation

            // From Active
            (Active, Deactivating) => true,
            (Active, Failed) => true,

            // From Deactivating
            (Deactivating, Inactive) => true,
            (Deactivating, Failed) => true,

            // From Failed
            (Failed, Activating) => true,
            (Failed, Inactive) => true,

            // Same status
            (status, new_status) if status == new_status => true,

            // All other transitions are invalid
            _ => false,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, EnvironmentStatus::Active)
    }

    pub fn is_transitioning(&self) -> bool {
        matches!(
            self.status,
            EnvironmentStatus::Activating | EnvironmentStatus::Deactivating
        )
    }

    pub fn add_process(&mut self, process_id: String) {
        if !self.processes.contains(&process_id) {
            self.processes.push(process_id);
            self.updated_at = Utc::now();
        }
    }

    pub fn remove_process(&mut self, process_id: &str) {
        self.processes.retain(|id| id != process_id);
        self.updated_at = Utc::now();
    }

    pub fn has_process(&self, process_id: &str) -> bool {
        self.processes.contains(&process_id.to_string())
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    pub fn get_process_startup_order(&self) -> Result<Vec<String>> {
        self.merged_config.get_startup_order()
    }

    pub fn get_process_definition(&self, name: &str) -> Option<&crate::config::ProcessDefinition> {
        self.merged_config.get_process_definition(name)
    }

    pub fn get_inactivity_timeout(&self) -> Option<&String> {
        self.merged_config.environment.inactivity_timeout.as_ref()
    }

    pub fn should_auto_shutdown(&self) -> bool {
        self.merged_config.environment.auto_shutdown.unwrap_or(true)
    }
}

pub struct EnvironmentManager {
    pub environments: HashMap<String, Environment>,
    pub global_config: GlobalConfig,
    pub database_pool: Option<Pool<Sqlite>>,
}

impl EnvironmentManager {
    pub async fn new(global_config: GlobalConfig) -> Result<Self> {
        Self::new_with_database(global_config, None).await
    }

    pub async fn new_with_database(
        global_config: GlobalConfig,
        database_pool: Option<Pool<Sqlite>>,
    ) -> Result<Self> {
        let mut manager = Self {
            environments: HashMap::new(),
            global_config,
            database_pool,
        };

        // Load environments from database if available
        if manager.database_pool.is_some() {
            manager.load_environments_from_database().await?;
        }

        Ok(manager)
    }

    pub async fn register_project(&mut self, project_path: &Path) -> Result<String> {
        // First check if we already have this environment in database
        let absolute_path = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());
        let path_str = absolute_path.to_string_lossy().to_string();

        if let Some(pool) = &self.database_pool {
            let query_manager = QueryManager::new(pool);
            if let Some(existing_env) = query_manager.get_environment_by_path(&path_str).await? {
                // Environment already exists in database, load it into memory
                let env_id = existing_env.id.clone();
                self.environments.insert(env_id.clone(), existing_env);
                return Ok(env_id);
            }
        }

        // Find project config
        let config_path = find_project_config(project_path).await?.ok_or_else(|| {
            RunceptError::ConfigError(format!(
                "No .runcept.toml found in or above {}",
                project_path.display()
            ))
        })?;

        // Load project config
        let project_config = ProjectConfig::load_from_path(&config_path).await?;

        // Create environment
        let environment = Environment::new(
            project_config.environment.name.clone(),
            absolute_path,
            project_config,
            self.global_config.clone(),
        );

        let env_id = environment.id.clone();

        // Save to database first
        self.save_environment_to_database(&environment).await?;

        // Then store in memory
        self.environments.insert(env_id.clone(), environment);

        Ok(env_id)
    }

    pub async fn activate_environment(&mut self, env_id: &str) -> Result<()> {
        // Update environment state in multiple steps to avoid borrowing conflicts
        {
            let environment = self.environments.get_mut(env_id).ok_or_else(|| {
                RunceptError::EnvironmentError(format!("Environment {env_id} not found"))
            })?;

            // Transition to activating
            environment.transition_to(EnvironmentStatus::Activating);
        }

        // Save intermediate state
        if let Some(environment) = self.environments.get(env_id) {
            self.save_environment_to_database(environment).await?;
        }

        // Continue with activation
        {
            let environment = self.environments.get_mut(env_id).ok_or_else(|| {
                RunceptError::EnvironmentError(format!("Environment {env_id} not found"))
            })?;

            // TODO: Start processes in dependency order
            // For now, just mark as active
            environment.transition_to(EnvironmentStatus::Active);
            environment.update_activity();
        }

        // Persist final changes to database
        if let Some(environment) = self.environments.get(env_id) {
            self.save_environment_to_database(environment).await?;
        }

        Ok(())
    }

    pub async fn deactivate_environment(&mut self, env_id: &str) -> Result<()> {
        // Update environment state in multiple steps to avoid borrowing conflicts
        {
            let environment = self.environments.get_mut(env_id).ok_or_else(|| {
                RunceptError::EnvironmentError(format!("Environment {env_id} not found"))
            })?;

            // Transition to deactivating
            environment.transition_to(EnvironmentStatus::Deactivating);
        }

        // Save intermediate state
        if let Some(environment) = self.environments.get(env_id) {
            self.save_environment_to_database(environment).await?;
        }

        // Continue with deactivation
        {
            let environment = self.environments.get_mut(env_id).ok_or_else(|| {
                RunceptError::EnvironmentError(format!("Environment {env_id} not found"))
            })?;

            // TODO: Stop all processes in reverse dependency order
            // For now, just mark as inactive
            environment.transition_to(EnvironmentStatus::Inactive);
        }

        // Persist final changes to database
        if let Some(environment) = self.environments.get(env_id) {
            self.save_environment_to_database(environment).await?;
        }

        Ok(())
    }

    pub async fn discover_projects(&mut self, root_path: &Path) -> Result<Vec<String>> {
        let mut discovered = Vec::new();

        // Use a simple directory traversal to find .runcept.toml files
        let mut stack = vec![root_path.to_path_buf()];

        while let Some(current_path) = stack.pop() {
            if let Ok(entries) = tokio::fs::read_dir(&current_path).await {
                let mut entries = entries;

                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();

                    if path.is_file()
                        && path.file_name() == Some(std::ffi::OsStr::new(".runcept.toml"))
                    {
                        // Found a project config, register it
                        if let Some(project_dir) = path.parent() {
                            match self.register_project(project_dir).await {
                                Ok(env_id) => discovered.push(env_id),
                                Err(e) => {
                                    // Log error but continue discovery
                                    eprintln!(
                                        "Failed to register project at {}: {}",
                                        project_dir.display(),
                                        e
                                    );
                                }
                            }
                        }
                    } else if path.is_dir() {
                        // Add directory to stack for traversal
                        stack.push(path);
                    }
                }
            }
        }

        Ok(discovered)
    }

    pub fn get_environment(&self, env_id: &str) -> Option<&Environment> {
        self.environments.get(env_id)
    }

    pub fn get_environment_mut(&mut self, env_id: &str) -> Option<&mut Environment> {
        self.environments.get_mut(env_id)
    }

    pub fn get_active_environments(&self) -> Vec<(&String, &Environment)> {
        self.environments
            .iter()
            .filter(|(_, env)| env.is_active())
            .collect()
    }

    pub fn get_all_environments(&self) -> Vec<(&String, &Environment)> {
        self.environments.iter().collect()
    }

    /// Update environment activity and persist to database
    pub async fn update_environment_activity(&mut self, env_id: &str) -> Result<()> {
        // Update activity first
        if let Some(environment) = self.environments.get_mut(env_id) {
            environment.update_activity();
        }

        // Then save to database
        if let Some(environment) = self.environments.get(env_id) {
            self.save_environment_to_database(environment).await?;
        }

        Ok(())
    }

    pub async fn cleanup_inactive_environments(
        &mut self,
        max_idle_hours: u32,
    ) -> Result<Vec<String>> {
        let cutoff_time = Utc::now() - chrono::Duration::hours(max_idle_hours as i64);
        let mut cleaned_up = Vec::new();

        let inactive_env_ids: Vec<String> = self
            .environments
            .iter()
            .filter(|(_, env)| {
                !env.is_active()
                    && env
                        .last_activity
                        .is_none_or(|activity| activity < cutoff_time)
            })
            .map(|(id, _)| id.clone())
            .collect();

        for env_id in inactive_env_ids {
            // Remove from database first
            self.remove_environment_from_database(&env_id).await?;

            // Then remove from memory
            self.environments.remove(&env_id);
            cleaned_up.push(env_id);
        }

        Ok(cleaned_up)
    }

    pub fn update_global_config(&mut self, global_config: GlobalConfig) {
        self.global_config = global_config.clone();

        // Update merged configs for all environments
        for environment in self.environments.values_mut() {
            environment.merged_config =
                environment.project_config.merge_with_global(&global_config);
        }
    }

    /// Reload the project configuration for a specific environment from disk
    pub async fn reload_environment_config(&mut self, env_id: &str) -> Result<()> {
        use crate::config::project::find_project_config;

        if let Some(environment) = self.environments.get_mut(env_id) {
            // Find the project config file
            let config_path = find_project_config(&environment.project_path)
                .await?
                .ok_or_else(|| {
                    RunceptError::ConfigError(format!(
                        "No .runcept.toml found in or above {}",
                        environment.project_path.display()
                    ))
                })?;

            // Reload the project config from disk
            let project_config = ProjectConfig::load_from_path(&config_path).await?;

            // Update the environment with the new config
            environment.project_config = project_config.clone();
            environment.merged_config = project_config.merge_with_global(&self.global_config);
        }

        // Save the updated environment to database after releasing the mutable borrow
        if let Some(environment) = self.environments.get(env_id) {
            self.save_environment_to_database(environment).await?;
        }

        Ok(())
    }

    /// Load environments from database
    async fn load_environments_from_database(&mut self) -> Result<()> {
        if let Some(pool) = &self.database_pool {
            let query_manager = QueryManager::new(pool);
            let environments = query_manager.list_environments().await?;

            for env in environments {
                self.environments.insert(env.id.clone(), env);
            }
        }
        Ok(())
    }

    /// Save environment to database
    async fn save_environment_to_database(&self, environment: &Environment) -> Result<()> {
        if let Some(pool) = &self.database_pool {
            let query_manager = QueryManager::new(pool);

            // Check if environment already exists
            if query_manager
                .get_environment_by_id(&environment.id)
                .await?
                .is_some()
            {
                query_manager.update_environment(environment).await?;
            } else {
                query_manager.insert_environment(environment).await?;
            }
        }
        Ok(())
    }

    /// Remove environment from database
    async fn remove_environment_from_database(&self, env_id: &str) -> Result<()> {
        if let Some(pool) = &self.database_pool {
            let query_manager = QueryManager::new(pool);
            query_manager.delete_environment(env_id).await?;
        }
        Ok(())
    }

    /// Search environments in database
    pub async fn search_environments(&self, query: &str) -> Result<Vec<Environment>> {
        if let Some(pool) = &self.database_pool {
            let query_manager = QueryManager::new(pool);
            query_manager.search_environments(query).await
        } else {
            // Fallback to in-memory search
            let results: Vec<Environment> = self
                .environments
                .values()
                .filter(|env| {
                    env.name.to_lowercase().contains(&query.to_lowercase())
                        || env
                            .project_path
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query.to_lowercase())
                })
                .cloned()
                .collect();
            Ok(results)
        }
    }

    /// Get environments by status from database
    pub async fn get_environments_by_status(
        &self,
        status: EnvironmentStatus,
    ) -> Result<Vec<Environment>> {
        if let Some(pool) = &self.database_pool {
            let query_manager = QueryManager::new(pool);
            query_manager.get_environments_by_status(status).await
        } else {
            // Fallback to in-memory filtering
            let results: Vec<Environment> = self
                .environments
                .values()
                .filter(|env| env.status == status)
                .cloned()
                .collect();
            Ok(results)
        }
    }
}
