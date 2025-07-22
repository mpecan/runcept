use crate::config::GlobalConfig;
use crate::error::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Instant, interval, sleep};

#[derive(Debug, Clone)]
pub struct ActivityInfo {
    pub last_activity_time: Instant,
    pub activity_count: u64,
}

impl ActivityInfo {
    pub fn new() -> Self {
        Self {
            last_activity_time: Instant::now(),
            activity_count: 1,
        }
    }

    pub fn update(&mut self) {
        self.last_activity_time = Instant::now();
        self.activity_count += 1;
    }

    pub fn is_inactive(&self, timeout: Duration) -> bool {
        self.last_activity_time.elapsed() > timeout
    }
}

impl Default for ActivityInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe activity tracker that can be shared across async tasks
pub struct ActivityTracker {
    pub process_activities: Arc<RwLock<HashMap<String, ActivityInfo>>>,
    pub environment_activities: Arc<RwLock<HashMap<String, ActivityInfo>>>,
}

impl ActivityTracker {
    pub fn new() -> Self {
        Self {
            process_activities: Arc::new(RwLock::new(HashMap::new())),
            environment_activities: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn record_process_activity(&self, process_id: &str) {
        let mut activities = self.process_activities.write().await;
        activities
            .entry(process_id.to_string())
            .and_modify(|activity| activity.update())
            .or_insert_with(ActivityInfo::new);
    }

    pub async fn record_environment_activity(&self, environment_id: &str) {
        let mut activities = self.environment_activities.write().await;
        activities
            .entry(environment_id.to_string())
            .and_modify(|activity| activity.update())
            .or_insert_with(ActivityInfo::new);
    }

    pub async fn check_process_inactivity(&self, process_id: &str, timeout: Duration) -> bool {
        let activities = self.process_activities.read().await;
        activities
            .get(process_id)
            .map(|activity| activity.is_inactive(timeout))
            .unwrap_or(true) // No activity recorded = inactive
    }

    pub async fn check_environment_inactivity(
        &self,
        environment_id: &str,
        timeout: Duration,
    ) -> bool {
        let activities = self.environment_activities.read().await;
        activities
            .get(environment_id)
            .map(|activity| activity.is_inactive(timeout))
            .unwrap_or(true) // No activity recorded = inactive
    }

    pub async fn get_all_inactive_processes(&self, timeout: Duration) -> Vec<String> {
        let activities = self.process_activities.read().await;
        activities
            .iter()
            .filter(|(_, activity)| activity.is_inactive(timeout))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub async fn get_all_inactive_environments(&self, timeout: Duration) -> Vec<String> {
        let activities = self.environment_activities.read().await;
        activities
            .iter()
            .filter(|(_, activity)| activity.is_inactive(timeout))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub async fn cleanup_old_activities(&self, max_age: Duration) -> usize {
        let cutoff_time = Instant::now() - max_age;
        let mut cleaned_count = 0;

        // Clean up old process activities
        {
            let mut activities = self.process_activities.write().await;
            let initial_len = activities.len();
            activities.retain(|_, activity| activity.last_activity_time >= cutoff_time);
            cleaned_count += initial_len - activities.len();
        }

        // Clean up old environment activities
        {
            let mut activities = self.environment_activities.write().await;
            let initial_len = activities.len();
            activities.retain(|_, activity| activity.last_activity_time >= cutoff_time);
            cleaned_count += initial_len - activities.len();
        }

        cleaned_count
    }
}

impl Default for ActivityTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified inactivity and auto-shutdown scheduler
pub struct InactivityScheduler {
    pub activity_tracker: ActivityTracker,
    pub shutdown_timers: Arc<RwLock<HashMap<String, mpsc::Sender<()>>>>,
    pub is_running: Arc<AtomicBool>,
    pub global_config: GlobalConfig,
    monitoring_tx: Option<mpsc::Sender<()>>,
}

impl InactivityScheduler {
    pub fn new(global_config: GlobalConfig) -> Self {
        Self {
            activity_tracker: ActivityTracker::new(),
            shutdown_timers: Arc::new(RwLock::new(HashMap::new())),
            is_running: Arc::new(AtomicBool::new(false)),
            global_config,
            monitoring_tx: None,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        if self.is_running.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.is_running.store(true, Ordering::Relaxed);

        // Start periodic cleanup task
        let (monitoring_tx, mut monitoring_rx) = mpsc::channel::<()>(1);
        let activity_tracker = self.activity_tracker.process_activities.clone();
        let env_tracker = self.activity_tracker.environment_activities.clone();
        let is_running = self.is_running.clone();
        let cleanup_interval =
            Duration::from_secs(self.global_config.process.cleanup_interval as u64);
        let max_activity_age =
            Duration::from_secs(self.global_config.process.max_activity_age as u64);

        tokio::spawn(async move {
            let mut interval_timer = interval(cleanup_interval);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        if !is_running.load(Ordering::Relaxed) {
                            break;
                        }

                        // Cleanup old activities
                        let cutoff_time = Instant::now() - max_activity_age;
                        let mut cleaned_count = 0;

                        // Clean process activities
                        {
                            let mut activities = activity_tracker.write().await;
                            let initial_len = activities.len();
                            activities.retain(|_, activity| activity.last_activity_time >= cutoff_time);
                            cleaned_count += initial_len - activities.len();
                        }

                        // Clean environment activities
                        {
                            let mut activities = env_tracker.write().await;
                            let initial_len = activities.len();
                            activities.retain(|_, activity| activity.last_activity_time >= cutoff_time);
                            cleaned_count += initial_len - activities.len();
                        }

                        if cleaned_count > 0 {
                            println!("Cleaned up {cleaned_count} old activities");
                        }
                    }
                    _ = monitoring_rx.recv() => {
                        // Shutdown signal received
                        break;
                    }
                }
            }
        });

        self.monitoring_tx = Some(monitoring_tx);
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.is_running.store(false, Ordering::Relaxed);

        // Stop monitoring task
        if let Some(tx) = self.monitoring_tx.take() {
            let _ = tx.send(()).await;
        }

        // Cancel all shutdown timers
        let mut timers = self.shutdown_timers.write().await;
        for (_, shutdown_tx) in timers.drain() {
            let _ = shutdown_tx.send(()).await;
        }

        Ok(())
    }

    pub async fn record_process_activity(&self, process_id: &str) {
        self.activity_tracker
            .record_process_activity(process_id)
            .await;
    }

    pub async fn record_environment_activity(&self, environment_id: &str) {
        self.activity_tracker
            .record_environment_activity(environment_id)
            .await;
    }

    pub async fn schedule_process_shutdown(
        &mut self,
        process_id: &str,
        delay: Duration,
    ) -> Result<()> {
        // Cancel existing timer if any
        {
            let mut timers = self.shutdown_timers.write().await;
            if let Some(existing_tx) = timers.remove(process_id) {
                let _ = existing_tx.send(()).await;
            }
        }

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let process_id_clone = process_id.to_string();
        let timers_clone = self.shutdown_timers.clone();

        // Spawn shutdown timer task
        tokio::spawn(async move {
            tokio::select! {
                _ = sleep(delay) => {
                    // Timeout reached - trigger shutdown
                    println!("Process {process_id_clone} scheduled for shutdown due to inactivity");

                    // Remove ourselves from the timers map
                    let mut timers = timers_clone.write().await;
                    timers.remove(&process_id_clone);

                    // TODO: Trigger actual process shutdown through callback/channel
                }
                _ = shutdown_rx.recv() => {
                    // Shutdown cancelled
                    println!("Process {process_id_clone} shutdown cancelled");
                }
            }
        });

        // Store the shutdown channel
        {
            let mut timers = self.shutdown_timers.write().await;
            timers.insert(process_id.to_string(), shutdown_tx);
        }

        Ok(())
    }

    pub async fn schedule_environment_shutdown(
        &mut self,
        environment_id: &str,
        delay: Duration,
    ) -> Result<()> {
        // Cancel existing timer if any
        {
            let mut timers = self.shutdown_timers.write().await;
            if let Some(existing_tx) = timers.remove(environment_id) {
                let _ = existing_tx.send(()).await;
            }
        }

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        let env_id_clone = environment_id.to_string();
        let timers_clone = self.shutdown_timers.clone();

        // Spawn shutdown timer task
        tokio::spawn(async move {
            tokio::select! {
                _ = sleep(delay) => {
                    // Timeout reached - trigger shutdown
                    println!("Environment {env_id_clone} scheduled for shutdown due to inactivity");

                    // Remove ourselves from the timers map
                    let mut timers = timers_clone.write().await;
                    timers.remove(&env_id_clone);

                    // TODO: Trigger actual environment shutdown through callback/channel
                }
                _ = shutdown_rx.recv() => {
                    // Shutdown cancelled
                    println!("Environment {env_id_clone} shutdown cancelled");
                }
            }
        });

        // Store the shutdown channel
        {
            let mut timers = self.shutdown_timers.write().await;
            timers.insert(environment_id.to_string(), shutdown_tx);
        }

        Ok(())
    }

    pub async fn cancel_shutdown(&self, id: &str) {
        let mut timers = self.shutdown_timers.write().await;
        if let Some(shutdown_tx) = timers.remove(id) {
            let _ = shutdown_tx.send(()).await;
        }
    }

    pub async fn check_and_schedule_shutdowns(&mut self) -> Result<()> {
        let process_timeout =
            Duration::from_secs(self.global_config.process.inactivity_timeout as u64);
        let environment_timeout =
            Duration::from_secs(self.global_config.environment.inactivity_timeout as u64);

        // Check for inactive processes
        let inactive_processes = self
            .activity_tracker
            .get_all_inactive_processes(process_timeout)
            .await;
        for process_id in inactive_processes {
            if !self.is_shutdown_scheduled(&process_id).await {
                self.schedule_process_shutdown(&process_id, Duration::from_secs(0))
                    .await?;
            }
        }

        // Check for inactive environments
        let inactive_environments = self
            .activity_tracker
            .get_all_inactive_environments(environment_timeout)
            .await;
        for env_id in inactive_environments {
            if !self.is_shutdown_scheduled(&env_id).await {
                self.schedule_environment_shutdown(&env_id, Duration::from_secs(0))
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn is_shutdown_scheduled(&self, id: &str) -> bool {
        let timers = self.shutdown_timers.read().await;
        timers.contains_key(id)
    }

    pub async fn get_scheduled_shutdown_count(&self) -> usize {
        let timers = self.shutdown_timers.read().await;
        timers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GlobalConfig;
    use crate::process::Process;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::Instant;

    #[tokio::test]
    async fn test_activity_tracker_creation() {
        let tracker = ActivityTracker::new();
        assert!(tracker.process_activities.read().await.is_empty());
        assert!(tracker.environment_activities.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_record_process_activity() {
        let tracker = ActivityTracker::new();
        let process_id = "test-process-1".to_string();

        tracker.record_process_activity(&process_id).await;

        let activities = tracker.process_activities.read().await;
        assert!(activities.contains_key(&process_id));
        let activity = activities.get(&process_id).unwrap();
        assert!(activity.last_activity_time.elapsed().as_secs() < 1);
    }

    #[tokio::test]
    async fn test_record_environment_activity() {
        let tracker = ActivityTracker::new();
        let env_id = "test-env-1".to_string();

        tracker.record_environment_activity(&env_id).await;

        let activities = tracker.environment_activities.read().await;
        assert!(activities.contains_key(&env_id));
    }

    #[tokio::test]
    async fn test_check_process_inactivity_within_timeout() {
        let tracker = ActivityTracker::new();
        let process_id = "test-process-1".to_string();

        tracker.record_process_activity(&process_id).await;

        let is_inactive = tracker
            .check_process_inactivity(&process_id, Duration::from_secs(10))
            .await;
        assert!(!is_inactive);
    }

    #[tokio::test]
    async fn test_check_process_inactivity_beyond_timeout() {
        let tracker = ActivityTracker::new();
        let process_id = "test-process-1".to_string();

        // Manually insert old activity
        {
            let mut activities = tracker.process_activities.write().await;
            activities.insert(
                process_id.clone(),
                ActivityInfo {
                    last_activity_time: Instant::now() - Duration::from_secs(20),
                    activity_count: 1,
                },
            );
        }

        let is_inactive = tracker
            .check_process_inactivity(&process_id, Duration::from_secs(10))
            .await;
        assert!(is_inactive);
    }

    #[tokio::test]
    async fn test_get_all_inactive_processes() {
        let tracker = ActivityTracker::new();

        // Add active and inactive processes
        tracker.record_process_activity("active-process").await;

        {
            let mut activities = tracker.process_activities.write().await;
            activities.insert(
                "inactive-process".to_string(),
                ActivityInfo {
                    last_activity_time: Instant::now() - Duration::from_secs(20),
                    activity_count: 1,
                },
            );
        }

        let inactive = tracker
            .get_all_inactive_processes(Duration::from_secs(10))
            .await;
        assert_eq!(inactive.len(), 1);
        assert!(inactive.contains(&"inactive-process".to_string()));
    }

    #[tokio::test]
    async fn test_cleanup_old_activities() {
        let tracker = ActivityTracker::new();

        // Add recent and old activities
        tracker.record_process_activity("recent-process").await;

        {
            let mut activities = tracker.process_activities.write().await;
            activities.insert(
                "old-process".to_string(),
                ActivityInfo {
                    last_activity_time: Instant::now() - Duration::from_secs(7200), // 2 hours old
                    activity_count: 1,
                },
            );
        }

        let cleaned = tracker
            .cleanup_old_activities(Duration::from_secs(3600))
            .await; // 1 hour cutoff
        assert_eq!(cleaned, 1);

        let activities = tracker.process_activities.read().await;
        assert!(activities.contains_key("recent-process"));
        assert!(!activities.contains_key("old-process"));
    }

    #[tokio::test]
    async fn test_inactivity_scheduler_creation() {
        let global_config = GlobalConfig::default();
        let scheduler = InactivityScheduler::new(global_config);

        assert!(
            !scheduler
                .is_running
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    #[tokio::test]
    async fn test_inactivity_scheduler_start_stop() {
        let global_config = GlobalConfig::default();
        let mut scheduler = InactivityScheduler::new(global_config);

        scheduler.start().await.unwrap();
        assert!(
            scheduler
                .is_running
                .load(std::sync::atomic::Ordering::Relaxed)
        );

        scheduler.stop().await.unwrap();
        assert!(
            !scheduler
                .is_running
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    #[tokio::test]
    async fn test_schedule_process_shutdown() {
        let global_config = GlobalConfig::default();
        let mut scheduler = InactivityScheduler::new(global_config);

        let temp_dir = TempDir::new().unwrap();
        let process = Process::new(
            "test-process".to_string(),
            "echo test".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
            "env-1".to_string(),
        );

        scheduler
            .schedule_process_shutdown(&process.id, Duration::from_millis(100))
            .await
            .unwrap();

        let timers = scheduler.shutdown_timers.read().await;
        assert!(timers.contains_key(&process.id));
    }

    #[tokio::test]
    async fn test_cancel_process_shutdown() {
        let global_config = GlobalConfig::default();
        let mut scheduler = InactivityScheduler::new(global_config);

        let temp_dir = TempDir::new().unwrap();
        let process = Process::new(
            "test-process".to_string(),
            "echo test".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
            "env-1".to_string(),
        );

        scheduler
            .schedule_process_shutdown(&process.id, Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(scheduler.shutdown_timers.read().await.len(), 1);

        scheduler.cancel_shutdown(&process.id).await;
        assert_eq!(scheduler.shutdown_timers.read().await.len(), 0);
    }
}
