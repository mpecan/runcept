use crate::error::{Result, RunceptError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Stdout,
    Stderr,
    Info,
    Error,
    Lifecycle, // Process lifecycle events (starting, stopping, etc.)
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Stdout => write!(f, "STDOUT"),
            LogLevel::Stderr => write!(f, "STDERR"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Lifecycle => write!(f, "LIFECYCLE"),
        }
    }
}

/// Helper struct for parsing legacy log format
#[derive(Debug)]
struct LegacyLogComponents {
    timestamp: String,
    level: String,
    message: String,
}

/// Process logger that handles writing process output to log files
pub struct ProcessLogger {
    pub process_name: String,
    pub log_file_path: PathBuf,
    log_file: Option<File>,
}

impl ProcessLogger {
    /// Create a new process logger
    pub async fn new(process_name: String, project_path: PathBuf) -> Result<Self> {
        let log_file_path = Self::build_log_file_path(&process_name, &project_path);

        // Ensure logs directory exists
        Self::ensure_logs_directory(&project_path).await?;

        // Open or create the log file in append mode
        let log_file = Self::open_log_file(&log_file_path).await?;

        Ok(Self {
            process_name,
            log_file_path,
            log_file: Some(log_file),
        })
    }

    /// Log a message with timestamp in JSON Line format
    pub async fn log_entry(&mut self, level: LogLevel, message: &str) -> Result<()> {
        let log_entry = self.create_log_entry(level, message);
        let log_line = Self::serialize_log_entry(&log_entry)?;

        self.write_to_file(&log_line).await
    }

    /// Helper method to write data to log file
    async fn write_to_file(&mut self, data: &str) -> Result<()> {
        if let Some(ref mut file) = self.log_file {
            file.write_all(data.as_bytes())
                .await
                .map_err(RunceptError::IoError)?;
            file.flush().await.map_err(RunceptError::IoError)?;
        }
        Ok(())
    }

    /// Log stdout output
    pub async fn log_stdout(&mut self, message: &str) -> Result<()> {
        self.log_entry(LogLevel::Stdout, message).await
    }

    /// Log stderr output
    pub async fn log_stderr(&mut self, message: &str) -> Result<()> {
        self.log_entry(LogLevel::Stderr, message).await
    }

    /// Log info message
    pub async fn log_info(&mut self, message: &str) -> Result<()> {
        self.log_entry(LogLevel::Info, message).await
    }

    /// Log error message
    pub async fn log_error(&mut self, message: &str) -> Result<()> {
        self.log_entry(LogLevel::Error, message).await
    }

    /// Log lifecycle event (starting, stopping, etc.)
    pub async fn log_lifecycle(&mut self, message: &str) -> Result<()> {
        self.log_entry(LogLevel::Lifecycle, message).await
    }

    /// Read log entries from the log file
    pub async fn read_logs(&self, max_lines: Option<usize>) -> Result<Vec<LogEntry>> {
        if !self.log_file_path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.log_file_path)
            .await
            .map_err(RunceptError::IoError)?;

        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut log_entries = Vec::new();

        while let Some(line) = lines.next_line().await.map_err(RunceptError::IoError)? {
            if let Some(entry) = self.parse_log_line(&line) {
                log_entries.push(entry);
            }
        }

