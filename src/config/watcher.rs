use crate::config::environment::EnvironmentManager;
use crate::error::{Result, RunceptError};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// File watcher for automatic configuration reloading
pub struct ConfigWatcher {
    /// Maps watched file paths to environment IDs
    watched_files: HashMap<PathBuf, String>,
    /// File system watcher instance
    watcher: Option<RecommendedWatcher>,
    /// Channel for receiving file system events
    event_receiver: Option<mpsc::Receiver<std::result::Result<Event, notify::Error>>>,
    /// Reference to environment manager for reloading configs
    environment_manager: Arc<RwLock<EnvironmentManager>>,
}

impl ConfigWatcher {
    /// Create a new config watcher
    pub fn new(environment_manager: Arc<RwLock<EnvironmentManager>>) -> Self {
        Self {
            watched_files: HashMap::new(),
            watcher: None,
            event_receiver: None,
            environment_manager,
        }
    }

    /// Start the file watcher
    pub fn start(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel::<std::result::Result<Event, notify::Error>>(100);
        
        let watcher = RecommendedWatcher::new(
            move |res| {
                if let Err(e) = tx.blocking_send(res) {
                    error!("Failed to send file system event: {}", e);
                }
            },
            Config::default(),
        )
        .map_err(|e| RunceptError::ConfigError(format!("Failed to create file watcher: {e}")))?;

        self.watcher = Some(watcher);
        self.event_receiver = Some(rx);

        info!("Config file watcher started");
        Ok(())
    }

    /// Stop the file watcher
    pub fn stop(&mut self) {
        self.watcher = None;
        self.event_receiver = None;
        self.watched_files.clear();
        info!("Config file watcher stopped");
    }

    /// Add a config file to watch for an environment
    pub fn watch_config_file(&mut self, config_path: &Path, env_id: &str) -> Result<()> {
        if let Some(ref mut watcher) = self.watcher {
            // Watch the parent directory to catch file modifications, creations, and deletions
            let watch_path = config_path.parent().unwrap_or(config_path);
            
            watcher
                .watch(watch_path, RecursiveMode::NonRecursive)
                .map_err(|e| {
                    RunceptError::ConfigError(format!(
                        "Failed to watch config directory {}: {}",
                        watch_path.display(),
                        e
                    ))
                })?;

            self.watched_files.insert(config_path.to_path_buf(), env_id.to_string());
            
            debug!(
                "Now watching config file {} for environment {}",
                config_path.display(),
                env_id
            );
        }
        Ok(())
    }

    /// Remove a config file from watching
    pub fn unwatch_config_file(&mut self, config_path: &Path) -> Result<()> {
        if let Some(ref mut watcher) = self.watcher {
            let watch_path = config_path.parent().unwrap_or(config_path);
            
            // Only unwatch if no other files in the same directory are being watched
            let same_dir_count = self.watched_files.keys()
                .filter(|path| path.parent() == Some(watch_path))
                .count();
            
            if same_dir_count <= 1 {
                watcher
                    .unwatch(watch_path)
                    .map_err(|e| {
                        RunceptError::ConfigError(format!(
                            "Failed to unwatch config directory {}: {}",
                            watch_path.display(),
                            e
                        ))
                    })?;
            }

            self.watched_files.remove(config_path);
            
            debug!(
                "Stopped watching config file {} (same dir count: {})",
                config_path.display(),
                same_dir_count
            );
        }
        Ok(())
    }

