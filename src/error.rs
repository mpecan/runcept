#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_error_creation() {
        let error = RunceptError::ProcessError("test process error".to_string());
        assert_eq!(error.to_string(), "Process error: test process error");
    }

    #[test]
    fn test_config_error_creation() {
        let error = RunceptError::ConfigError("invalid config".to_string());
        assert_eq!(error.to_string(), "Configuration error: invalid config");
    }

    #[test]
    fn test_database_error_creation() {
        let error = RunceptError::DatabaseError("connection failed".to_string());
        assert_eq!(error.to_string(), "Database error: connection failed");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let runcept_error: RunceptError = io_error.into();
        assert!(matches!(runcept_error, RunceptError::IoError(_)));
    }

    #[test]
    fn test_toml_error_conversion() {
        let invalid_toml = "invalid = [toml";
        let toml_error = toml::from_str::<toml::Value>(invalid_toml).unwrap_err();
        let runcept_error: RunceptError = toml_error.into();
        assert!(matches!(runcept_error, RunceptError::ConfigError(_)));
    }

    #[test]
    fn test_error_is_retriable() {
        let io_error =
            RunceptError::IoError(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        assert!(io_error.is_retriable());

        let config_error = RunceptError::ConfigError("test".to_string());
        assert!(!config_error.is_retriable());
    }
}

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RunceptError {
    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Environment error: {0}")]
    EnvironmentError(String),

    #[error("Health check error: {0}")]
    HealthCheckError(String),

    #[error("MCP error: {0}")]
    McpError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("SQL error: {0}")]
    SqlError(#[from] sqlx::Error),

    #[error("Migration error: {0}")]
    MigrationError(#[from] sqlx::migrate::MigrateError),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("System error: {0}")]
    SystemError(String),

    #[error("Timeout error: {0}")]
    TimeoutError(String),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

impl From<toml::de::Error> for RunceptError {
    fn from(error: toml::de::Error) -> Self {
        RunceptError::ConfigError(error.to_string())
    }
}

impl RunceptError {
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            RunceptError::IoError(_)
                | RunceptError::HttpError(_)
                | RunceptError::DatabaseError(_)
                | RunceptError::SqlError(_)
                | RunceptError::MigrationError(_)
                | RunceptError::HealthCheckError(_)
                | RunceptError::TimeoutError(_)
                | RunceptError::ConnectionError(_)
        )
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            RunceptError::ProcessError(_) => "PROCESS_ERROR",
            RunceptError::ConfigError(_) => "CONFIG_ERROR",
            RunceptError::DatabaseError(_) => "DATABASE_ERROR",
            RunceptError::EnvironmentError(_) => "ENVIRONMENT_ERROR",
            RunceptError::HealthCheckError(_) => "HEALTH_CHECK_ERROR",
            RunceptError::McpError(_) => "MCP_ERROR",
            RunceptError::IoError(_) => "IO_ERROR",
            RunceptError::JsonError(_) => "JSON_ERROR",
            RunceptError::SqlError(_) => "SQL_ERROR",
            RunceptError::MigrationError(_) => "MIGRATION_ERROR",
            RunceptError::HttpError(_) => "HTTP_ERROR",
            RunceptError::SystemError(_) => "SYSTEM_ERROR",
            RunceptError::TimeoutError(_) => "TIMEOUT_ERROR",
            RunceptError::ConnectionError(_) => "CONNECTION_ERROR",
            RunceptError::SerializationError(_) => "SERIALIZATION_ERROR",
        }
    }
}

pub type Result<T> = std::result::Result<T, RunceptError>;
