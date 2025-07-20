#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn test_global_config_default() {
        let config = GlobalConfig::default();

        assert_eq!(config.database.connection_timeout, 30);
        assert_eq!(config.database.cleanup_interval_hours, 24);
        assert_eq!(config.database.activity_log_retention_days, 30);
        assert_eq!(config.process.default_inactivity_timeout, "30m");
        assert_eq!(config.process.health_check_interval, 30);
        assert_eq!(config.process.restart_delay, 5);
        assert_eq!(config.process.inactivity_timeout, 1800);
        assert_eq!(config.environment.inactivity_timeout, 1800);
        assert!(config.environment.auto_shutdown_enabled);
        assert_eq!(config.mcp.port, 3000);
        assert!(config.mcp.auto_start);
        assert!(config.logging.level == "info");
    }

    #[test]
    fn test_global_config_serialization() {
        let config = GlobalConfig::default();

        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("[database]"));
        assert!(toml_str.contains("[process]"));
        assert!(toml_str.contains("[environment]"));
        assert!(toml_str.contains("[mcp]"));
        assert!(toml_str.contains("[logging]"));

        let deserialized: GlobalConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(
            config.database.connection_timeout,
            deserialized.database.connection_timeout
        );
        assert_eq!(
            config.process.default_inactivity_timeout,
            deserialized.process.default_inactivity_timeout
        );
    }

    #[test]
    fn test_get_config_dir() {
        let config_dir = get_config_dir().unwrap();
        assert!(config_dir.ends_with(".runcept"));
    }

    #[tokio::test]
    async fn test_global_config_load_default_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config = GlobalConfig::load_from_path(&config_path).await.unwrap();

        // Should be default config when file doesn't exist
        assert_eq!(config.database.connection_timeout, 30);
        assert_eq!(config.process.default_inactivity_timeout, "30m");
    }

    #[tokio::test]
    async fn test_global_config_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let mut config = GlobalConfig::default();
        config.database.connection_timeout = 60;
        config.process.default_inactivity_timeout = "1h".to_string();
        config.mcp.port = 4000;

        config.save_to_path(&config_path).await.unwrap();

        let loaded_config = GlobalConfig::load_from_path(&config_path).await.unwrap();
        assert_eq!(loaded_config.database.connection_timeout, 60);
        assert_eq!(loaded_config.process.default_inactivity_timeout, "1h");
        assert_eq!(loaded_config.mcp.port, 4000);
    }

    #[tokio::test]
    async fn test_global_config_partial_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Write partial config
        let partial_config = r#"
[database]
connection_timeout = 120

