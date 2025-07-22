use crate::error::{Result, RunceptError};
use crate::process::traits::{ActivityEvent, ActivityLoggerTrait, ActivityType};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Pool, Row, Sqlite};
use std::sync::Arc;
use tracing::{debug, error, info};

/// Default implementation of ActivityLoggerTrait using SQLite database
///
/// This implementation stores activity logs in the existing activity_logs table
/// and provides querying capabilities for process and environment activities.
pub struct DatabaseActivityLogger {
    pool: Arc<Pool<Sqlite>>,
}

impl DatabaseActivityLogger {
    pub fn new(pool: Arc<Pool<Sqlite>>) -> Self {
        Self { pool }
    }

    /// Convert ActivityType to string for database storage
    fn activity_type_to_string(activity_type: &ActivityType) -> String {
        match activity_type {
            ActivityType::ProcessStarted => "process_started".to_string(),
            ActivityType::ProcessStopped => "process_stopped".to_string(),
            ActivityType::ProcessCrashed => "process_crashed".to_string(),
            ActivityType::ProcessRestarted => "process_restarted".to_string(),
            ActivityType::StatusChanged { from, to } => {
                format!("status_changed:{}:{}", from, to)
            }
            ActivityType::HealthCheckPassed => "health_check_passed".to_string(),
            ActivityType::HealthCheckFailed => "health_check_failed".to_string(),
            ActivityType::ConfigurationChanged => "configuration_changed".to_string(),
            ActivityType::EnvironmentActivated => "environment_activated".to_string(),
            ActivityType::EnvironmentDeactivated => "environment_deactivated".to_string(),
        }
    }

    /// Convert string from database back to ActivityType
    fn string_to_activity_type(type_str: &str) -> ActivityType {
        if type_str.starts_with("status_changed:") {
            let parts: Vec<&str> = type_str.split(':').collect();
            if parts.len() == 3 {
                // Parse the from and to status - simplified implementation
                // In production, you might want more robust parsing
                match (parts[1], parts[2]) {
                    ("Running", "Stopped") => ActivityType::StatusChanged {
                        from: crate::process::ProcessStatus::Running,
                        to: crate::process::ProcessStatus::Stopped,
                    },
                    ("Stopped", "Starting") => ActivityType::StatusChanged {
                        from: crate::process::ProcessStatus::Stopped,
                        to: crate::process::ProcessStatus::Starting,
                    },
                    ("Starting", "Running") => ActivityType::StatusChanged {
                        from: crate::process::ProcessStatus::Starting,
                        to: crate::process::ProcessStatus::Running,
                    },
                    _ => ActivityType::ConfigurationChanged, // Default for unknown status combinations
                }
            } else {
                ActivityType::ConfigurationChanged
            }
        } else {
            match type_str {
                "process_started" => ActivityType::ProcessStarted,
                "process_stopped" => ActivityType::ProcessStopped,
                "process_crashed" => ActivityType::ProcessCrashed,
                "process_restarted" => ActivityType::ProcessRestarted,
                "health_check_passed" => ActivityType::HealthCheckPassed,
                "health_check_failed" => ActivityType::HealthCheckFailed,
                "configuration_changed" => ActivityType::ConfigurationChanged,
                "environment_activated" => ActivityType::EnvironmentActivated,
                "environment_deactivated" => ActivityType::EnvironmentDeactivated,
                _ => ActivityType::ConfigurationChanged, // Default for unknown types
            }
        }
    }

    /// Insert a single activity log entry into the database
    async fn insert_activity_log(&self, event: &ActivityEvent) -> Result<()> {
        let activity_type_str = Self::activity_type_to_string(&event.activity_type);
        let details_json = serde_json::to_string(&event.details)?;

        let query = r#"
            INSERT INTO activity_logs (
                process_id, 
                environment_id, 
                activity_type, 
                timestamp, 
                details
            ) VALUES (?, ?, ?, ?, ?)
        "#;

        let result = sqlx::query(query)
            .bind(&event.process_id)
            .bind(&event.environment_id)
            .bind(&activity_type_str)
            .bind(event.timestamp)
            .bind(&details_json)
            .execute(self.pool.as_ref())
            .await;

        match result {
            Ok(_) => {
                debug!("Activity log inserted: {:?}", activity_type_str);
                Ok(())
            }
            Err(e) => {
                error!("Failed to insert activity log: {}", e);
                Err(RunceptError::DatabaseError(format!(
                    "Failed to insert activity log: {}",
                    e
                )))
            }
        }
    }
}

#[async_trait]
impl ActivityLoggerTrait for DatabaseActivityLogger {
    async fn log_activity(&self, event: ActivityEvent) -> Result<()> {
        self.insert_activity_log(&event).await
    }

