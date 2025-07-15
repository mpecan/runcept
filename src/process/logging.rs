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
}

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
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Stdout => write!(f, "STDOUT"),
            LogLevel::Stderr => write!(f, "STDERR"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
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
        // Create logs directory structure: {project_path}/.runcept/logs/
        let logs_dir = project_path.join(".runcept").join("logs");
        tokio::fs::create_dir_all(&logs_dir)
            .await
            .map_err(RunceptError::IoError)?;

        // Create log file path: {project_path}/.runcept/logs/{process_name}.log
        let log_file_path = logs_dir.join(format!("{process_name}.log"));

        // Open or create the log file in append mode
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file_path)
            .await
            .map_err(RunceptError::IoError)?;

        Ok(Self {
            process_name,
            log_file_path,
            log_file: Some(log_file),
        })
    }

    /// Log a message with timestamp in JSON Line format
    pub async fn log_entry(&mut self, level: LogLevel, message: &str) -> Result<()> {
        let timestamp = Utc::now();
        let log_entry = LogEntry {
            timestamp,
            level: level.clone(),
            message: message.trim().to_string(),
            process_name: Some(self.process_name.clone()),
            process_id: None, // Will be set when we have process ID context
        };

        // Serialize to JSON and add newline for JSONL format
        let json_line = serde_json::to_string(&log_entry).map_err(|e| {
            RunceptError::SerializationError(format!("Failed to serialize log entry: {e}"))
        })?;
        let log_line = format!("{json_line}\n");

        if let Some(ref mut file) = self.log_file {
            file.write_all(log_line.as_bytes())
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
        if let Some(max) = max_lines {
            if log_entries.len() > max {
                log_entries = log_entries.into_iter().rev().take(max).rev().collect();
            }
        }

        Ok(log_entries)
    }

    /// Parse a JSON line into a LogEntry
    fn parse_log_line(&self, line: &str) -> Option<LogEntry> {
        // Try to parse as JSON first (new format)
        if let Ok(entry) = serde_json::from_str::<LogEntry>(line.trim()) {
            return Some(entry);
        }

        // Fallback to legacy format for backward compatibility
        // Expected format: "2025-07-15 07:31:44.540 [LEVEL] message"

        // Find the first '[' to locate where the level starts
        let bracket_pos = line.find('[')?;
        if bracket_pos < 20 {
            return None;
        }

        // Extract timestamp (everything before the '[' minus the space)
        let timestamp_str = line[..bracket_pos].trim();

        let timestamp =
            match chrono::NaiveDateTime::parse_from_str(timestamp_str, "%Y-%m-%d %H:%M:%S%.3f") {
                Ok(naive_ts) => naive_ts.and_utc(),
                Err(_) => return None,
            };

        // The rest of the line from the bracket
        let rest = &line[bracket_pos..];

        // Parse level and message
        if !rest.starts_with('[') {
            return None;
        }

        let end_bracket = rest.find(']')?;
        let level_str = &rest[1..end_bracket];
        let message = rest[end_bracket + 1..].trim();

        let level = match level_str {
            "STDOUT" => LogLevel::Stdout,
            "STDERR" => LogLevel::Stderr,
            "INFO" => LogLevel::Info,
            "ERROR" => LogLevel::Error,
            _ => return None,
        };

        Some(LogEntry {
            timestamp,
            level,
            message: message.to_string(),
            process_name: None,
            process_id: None,
        })
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
    let logs_dir = project_path.join(".runcept").join("logs");
    let log_file_path = logs_dir.join(format!("{process_name}.log"));

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