        // Return the last N lines if max_lines is specified
        Ok(Self::tail_log_entries(log_entries, max_lines))
    }

    /// Helper method to get the last N entries from log entries
    fn tail_log_entries(mut log_entries: Vec<LogEntry>, max_lines: Option<usize>) -> Vec<LogEntry> {
        if let Some(max) = max_lines {
            if log_entries.len() > max {
                log_entries = log_entries.into_iter().rev().take(max).rev().collect();
            }
        }
        log_entries
    }

    /// Parse a JSON line into a LogEntry
    fn parse_log_line(&self, line: &str) -> Option<LogEntry> {
        // Try to parse as JSON first (new format)
        if let Ok(entry) = serde_json::from_str::<LogEntry>(line.trim()) {
            return Some(entry);
        }

        // Fallback to legacy format for backward compatibility
        Self::parse_legacy_log_line(line)
    }

    /// Parse legacy format log line
    fn parse_legacy_log_line(line: &str) -> Option<LogEntry> {
        // Expected format: "2025-07-15 07:31:44.540 [LEVEL] message"
        let components = Self::extract_legacy_log_components(line)?;

        let timestamp = Self::parse_legacy_timestamp(&components.timestamp)?;
        let level = Self::parse_legacy_level(&components.level)?;

        Some(LogEntry {
            timestamp,
            level,
            message: components.message.to_string(),
            process_name: None,
            process_id: None,
        })
    }

    /// Extract components from legacy log line
    fn extract_legacy_log_components(line: &str) -> Option<LegacyLogComponents> {
        let bracket_pos = line.find('[')?;
        if bracket_pos < 20 {
            return None;
        }

        let timestamp_str = line[..bracket_pos].trim();
        let rest = &line[bracket_pos..];

        if !rest.starts_with('[') {
            return None;
        }

        let end_bracket = rest.find(']')?;
        let level_str = &rest[1..end_bracket];
        let message = rest[end_bracket + 1..].trim();

        Some(LegacyLogComponents {
            timestamp: timestamp_str.to_string(),
            level: level_str.to_string(),
            message: message.to_string(),
        })
    }

    /// Parse legacy timestamp format
    fn parse_legacy_timestamp(timestamp_str: &str) -> Option<DateTime<Utc>> {
        chrono::NaiveDateTime::parse_from_str(timestamp_str, "%Y-%m-%d %H:%M:%S%.3f")
            .ok()
            .map(|naive_ts| naive_ts.and_utc())
    }

    /// Parse legacy log level
    fn parse_legacy_level(level_str: &str) -> Option<LogLevel> {
        match level_str {
            "STDOUT" => Some(LogLevel::Stdout),
            "STDERR" => Some(LogLevel::Stderr),
            "INFO" => Some(LogLevel::Info),
            "ERROR" => Some(LogLevel::Error),
            "LIFECYCLE" => Some(LogLevel::Lifecycle),
            _ => None,
        }
    }

    /// Close the log file
    pub async fn close(&mut self) -> Result<()> {
        if let Some(mut file) = self.log_file.take() {
            file.flush().await.map_err(RunceptError::IoError)?;
        }
        Ok(())
    }

    /// Get the log file path
    pub fn get_log_file_path(&self) -> &Path {
        &self.log_file_path
    }

    /// Helper method to create logs directory path
    fn get_logs_directory(project_path: &Path) -> PathBuf {
        project_path.join(".runcept").join("logs")
    }

    /// Helper method to create log file path
    fn build_log_file_path(process_name: &str, project_path: &Path) -> PathBuf {
        Self::get_logs_directory(project_path).join(format!("{process_name}.log"))
    }

    /// Helper method to ensure logs directory exists
    async fn ensure_logs_directory(project_path: &Path) -> Result<()> {
        let logs_dir = Self::get_logs_directory(project_path);
        tokio::fs::create_dir_all(&logs_dir)
            .await
            .map_err(RunceptError::IoError)
    }

    /// Helper method to open log file in append mode
    async fn open_log_file(log_file_path: &Path) -> Result<File> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file_path)
            .await
            .map_err(RunceptError::IoError)
    }

    /// Helper method to serialize log entry to JSON line
    fn serialize_log_entry(entry: &LogEntry) -> Result<String> {
        let json_line = serde_json::to_string(entry).map_err(|e| {
            RunceptError::SerializationError(format!("Failed to serialize log entry: {e}"))
        })?;
        Ok(format!("{json_line}\n"))
    }

    /// Helper method to create a log entry
    fn create_log_entry(&self, level: LogLevel, message: &str) -> LogEntry {
        LogEntry {
            timestamp: Utc::now(),
            level,
            message: message.trim().to_string(),
            process_name: Some(self.process_name.clone()),
            process_id: None, // Will be set when we have process ID context
        }
    }
}

/// Create a process logger for a given process and project path
pub async fn create_process_logger(
    process_name: &str,
    project_path: &Path,
) -> Result<ProcessLogger> {
    ProcessLogger::new(process_name.to_string(), project_path.to_path_buf()).await
}