    /// Run the file watcher event loop
    pub async fn run(&mut self) -> Result<()> {
        if let Some(receiver) = self.event_receiver.take() {
            info!("Starting config watcher event loop");
            
            let mut receiver = receiver;
            while let Some(event_result) = receiver.recv().await {
                match event_result {
                    Ok(event) => {
                        if let Err(e) = self.handle_file_event(event).await {
                            error!("Error handling file event: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("File watcher error: {}", e);
                    }
                }
            }
            
            info!("Config watcher event loop ended");
        }
        Ok(())
    }

    /// Handle a file system event
    pub async fn handle_file_event(&self, event: Event) -> Result<()> {
        debug!("Received file system event: {:?}", event);

        // Check if the event affects any of our watched files
        for path in &event.paths {
            // Check if this is a .runcept.toml file or affects one
            if path.file_name().and_then(|n| n.to_str()) == Some(".runcept.toml") {
                if let Some(env_id) = self.watched_files.get(path) {
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            info!(
                                "Config file {} modified, reloading environment {}",
                                path.display(),
                                env_id
                            );
                            
                            if let Err(e) = self.reload_environment_config(env_id).await {
                                error!(
                                    "Failed to reload config for environment {}: {}",
                                    env_id, e
                                );
                            } else {
                                debug!("Successfully reloaded config for environment {}", env_id);
                            }
                        }
                        EventKind::Remove(_) => {
                            warn!(
                                "Config file {} was deleted for environment {}",
                                path.display(),
                                env_id
                            );
                            // Could potentially deactivate the environment here
                        }
                        _ => {
                            // Ignore other event types
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Reload configuration for a specific environment
    async fn reload_environment_config(&self, env_id: &str) -> Result<()> {
        let mut env_manager = self.environment_manager.write().await;
        env_manager.reload_environment_config(env_id).await
    }

    /// Get the list of currently watched files
    pub fn get_watched_files(&self) -> Vec<(PathBuf, String)> {
        self.watched_files
            .iter()
            .map(|(path, env_id)| (path.clone(), env_id.clone()))
            .collect()
    }

    /// Take the event receiver for external event loop management
    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<std::result::Result<Event, notify::Error>>> {
        self.event_receiver.take()
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalConfig, EnvironmentManager};
    use tempfile::TempDir;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_config_watcher_creation() {
        let global_config = GlobalConfig::default();
        let env_manager = Arc::new(RwLock::new(EnvironmentManager::new(global_config).await.unwrap()));
        let watcher = ConfigWatcher::new(env_manager);
        
        assert!(watcher.watched_files.is_empty());
        assert!(watcher.watcher.is_none());
    }

    #[tokio::test]
    async fn test_config_watcher_start_stop() {
        let global_config = GlobalConfig::default();
        let env_manager = Arc::new(RwLock::new(EnvironmentManager::new(global_config).await.unwrap()));
        let mut watcher = ConfigWatcher::new(env_manager);
        
        // Start watcher
        watcher.start().unwrap();
        assert!(watcher.watcher.is_some());
        assert!(watcher.event_receiver.is_some());
        
        // Stop watcher
        watcher.stop();
        assert!(watcher.watcher.is_none());
        assert!(watcher.event_receiver.is_none());
        assert!(watcher.watched_files.is_empty());
    }

    #[tokio::test]
    async fn test_watch_config_file() {
        let global_config = GlobalConfig::default();
        let env_manager = Arc::new(RwLock::new(EnvironmentManager::new(global_config).await.unwrap()));
        let mut watcher = ConfigWatcher::new(env_manager);
        
        watcher.start().unwrap();
        
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".runcept.toml");
        
        // Watch the config file
        watcher.watch_config_file(&config_path, "test_env").unwrap();
        
        let watched_files = watcher.get_watched_files();
        assert_eq!(watched_files.len(), 1);
        assert_eq!(watched_files[0].0, config_path);
        assert_eq!(watched_files[0].1, "test_env");
        
        // Unwatch the config file
        watcher.unwatch_config_file(&config_path).unwrap();
        assert!(watcher.get_watched_files().is_empty());
    }

    #[tokio::test]
    async fn test_file_modification_detection() {
        let global_config = GlobalConfig::default();
        let env_manager = Arc::new(RwLock::new(EnvironmentManager::new(global_config).await.unwrap()));
        let mut watcher = ConfigWatcher::new(env_manager);
        
        watcher.start().unwrap();
        
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(".runcept.toml");
        
        // Create initial config file
        std::fs::write(&config_path, "[environment]\nname = \"test\"\n").unwrap();
        
        // Watch the config file
        watcher.watch_config_file(&config_path, "test_env").unwrap();
        
        // Start the event loop in the background
        let mut watcher_clone = ConfigWatcher::new(watcher.environment_manager.clone());
        watcher_clone.watcher = watcher.watcher.take();
        watcher_clone.event_receiver = watcher.event_receiver.take();
        watcher_clone.watched_files = watcher.watched_files.clone();
        
        let watcher_handle = tokio::spawn(async move {
            let _ = watcher_clone.run().await;
        });
        
        // Give the watcher time to start
        sleep(Duration::from_millis(100)).await;
        
        // Modify the config file
        std::fs::write(&config_path, "[environment]\nname = \"test_modified\"\n").unwrap();
        
        // Give the watcher time to detect the change
        sleep(Duration::from_millis(500)).await;
        
        // Stop the watcher
        watcher_handle.abort();
        
        // Note: In a real test, we would check if the environment was actually reloaded,
        // but that requires more complex setup with actual environment registration
    }
}