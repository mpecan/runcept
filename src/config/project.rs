#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::global::GlobalConfig;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_process_definition_creation() {
        let mut env_vars = HashMap::new();
        env_vars.insert("NODE_ENV".to_string(), "development".to_string());

        let process = ProcessDefinition {
            name: "web-server".to_string(),
            command: "npm start".to_string(),
            working_dir: Some("./web".to_string()),
            auto_restart: Some(true),
            health_check_url: Some("http://localhost:3000/health".to_string()),
            health_check_interval: Some(30),
            depends_on: vec!["database".to_string(), "redis".to_string()],
            env_vars,
        };

        assert_eq!(process.name, "web-server");
        assert_eq!(process.command, "npm start");
        assert_eq!(process.working_dir, Some("./web".to_string()));
        assert_eq!(process.auto_restart, Some(true));
        assert_eq!(process.depends_on.len(), 2);
        assert_eq!(process.env_vars.len(), 1);
    }

    #[test]
    fn test_project_config_defaults() {
        let config = ProjectConfig::default();

        assert!(config.environment.name.is_empty());
        assert!(config.environment.inactivity_timeout.is_none());
        assert!(config.environment.auto_shutdown.is_none());
        assert!(config.processes.is_empty());
        assert!(config.environment.env_vars.is_empty());
    }

    #[test]
    fn test_project_config_serialization() {
        let mut processes = HashMap::new();
        processes.insert(
            "app".to_string(),
            ProcessDefinition {
                name: "app".to_string(),
                command: "cargo run".to_string(),
                working_dir: None,
                auto_restart: Some(false),
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec![],
                env_vars: HashMap::new(),
            },
        );

        let mut env_vars = HashMap::new();
        env_vars.insert("RUST_LOG".to_string(), "debug".to_string());

        let config = ProjectConfig {
            environment: EnvironmentConfig {
                name: "dev-env".to_string(),
                inactivity_timeout: Some("1h".to_string()),
                auto_shutdown: Some(true),
                env_vars,
            },
            processes,
        };

        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("[environment]"));
        assert!(toml_str.contains("name = \"dev-env\""));
        assert!(toml_str.contains("[processes.app]"));

        let deserialized: ProjectConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.environment.name, "dev-env");
        assert_eq!(deserialized.processes.len(), 1);
    }

    #[tokio::test]
    async fn test_project_config_load_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".runcept.toml");

        let result = ProjectConfig::load_from_path(&config_path).await;
        assert!(result.is_err());

        match result.err().unwrap() {
            crate::error::RunceptError::ConfigError(msg) => {
                assert!(msg.contains("not found"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }

    #[tokio::test]
    async fn test_project_config_load_and_save() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".runcept.toml");

        let mut processes = HashMap::new();
        // Don't add invalid dependencies for this test
        processes.insert(
            "test-app".to_string(),
            ProcessDefinition {
                name: "test-app".to_string(),
                command: "echo hello".to_string(),
                working_dir: Some("./src".to_string()),
                auto_restart: Some(true),
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec![], // No dependencies
                env_vars: HashMap::new(),
            },
        );

        let config = ProjectConfig {
            environment: EnvironmentConfig {
                name: "test-env".to_string(),
                inactivity_timeout: Some("2h".to_string()),
                auto_shutdown: Some(false),
                env_vars: HashMap::new(),
            },
            processes,
        };

        config.save_to_path(&config_path).await.unwrap();

        let loaded = ProjectConfig::load_from_path(&config_path).await.unwrap();
        assert_eq!(loaded.environment.name, "test-env");
        assert_eq!(
            loaded.environment.inactivity_timeout,
            Some("2h".to_string())
        );
        assert_eq!(loaded.processes.len(), 1);
        assert!(loaded.processes.contains_key("test-app"));
    }

    #[test]
    fn test_project_config_validation() {
        let mut config = ProjectConfig::default();
        config.environment.name = "valid-env".to_string();

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Empty environment name should fail
        config.environment.name = String::new();
        assert!(config.validate().is_err());

        // Invalid timeout format should fail
        config.environment.name = "valid-env".to_string();
        config.environment.inactivity_timeout = Some("invalid".to_string());
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_dependency_validation() {
        let mut config = ProjectConfig::default();
        config.environment.name = "test-env".to_string();

        let mut processes = HashMap::new();

        // Process with dependency that doesn't exist
        processes.insert(
            "app".to_string(),
            ProcessDefinition {
                name: "app".to_string(),
                command: "echo test".to_string(),
                working_dir: None,
                auto_restart: None,
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec!["nonexistent".to_string()],
                env_vars: HashMap::new(),
            },
        );

        config.processes = processes;

        let result = config.validate();
        assert!(result.is_err());

        match result.err().unwrap() {
            crate::error::RunceptError::ConfigError(msg) => {
                assert!(msg.contains("dependency"));
                assert!(msg.contains("nonexistent"));
            }
            _ => panic!("Expected ConfigError about dependency"),
        }
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut config = ProjectConfig::default();
        config.environment.name = "test-env".to_string();

        let mut processes = HashMap::new();

        // Create circular dependency: a -> b -> c -> a
        processes.insert(
            "a".to_string(),
            ProcessDefinition {
                name: "a".to_string(),
                command: "echo a".to_string(),
                working_dir: None,
                auto_restart: None,
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec!["b".to_string()],
                env_vars: HashMap::new(),
            },
        );

        processes.insert(
            "b".to_string(),
            ProcessDefinition {
                name: "b".to_string(),
                command: "echo b".to_string(),
                working_dir: None,
                auto_restart: None,
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec!["c".to_string()],
                env_vars: HashMap::new(),
            },
        );

        processes.insert(
            "c".to_string(),
            ProcessDefinition {
                name: "c".to_string(),
                command: "echo c".to_string(),
                working_dir: None,
                auto_restart: None,
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec!["a".to_string()],
                env_vars: HashMap::new(),
            },
        );

        config.processes = processes;

        let result = config.validate();
        assert!(result.is_err());

        match result.err().unwrap() {
            crate::error::RunceptError::ConfigError(msg) => {
                assert!(msg.contains("Circular") || msg.contains("circular"));
            }
            _ => panic!("Expected ConfigError about circular dependency"),
        }
    }

    #[test]
    fn test_merge_with_global_config() {
        let global_config = GlobalConfig::default();

        let mut env_vars = HashMap::new();
        env_vars.insert("PROJECT_VAR".to_string(), "project_value".to_string());

        let project_config = ProjectConfig {
            environment: EnvironmentConfig {
                name: "project-env".to_string(),
                inactivity_timeout: None,   // Should use global default
                auto_shutdown: Some(false), // Override global
                env_vars,
            },
            processes: HashMap::new(),
        };

        let merged = project_config.merge_with_global(&global_config);

        // Should use global timeout when not specified in project
        assert_eq!(
            merged.environment.inactivity_timeout,
            Some("30m".to_string())
        );
        // Should use project override
        assert_eq!(merged.environment.auto_shutdown, Some(false));
        // Should have project environment variables
        assert!(merged.environment.env_vars.contains_key("PROJECT_VAR"));
    }

    #[test]
    fn test_get_startup_order() {
        let mut config = ProjectConfig::default();
        config.environment.name = "test-env".to_string();

        let mut processes = HashMap::new();

        // Create dependency chain: database -> cache -> web
        processes.insert(
            "web".to_string(),
            ProcessDefinition {
                name: "web".to_string(),
                command: "npm start".to_string(),
                working_dir: None,
                auto_restart: None,
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec!["database".to_string(), "cache".to_string()],
                env_vars: HashMap::new(),
            },
        );

        processes.insert(
            "cache".to_string(),
            ProcessDefinition {
                name: "cache".to_string(),
                command: "redis-server".to_string(),
                working_dir: None,
                auto_restart: None,
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec!["database".to_string()],
                env_vars: HashMap::new(),
            },
        );

        processes.insert(
            "database".to_string(),
            ProcessDefinition {
                name: "database".to_string(),
                command: "postgres".to_string(),
                working_dir: None,
                auto_restart: None,
                health_check_url: None,
                health_check_interval: None,
                depends_on: vec![],
                env_vars: HashMap::new(),
            },
        );

        config.processes = processes;

        let order = config.get_startup_order().unwrap();

        // Database should come first
        assert_eq!(order[0], "database");
        // Cache should come second
        assert_eq!(order[1], "cache");
        // Web should come last
        assert_eq!(order[2], "web");
    }

    #[tokio::test]
    async fn test_find_project_config() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        let subdir = project_dir.join("subdir");

        tokio::fs::create_dir_all(&subdir).await.unwrap();

        let config_path = project_dir.join(".runcept.toml");
        let config = ProjectConfig::default();
        config.save_to_path(&config_path).await.unwrap();

        // Should find config from project root
        let found = find_project_config(&project_dir).await.unwrap();
        assert_eq!(found, Some(config_path.clone()));

        // Should find config from subdirectory
        let found = find_project_config(&subdir).await.unwrap();
        assert_eq!(found, Some(config_path));

        // Should return None when no config found
        let other_dir = temp_dir.path().join("other");
        tokio::fs::create_dir_all(&other_dir).await.unwrap();
        let found = find_project_config(&other_dir).await.unwrap();
        assert_eq!(found, None);
    }
}