[process]
default_inactivity_timeout = "2h"
"#;

        tokio::fs::write(&config_path, partial_config)
            .await
            .unwrap();

        let config = GlobalConfig::load_from_path(&config_path).await.unwrap();

        // Should have custom values where specified
        assert_eq!(config.database.connection_timeout, 120);
        assert_eq!(config.process.default_inactivity_timeout, "2h");

        // Should have defaults for unspecified values
        assert_eq!(config.database.cleanup_interval_hours, 24); // default
        assert_eq!(config.mcp.port, 3000); // default
    }

    #[tokio::test]
    async fn test_global_config_manager() {
        let temp_dir = TempDir::new().unwrap();
        unsafe {
            env::set_var("HOME", temp_dir.path());
        }

        let manager = GlobalConfigManager::new_with_custom_dir(temp_dir.path().join(".runcept"));

        let config = manager.load().await.unwrap();
        assert_eq!(config.database.connection_timeout, 30); // default

        let mut new_config = config.clone();
        new_config.process.default_inactivity_timeout = "45m".to_string();

        manager.save(&new_config).await.unwrap();

        let reloaded = manager.load().await.unwrap();
        assert_eq!(reloaded.process.default_inactivity_timeout, "45m");
    }

    #[test]
    fn test_merge_env_vars() {
        let mut config = GlobalConfig::default();
        let mut env_vars = std::collections::HashMap::new();
        env_vars.insert("NODE_ENV".to_string(), "production".to_string());
        env_vars.insert("DEBUG".to_string(), "true".to_string());

        config.merge_env_vars(&env_vars);

        assert_eq!(
            config.global_env_vars.get("NODE_ENV"),
            Some(&"production".to_string())
        );
        assert_eq!(
            config.global_env_vars.get("DEBUG"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn test_config_validation() {
        let mut config = GlobalConfig::default();

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Invalid timeout should fail
        config.database.connection_timeout = 0;
        assert!(config.validate().is_err());

        // Reset and test invalid port
        config = GlobalConfig::default();
        config.mcp.port = 0; // invalid port
        assert!(config.validate().is_err());
    }
}

use crate::error::{Result, RunceptError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub process: ProcessConfig,
    #[serde(default)]
    pub environment: EnvironmentConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub global_env_vars: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u32,
    #[serde(default = "default_cleanup_interval_hours")]
    pub cleanup_interval_hours: u32,
    #[serde(default = "default_activity_log_retention_days")]
    pub activity_log_retention_days: u32,
    pub database_path: Option<String>, // If None, uses default ~/.runcept/runcept.db
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    #[serde(default = "default_inactivity_timeout")]
    pub default_inactivity_timeout: String, // e.g., "30m", "1h", "2h30m"
    #[serde(default = "default_health_check_interval")]
    pub health_check_interval: u32, // seconds
    #[serde(default = "default_health_check_timeout")]
    pub health_check_timeout: u32, // seconds - timeout for individual health checks
    #[serde(default = "default_startup_health_check_timeout")]
    pub startup_health_check_timeout: u32, // seconds - timeout for startup health check blocking
    #[serde(default = "default_restart_delay")]
    pub restart_delay: u32, // seconds
    #[serde(default = "default_max_restart_attempts")]
    pub max_restart_attempts: u32,
    #[serde(default = "default_auto_restart_on_crash")]
    pub auto_restart_on_crash: bool,
    #[serde(default = "default_inactivity_timeout_seconds")]
    pub inactivity_timeout: u32, // seconds
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval: u32, // seconds
    #[serde(default = "default_max_activity_age")]
    pub max_activity_age: u32, // seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    #[serde(default = "default_environment_inactivity_timeout")]
    pub inactivity_timeout: u32, // seconds
    #[serde(default = "default_auto_shutdown_enabled")]
    pub auto_shutdown_enabled: bool,
    #[serde(default = "default_graceful_shutdown_timeout")]
    pub graceful_shutdown_timeout: u32, // seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_mcp_port")]
    pub port: u16,
    #[serde(default = "default_mcp_host")]
    pub host: String,
    #[serde(default = "default_mcp_auto_start")]
    pub auto_start: bool,
    #[serde(default)]
    pub auth_enabled: bool,
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String, // "trace", "debug", "info", "warn", "error"
    #[serde(default = "default_file_enabled")]
    pub file_enabled: bool,
    pub file_path: Option<String>, // If None, uses default ~/.runcept/logs/
    #[serde(default = "default_max_file_size_mb")]
    pub max_file_size_mb: u32,
    #[serde(default = "default_max_files")]
    pub max_files: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            connection_timeout: default_connection_timeout(),
            cleanup_interval_hours: default_cleanup_interval_hours(),
            activity_log_retention_days: default_activity_log_retention_days(),
            database_path: None,
        }
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            default_inactivity_timeout: default_inactivity_timeout(),
            health_check_interval: default_health_check_interval(),
            health_check_timeout: default_health_check_timeout(),
            startup_health_check_timeout: default_startup_health_check_timeout(),
            restart_delay: default_restart_delay(),
            max_restart_attempts: default_max_restart_attempts(),
            auto_restart_on_crash: default_auto_restart_on_crash(),
            inactivity_timeout: default_inactivity_timeout_seconds(),
            cleanup_interval: default_cleanup_interval(),
            max_activity_age: default_max_activity_age(),
        }
    }
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            inactivity_timeout: default_environment_inactivity_timeout(),
            auto_shutdown_enabled: default_auto_shutdown_enabled(),
            graceful_shutdown_timeout: default_graceful_shutdown_timeout(),
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            port: default_mcp_port(),
            host: default_mcp_host(),
            auto_start: default_mcp_auto_start(),
            auth_enabled: false,
            auth_token: None,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_enabled: default_file_enabled(),
            file_path: None,
            max_file_size_mb: default_max_file_size_mb(),
            max_files: default_max_files(),
        }
    }
}

impl GlobalConfig {
    pub async fn load() -> Result<Self> {
        let config_dir = get_config_dir()?;
        let config_path = config_dir.join("config.toml");
        Self::load_from_path(&config_path).await
    }