    async fn log_activities(&self, events: &[ActivityEvent]) -> Result<()> {
        // Use a transaction for batch inserts
        let mut tx = self.pool.begin().await?;

        for event in events {
            let activity_type_str = Self::activity_type_to_string(&event.activity_type);
            let details_json = serde_json::to_string(&event.details)?;

            let query = r#"
                INSERT INTO activity_logs (
                    process_id, 
                    environment_id, 
                    activity_type, 
                    timestamp, 
                    details
                ) VALUES (?, ?, ?, ?, ?)
            "#;

            sqlx::query(query)
                .bind(&event.process_id)
                .bind(&event.environment_id)
                .bind(&activity_type_str)
                .bind(event.timestamp)
                .bind(&details_json)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        info!("Batch inserted {} activity logs", events.len());
        Ok(())
    }

    async fn get_process_activities(
        &self,
        process_id: &str,
        since: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<ActivityEvent>> {
        let mut query_str = r#"
            SELECT process_id, environment_id, activity_type, timestamp, details
            FROM activity_logs 
            WHERE process_id = ?
        "#
        .to_string();

        let mut bind_values = vec![process_id.to_string()];

        if since.is_some() {
            query_str.push_str(" AND timestamp >= ?");
            bind_values.push(since.unwrap().to_rfc3339());
        }

        query_str.push_str(" ORDER BY timestamp DESC");

        if let Some(limit_val) = limit {
            query_str.push_str(&format!(" LIMIT {}", limit_val));
        }

        let mut query = sqlx::query(&query_str);
        for value in bind_values {
            query = query.bind(value);
        }

        let rows = query.fetch_all(self.pool.as_ref()).await?;

        let mut activities = Vec::new();
        for row in rows {
            let process_id: Option<String> = row.get("process_id");
            let environment_id: Option<String> = row.get("environment_id");
            let activity_type_str: String = row.get("activity_type");
            let timestamp: DateTime<Utc> = row.get("timestamp");
            let details_json: String = row.get("details");

            let activity_type = Self::string_to_activity_type(&activity_type_str);
            let details =
                serde_json::from_str(&details_json).unwrap_or_else(|_| serde_json::json!({}));

            activities.push(ActivityEvent {
                process_id,
                environment_id,
                activity_type,
                timestamp,
                details,
            });
        }

        Ok(activities)
    }

    async fn get_environment_activities(
        &self,
        environment_id: &str,
        since: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<ActivityEvent>> {
        let mut query_str = r#"
            SELECT process_id, environment_id, activity_type, timestamp, details
            FROM activity_logs 
            WHERE environment_id = ?
        "#
        .to_string();

        let mut bind_values = vec![environment_id.to_string()];

        if since.is_some() {
            query_str.push_str(" AND timestamp >= ?");
            bind_values.push(since.unwrap().to_rfc3339());
        }

        query_str.push_str(" ORDER BY timestamp DESC");

        if let Some(limit_val) = limit {
            query_str.push_str(&format!(" LIMIT {}", limit_val));
        }

        let mut query = sqlx::query(&query_str);
        for value in bind_values {
            query = query.bind(value);
        }

        let rows = query.fetch_all(self.pool.as_ref()).await?;

        let mut activities = Vec::new();
        for row in rows {
            let process_id: Option<String> = row.get("process_id");
            let environment_id: Option<String> = row.get("environment_id");
            let activity_type_str: String = row.get("activity_type");
            let timestamp: DateTime<Utc> = row.get("timestamp");
            let details_json: String = row.get("details");

            let activity_type = Self::string_to_activity_type(&activity_type_str);
            let details =
                serde_json::from_str(&details_json).unwrap_or_else(|_| serde_json::json!({}));

            activities.push(ActivityEvent {
                process_id,
                environment_id,
                activity_type,
                timestamp,
                details,
            });
        }

        Ok(activities)
    }
}

/// Convenience functions for common activity logging scenarios

/// Log a process lifecycle event (start, stop, crash, etc.)
pub async fn log_process_lifecycle_event(
    logger: &dyn ActivityLoggerTrait,
    process_id: &str,
    environment_id: &str,
    activity_type: ActivityType,
    additional_details: Option<serde_json::Value>,
) -> Result<()> {
    let mut details = serde_json::json!({
        "process_id": process_id,
        "environment_id": environment_id,
        "timestamp": Utc::now().to_rfc3339(),
    });

    if let Some(extra) = additional_details {
        if let serde_json::Value::Object(extra_map) = extra {
            if let serde_json::Value::Object(ref mut details_map) = details {
                for (key, value) in extra_map {
                    details_map.insert(key, value);
                }
            }
        }
    }

    let event = ActivityEvent {
        process_id: Some(process_id.to_string()),
        environment_id: Some(environment_id.to_string()),
        activity_type,
        timestamp: Utc::now(),
        details,
    };

    logger.log_activity(event).await
}

/// Log a health check result
pub async fn log_health_check_result(
    logger: &dyn ActivityLoggerTrait,
    process_id: &str,
    environment_id: &str,
    passed: bool,
    check_type: &str,
    duration_ms: u64,
    error: Option<&str>,
) -> Result<()> {
    let activity_type = if passed {
        ActivityType::HealthCheckPassed
    } else {
        ActivityType::HealthCheckFailed
    };

    let mut details = serde_json::json!({
        "process_id": process_id,
        "environment_id": environment_id,
        "check_type": check_type,
        "duration_ms": duration_ms,
        "passed": passed,
    });

    if let Some(error_msg) = error {
        details["error"] = serde_json::Value::String(error_msg.to_string());
    }

    log_process_lifecycle_event(
        logger,
        process_id,
        environment_id,
        activity_type,
        Some(details),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use chrono::Utc;

    async fn create_test_database() -> Database {
        let db = Database::new("sqlite::memory:").await.unwrap();
        db.init().await.unwrap();
        db
    }

    /// Helper to create test environment and process records
    async fn create_test_records(
        db: &Database,
        environment_id: &str,
        process_id: &str,
        process_name: &str,
    ) {
        let now = chrono::Utc::now();

        // Create environment (ignore if it already exists)
        let _ = sqlx::query(
            r#"
            INSERT OR IGNORE INTO environments (id, name, description, project_path, config_path, status, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(environment_id)
        .bind("Test Environment")
        .bind("Test Description")
        .bind("/tmp/test")
        .bind("/tmp/test/.runcept.toml")
        .bind("active")
        .bind(now)
        .bind(now)
        .execute(db.get_pool())
        .await;

        // Create process (replace if it already exists)
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO processes (
                id, name, command, working_dir, environment_id, status, pid,
                created_at, updated_at, last_activity, auto_restart,
                health_check_url, health_check_interval, depends_on, env_vars
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(process_id)
        .bind(process_name)
        .bind("echo hello")
        .bind("/tmp")
        .bind(environment_id)
        .bind("stopped")
        .bind(None::<i64>)
        .bind(now)
        .bind(now)
        .bind(None::<chrono::DateTime<chrono::Utc>>)
        .bind(false)
        .bind(None::<String>)
        .bind(None::<i64>)
        .bind("[]") // empty JSON array
        .bind("{}") // empty JSON object
        .execute(db.get_pool())
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_activity_logger_single_event() {
        let db = create_test_database().await;
        let logger = DatabaseActivityLogger::new(Arc::new(db.get_pool().clone()));

        // Create prerequisite records
        create_test_records(&db, "env1", "env1:test-process", "test-process").await;

        let event = ActivityEvent {
            process_id: Some("env1:test-process".to_string()),
            environment_id: Some("env1".to_string()),
            activity_type: ActivityType::ProcessStarted,
            timestamp: Utc::now(),
            details: serde_json::json!({
                "pid": 1234,
                "command": "echo hello"
            }),
        };

        logger.log_activity(event.clone()).await.unwrap();

        // Retrieve the activity
        let activities = logger
            .get_process_activities("env1:test-process", None, None)
            .await
            .unwrap();

        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].process_id, event.process_id);
        assert!(matches!(
            activities[0].activity_type,
            ActivityType::ProcessStarted
        ));
    }

    #[tokio::test]
    async fn test_activity_logger_batch_events() {
        let db = create_test_database().await;
        let logger = DatabaseActivityLogger::new(Arc::new(db.get_pool().clone()));

        // Create prerequisite records
        create_test_records(&db, "env1", "env1:test-process", "test-process").await;

        let events = vec![
            ActivityEvent {
                process_id: Some("env1:test-process".to_string()),
                environment_id: Some("env1".to_string()),
                activity_type: ActivityType::ProcessStarted,
                timestamp: Utc::now(),
                details: serde_json::json!({"step": 1}),
            },
            ActivityEvent {
                process_id: Some("env1:test-process".to_string()),
                environment_id: Some("env1".to_string()),
                activity_type: ActivityType::StatusChanged {
                    from: crate::process::ProcessStatus::Starting,
                    to: crate::process::ProcessStatus::Running,
                },
                timestamp: Utc::now(),
                details: serde_json::json!({"step": 2}),
            },
            ActivityEvent {
                process_id: Some("env1:test-process".to_string()),
                environment_id: Some("env1".to_string()),
                activity_type: ActivityType::ProcessStopped,
                timestamp: Utc::now(),
                details: serde_json::json!({"step": 3}),
            },
        ];

        logger.log_activities(&events).await.unwrap();

        // Retrieve the activities
        let activities = logger
            .get_process_activities("env1:test-process", None, None)
            .await
            .unwrap();

        assert_eq!(activities.len(), 3);
    }

    #[tokio::test]
    async fn test_get_environment_activities() {
        let db = create_test_database().await;
        let logger = DatabaseActivityLogger::new(Arc::new(db.get_pool().clone()));

        // Create prerequisite records for multiple environments and processes
        create_test_records(&db, "env1", "env1:process1", "process1").await;
        create_test_records(&db, "env1", "env1:process2", "process2").await;
        create_test_records(&db, "env2", "env2:process1", "process1").await;

        let events = vec![
            ActivityEvent {
                process_id: Some("env1:process1".to_string()),
                environment_id: Some("env1".to_string()),
                activity_type: ActivityType::ProcessStarted,
                timestamp: Utc::now(),
                details: serde_json::json!({}),
            },
            ActivityEvent {
                process_id: Some("env1:process2".to_string()),
                environment_id: Some("env1".to_string()),
                activity_type: ActivityType::ProcessStarted,
                timestamp: Utc::now(),
                details: serde_json::json!({}),
            },
            ActivityEvent {
                process_id: Some("env2:process1".to_string()),
                environment_id: Some("env2".to_string()),
                activity_type: ActivityType::ProcessStarted,
                timestamp: Utc::now(),
                details: serde_json::json!({}),
            },
        ];

        logger.log_activities(&events).await.unwrap();

        // Get activities for env1 only
        let env1_activities = logger
            .get_environment_activities("env1", None, None)
            .await
            .unwrap();

        assert_eq!(env1_activities.len(), 2);
        for activity in &env1_activities {
            assert_eq!(activity.environment_id, Some("env1".to_string()));
        }

        // Get activities for env2 only
        let env2_activities = logger
            .get_environment_activities("env2", None, None)
            .await
            .unwrap();

        assert_eq!(env2_activities.len(), 1);
        assert_eq!(env2_activities[0].environment_id, Some("env2".to_string()));
    }

    #[tokio::test]
    async fn test_activity_logger_with_limit() {
        let db = create_test_database().await;
        let logger = DatabaseActivityLogger::new(Arc::new(db.get_pool().clone()));

        // Create prerequisite records
        create_test_records(&db, "env1", "env1:test-process", "test-process").await;

        // Insert 5 events
        for i in 0..5 {
            let event = ActivityEvent {
                process_id: Some("env1:test-process".to_string()),
                environment_id: Some("env1".to_string()),
                activity_type: ActivityType::ProcessStarted,
                timestamp: Utc::now(),
                details: serde_json::json!({"step": i}),
            };
            logger.log_activity(event).await.unwrap();
        }

        // Retrieve with limit
        let activities = logger
            .get_process_activities("env1:test-process", None, Some(3))
            .await
            .unwrap();

        assert_eq!(activities.len(), 3);
    }

    #[tokio::test]
    async fn test_log_process_lifecycle_event_helper() {
        let db = create_test_database().await;
        let logger = DatabaseActivityLogger::new(Arc::new(db.get_pool().clone()));

        // Create prerequisite records
        create_test_records(&db, "env1", "env1:test-process", "test-process").await;

        log_process_lifecycle_event(
            &logger,
            "env1:test-process",
            "env1",
            ActivityType::ProcessStarted,
            Some(serde_json::json!({"pid": 1234})),
        )
        .await
        .unwrap();

        let activities = logger
            .get_process_activities("env1:test-process", None, None)
            .await
            .unwrap();

        assert_eq!(activities.len(), 1);
        assert!(matches!(
            activities[0].activity_type,
            ActivityType::ProcessStarted
        ));
    }

    #[tokio::test]
    async fn test_log_health_check_result_helper() {
        let db = create_test_database().await;
        let logger = DatabaseActivityLogger::new(Arc::new(db.get_pool().clone()));

        // Create prerequisite records
        create_test_records(&db, "env1", "env1:test-process", "test-process").await;

        // Log successful health check
        log_health_check_result(
            &logger,
            "env1:test-process",
            "env1",
            true,
            "http",
            150,
            None,
        )
        .await
        .unwrap();

        // Log failed health check
        log_health_check_result(
            &logger,
            "env1:test-process",
            "env1",
            false,
            "tcp",
            5000,
            Some("Connection refused"),
        )
        .await
        .unwrap();

        let activities = logger
            .get_process_activities("env1:test-process", None, None)
            .await
            .unwrap();

        assert_eq!(activities.len(), 2);
    }
}