use crate::config::global::GlobalConfig;
use crate::error::{Result, RunceptError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    #[serde(default)]
    pub environment: EnvironmentConfig,
    #[serde(default)]
    pub processes: HashMap<String, ProcessDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnvironmentConfig {
    #[serde(default)]
    pub name: String,
    pub inactivity_timeout: Option<String>,
    pub auto_shutdown: Option<bool>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDefinition {
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
    pub auto_restart: Option<bool>,
    pub health_check_url: Option<String>,
    pub health_check_interval: Option<u32>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
}

impl ProjectConfig {
    pub async fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(RunceptError::ConfigError(format!(
                "Project configuration file not found: {}",
                path.display()
            )));
        }

        let content = tokio::fs::read_to_string(path).await?;
        let config: Self = toml::from_str(&content)?;

        // Validate the loaded config
        config.validate()?;

        Ok(config)
    }

    pub async fn save_to_path(&self, path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|e| RunceptError::ConfigError(format!("Failed to serialize config: {e}")))?;

        tokio::fs::write(path, content).await?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        // Validate environment name
        if self.environment.name.trim().is_empty() {
            return Err(RunceptError::ConfigError(
                "Environment name cannot be empty".to_string(),
            ));
        }

        // Validate inactivity timeout format if specified
        if let Some(timeout) = &self.environment.inactivity_timeout {
            if !is_valid_timeout_format(timeout) {
                return Err(RunceptError::ConfigError(format!(
                    "Invalid timeout format: {timeout}. Use formats like '30m', '1h', '2h30m'"
                )));
            }
        }

        // Validate process dependencies exist
        let process_names: HashSet<String> = self.processes.keys().cloned().collect();

        for (process_name, process_def) in &self.processes {
            for dependency in &process_def.depends_on {
                if !process_names.contains(dependency) {
                    return Err(RunceptError::ConfigError(format!(
                        "Process '{process_name}' has dependency '{dependency}' which does not exist"
                    )));
                }
            }
        }

        // Check for circular dependencies
        self.validate_no_circular_dependencies()?;

        Ok(())
    }

    fn validate_no_circular_dependencies(&self) -> Result<()> {
        for process_name in self.processes.keys() {
            if self.has_circular_dependency(process_name, &mut HashSet::new()) {
                return Err(RunceptError::ConfigError(format!(
                    "Circular dependency detected involving process '{process_name}'"
                )));
            }
        }
        Ok(())
    }

    fn has_circular_dependency(&self, process_name: &str, visited: &mut HashSet<String>) -> bool {
        if visited.contains(process_name) {
            return true;
        }

        visited.insert(process_name.to_string());

        if let Some(process_def) = self.processes.get(process_name) {
            for dependency in &process_def.depends_on {
                if self.has_circular_dependency(dependency, visited) {
                    return true;
                }
            }
        }

        visited.remove(process_name);
        false
    }

    pub fn merge_with_global(&self, global_config: &GlobalConfig) -> ProjectConfig {
        let mut merged = self.clone();

        // Merge inactivity timeout if not specified in project
        if merged.environment.inactivity_timeout.is_none() {
            merged.environment.inactivity_timeout =
                Some(global_config.process.default_inactivity_timeout.clone());
        }

        // Merge auto_shutdown if not specified in project
        if merged.environment.auto_shutdown.is_none() {
            merged.environment.auto_shutdown = Some(true); // Default to true
        }

        // Merge environment variables (project overrides global)
        for (key, value) in &global_config.global_env_vars {
            merged
                .environment
                .env_vars
                .entry(key.clone())
                .or_insert(value.clone());
        }

        // Merge process defaults
        for process_def in merged.processes.values_mut() {
            if process_def.auto_restart.is_none() {
                process_def.auto_restart = Some(global_config.process.auto_restart_on_crash);
            }

            if process_def.health_check_interval.is_none() {
                process_def.health_check_interval =
                    Some(global_config.process.health_check_interval);
            }
        }

        merged
    }

    pub fn get_startup_order(&self) -> Result<Vec<String>> {
        let mut order = Vec::new();
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize in-degree count and graph
        for process_name in self.processes.keys() {
            in_degree.insert(process_name.clone(), 0);
            graph.insert(process_name.clone(), Vec::new());
        }

        // Build dependency graph and calculate in-degrees
        for (process_name, process_def) in &self.processes {
            for dependency in &process_def.depends_on {
                graph
                    .get_mut(dependency)
                    .unwrap()
                    .push(process_name.clone());
                *in_degree.get_mut(process_name).unwrap() += 1;
            }
        }

        // Topological sort using Kahn's algorithm
        let mut queue: VecDeque<String> = VecDeque::new();

        // Start with processes that have no dependencies
        for (process_name, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(process_name.clone());
            }
        }

        while let Some(process_name) = queue.pop_front() {
            order.push(process_name.clone());

            // Reduce in-degree for dependent processes
            if let Some(dependents) = graph.get(&process_name) {
                for dependent in dependents {
                    let degree = in_degree.get_mut(dependent).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }

        // Check if all processes were included (no circular dependencies)
        if order.len() != self.processes.len() {
            return Err(RunceptError::ConfigError(
                "Circular dependencies detected in process configuration".to_string(),
            ));
        }

        Ok(order)
    }

    pub fn get_process_definition(&self, name: &str) -> Option<&ProcessDefinition> {
        self.processes.get(name)
    }

    pub fn get_all_process_names(&self) -> Vec<String> {
        self.processes.keys().cloned().collect()
    }
}

pub async fn find_project_config(start_path: &Path) -> Result<Option<PathBuf>> {
    let mut current_path = start_path.to_path_buf();

    loop {
        let config_path = current_path.join(".runcept.toml");

        if config_path.exists() {
            return Ok(Some(config_path));
        }

        // Move to parent directory
        if let Some(parent) = current_path.parent() {
            current_path = parent.to_path_buf();
        } else {
            // Reached filesystem root
            break;
        }
    }

    Ok(None)
}

fn is_valid_timeout_format(timeout: &str) -> bool {
    // Simple validation for formats like "30m", "1h", "2h30m"
    let timeout = timeout.trim().to_lowercase();

    if timeout.is_empty() {
        return false;
    }

    // Check if it contains only digits, 'h', 'm', and is properly formatted
    let chars: Vec<char> = timeout.chars().collect();
    let mut i = 0;
    let mut found_unit = false;

    while i < chars.len() {
        // Expect digits
        let mut digit_count = 0;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
            digit_count += 1;
        }

        if digit_count == 0 {
            return false;
        }

        // Expect unit (h or m)
        if i < chars.len() && (chars[i] == 'h' || chars[i] == 'm') {
            found_unit = true;
            i += 1;
        } else {
            return false;
        }
    }

    found_unit
}