    pub async fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            // Return default config if file doesn't exist
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(path).await?;
        let config: Self = toml::from_str(&content)?;

        // Validate the loaded config
        config.validate()?;

        Ok(config)
    }

    pub async fn save(&self) -> Result<()> {
        let config_dir = get_config_dir()?;
        let config_path = config_dir.join("config.toml");
        self.save_to_path(&config_path).await
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
        // Validate database config
        if self.database.connection_timeout == 0 {
            return Err(RunceptError::ConfigError(
                "Database connection timeout must be greater than 0".to_string(),
            ));
        }

        // Validate MCP config
        if self.mcp.port == 0 {
            return Err(RunceptError::ConfigError(
                "MCP port must be between 1 and 65535".to_string(),
            ));
        }

        // Validate logging level
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.logging.level.as_str()) {
            return Err(RunceptError::ConfigError(format!(
                "Invalid logging level: {}",
                self.logging.level
            )));
        }

        Ok(())
    }

    pub fn merge_env_vars(&mut self, env_vars: &HashMap<String, String>) {
        for (key, value) in env_vars {
            self.global_env_vars.insert(key.clone(), value.clone());
        }
    }

    pub fn get_database_path(&self) -> PathBuf {
        match &self.database.database_path {
            Some(path) => PathBuf::from(path),
            None => {
                let config_dir = get_config_dir().unwrap_or_else(|_| PathBuf::from(".runcept"));
                config_dir.join("runcept.db")
            }
        }
    }

    pub fn get_log_dir(&self) -> PathBuf {
        match &self.logging.file_path {
            Some(path) => PathBuf::from(path),
            None => {
                let config_dir = get_config_dir().unwrap_or_else(|_| PathBuf::from(".runcept"));
                config_dir.join("logs")
            }
        }
    }
}

pub struct GlobalConfigManager {
    config_dir: PathBuf,
}

impl GlobalConfigManager {
    pub fn new() -> Result<Self> {
        let config_dir = get_config_dir()?;
        Ok(Self { config_dir })
    }

    pub fn new_with_custom_dir(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    pub async fn load(&self) -> Result<GlobalConfig> {
        let config_path = self.config_dir.join("config.toml");
        GlobalConfig::load_from_path(&config_path).await
    }

    pub async fn save(&self, config: &GlobalConfig) -> Result<()> {
        let config_path = self.config_dir.join("config.toml");
        config.save_to_path(&config_path).await
    }

    pub async fn ensure_config_dir(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.config_dir).await?;
        Ok(())
    }

    pub fn get_config_dir(&self) -> &Path {
        &self.config_dir
    }
}

pub fn get_config_dir() -> Result<PathBuf> {
    let home_dir = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| RunceptError::ConfigError("Could not determine home directory".to_string()))?;

    Ok(PathBuf::from(home_dir).join(".runcept"))
}

// Default value functions for serde
fn default_connection_timeout() -> u32 {
    30
}
fn default_cleanup_interval_hours() -> u32 {
    24
}
fn default_activity_log_retention_days() -> u32 {
    30
}
fn default_inactivity_timeout() -> String {
    "30m".to_string()
}
fn default_health_check_interval() -> u32 {
    30
}
fn default_health_check_timeout() -> u32 {
    1 // 1 second for individual health checks
}
fn default_startup_health_check_timeout() -> u32 {
    15 // 15 seconds to wait for health check during startup
}
fn default_restart_delay() -> u32 {
    5
}
fn default_max_restart_attempts() -> u32 {
    3
}
fn default_auto_restart_on_crash() -> bool {
    true
}
fn default_mcp_port() -> u16 {
    3000
}
fn default_mcp_host() -> String {
    "127.0.0.1".to_string()
}
fn default_mcp_auto_start() -> bool {
    true
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_file_enabled() -> bool {
    true
}
fn default_max_file_size_mb() -> u32 {
    10
}
fn default_max_files() -> u32 {
    5
}

fn default_inactivity_timeout_seconds() -> u32 {
    1800 // 30 minutes
}

fn default_cleanup_interval() -> u32 {
    300 // 5 minutes
}

fn default_max_activity_age() -> u32 {
    3600 // 1 hour
}

fn default_environment_inactivity_timeout() -> u32 {
    1800 // 30 minutes
}

fn default_auto_shutdown_enabled() -> bool {
    true
}

fn default_graceful_shutdown_timeout() -> u32 {
    30 // 30 seconds
}