/// Read logs for a specific process
pub async fn read_process_logs(
    process_name: &str,
    project_path: &Path,
    max_lines: Option<usize>,
) -> Result<Vec<LogEntry>> {
    let log_file_path = ProcessLogger::build_log_file_path(process_name, project_path);

    if !log_file_path.exists() {
        return Ok(Vec::new());
    }

    let logger = ProcessLogger {
        process_name: process_name.to_string(),
        log_file_path,
        log_file: None,
    };

    logger.read_logs(max_lines).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_process_logger_creation() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_path_buf();

        let logger = ProcessLogger::new("test-process".to_string(), project_path.clone())
            .await
            .unwrap();

        assert_eq!(logger.process_name, "test-process");
        assert!(logger.log_file_path.starts_with(&project_path));
        assert!(logger.log_file_path.ends_with("test-process.log"));
    }

    #[tokio::test]
    async fn test_log_output() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_path_buf();

        let mut logger = ProcessLogger::new("test-app".to_string(), project_path.clone())
            .await
            .unwrap();

        logger.log_stdout("Hello from stdout").await.unwrap();
        logger.log_stderr("Error from stderr").await.unwrap();

        // Read the log file and verify content - now using JSON format
        let log_content = tokio::fs::read_to_string(&logger.log_file_path)
            .await
            .unwrap();
        assert!(log_content.contains("Hello from stdout"));
        assert!(log_content.contains("Error from stderr"));
        assert!(log_content.contains("\"Stdout\""));
        assert!(log_content.contains("\"Stderr\""));
    }

    #[tokio::test]
    async fn test_read_logs() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_path_buf();

        let mut logger = ProcessLogger::new("test-reader".to_string(), project_path.clone())
            .await
            .unwrap();

        // Write some test data
        for i in 1..=5 {
            logger.log_stdout(&format!("Line {i}")).await.unwrap();
        }

        // Test reading all logs
        let logs = logger.read_logs(None).await.unwrap();
        assert_eq!(logs.len(), 5);

        // Test reading limited logs
        let logs = logger.read_logs(Some(3)).await.unwrap();
        assert_eq!(logs.len(), 3);
        assert!(
            logs[0].message.contains("Line 3")
                || logs[0].message.contains("Line 4")
                || logs[0].message.contains("Line 5")
        );
    }

    #[tokio::test]
    async fn test_logs_directory_creation() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_path_buf();

        // Ensure logs directory doesn't exist initially
        let logs_dir = project_path.join(".runcept").join("logs");
        assert!(!logs_dir.exists());

        let _logger = ProcessLogger::new("test-create".to_string(), project_path.clone())
            .await
            .unwrap();

        // Verify logs directory was created
        assert!(logs_dir.exists());
        assert!(logs_dir.is_dir());
    }

    #[tokio::test]
    async fn test_lifecycle_logging() {
        let temp_dir = TempDir::new().unwrap();
        let project_path = temp_dir.path().to_path_buf();

        let mut logger = ProcessLogger::new("test-lifecycle".to_string(), project_path.clone())
            .await
            .unwrap();

        // Log various lifecycle events
        logger
            .log_lifecycle("Starting process 'test-lifecycle'")
            .await
            .unwrap();
        logger
            .log_lifecycle("Process spawned successfully with PID 12345")
            .await
            .unwrap();
        logger.log_lifecycle("Health check passed").await.unwrap();
        logger
            .log_lifecycle("Successfully started process")
            .await
            .unwrap();

        // Read the log file and verify content
        let log_content = tokio::fs::read_to_string(&logger.log_file_path)
            .await
            .unwrap();

        assert!(log_content.contains("Starting process"));
        assert!(log_content.contains("Process spawned successfully"));
        assert!(log_content.contains("Health check passed"));
        assert!(log_content.contains("\"Lifecycle\""));

        // Test reading logs back
        let logs = logger.read_logs(None).await.unwrap();
        assert_eq!(logs.len(), 4);

        for log in &logs {
            assert!(matches!(log.level, super::LogLevel::Lifecycle));
            assert!(log.process_name.as_ref().unwrap() == "test-lifecycle");
        }
    }
}
