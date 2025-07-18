use crate::error::{Result, RunceptError};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Request to watch or unwatch a file
#[derive(Debug, Clone)]
pub enum WatchRequest {
    /// Start watching a file
    Watch {
        file_path: PathBuf,
        context: String, // Can be environment ID or any other context
    },
    /// Stop watching a file
    Unwatch { file_path: PathBuf },
}

/// Configuration file change event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeEvent {
    /// Path to the file that changed
    pub file_path: PathBuf,
    /// Context associated with the file (environment ID, etc.)
    pub context: String,
    /// Type of change (modified, created, deleted)
    pub change_type: ConfigChangeType,
}

/// Type of configuration change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigChangeType {
    Modified,
    Created,
    Deleted,
}

/// File watcher for automatic configuration reloading
pub struct ConfigWatcher {
    /// Maps watched file paths to their context (environment ID, etc.)
    watched_files: HashMap<PathBuf, String>,
    /// File system watcher instance
    watcher: Option<RecommendedWatcher>,
}

/// ConfigWatcher channels for communication
pub struct ConfigWatcherChannels {
    /// Send watch requests to ConfigWatcher
    pub watch_sender: mpsc::Sender<WatchRequest>,
    /// Receive file change events from ConfigWatcher
    pub event_receiver: mpsc::Receiver<ConfigChangeEvent>,
}

impl ConfigWatcher {
    /// Create a new config watcher and return channels for communication
    pub fn new() -> ConfigWatcherChannels {
        let (watch_sender, watch_receiver) = mpsc::channel::<WatchRequest>(100);
        let (event_sender, event_receiver) = mpsc::channel::<ConfigChangeEvent>(100);

        let watcher = Self {
            watched_files: HashMap::new(),
            watcher: None,
        };

        // Spawn the watcher task
        tokio::spawn(async move {
            if let Err(e) = watcher.run(watch_receiver, event_sender).await {
                error!("Config watcher error: {}", e);
            }
        });

        ConfigWatcherChannels {
            watch_sender,
            event_receiver,
        }
    }

    /// Start the config watcher event loop
    pub async fn run(
        mut self,
        mut watch_receiver: mpsc::Receiver<WatchRequest>,
        event_sender: mpsc::Sender<ConfigChangeEvent>,
    ) -> Result<()> {
        // Create file system watcher
        let (fs_tx, mut fs_rx) = mpsc::channel::<std::result::Result<Event, notify::Error>>(100);

        let watcher = RecommendedWatcher::new(
            move |res| {
                if let Err(e) = fs_tx.blocking_send(res) {
                    error!("Failed to send file system event: {}", e);
                }
            },
            Config::default(),
        )
        .map_err(|e| RunceptError::ConfigError(format!("Failed to create file watcher: {e}")))?;

        self.watcher = Some(watcher);
        info!("Config watcher started");

        // Main event loop
        loop {
            tokio::select! {
                // Handle watch requests
                watch_request = watch_receiver.recv() => {
                    match watch_request {
                        Some(WatchRequest::Watch { file_path, context }) => {
                            if let Err(e) = self.watch_file(&file_path, &context) {
                                error!("Failed to watch file {}: {}", file_path.display(), e);
                            }
                        }
                        Some(WatchRequest::Unwatch { file_path }) => {
                            if let Err(e) = self.unwatch_file(&file_path) {
                                error!("Failed to unwatch file {}: {}", file_path.display(), e);
                            }
                        }
                        None => {
                            info!("Watch request channel closed, stopping config watcher");
                            break;
                        }
                    }
                }

                // Handle file system events
                fs_event = fs_rx.recv() => {
                    match fs_event {
                        Some(Ok(event)) => {
                            if let Err(e) = self.handle_file_event(event, &event_sender).await {
                                error!("Error handling file event: {}", e);
                            }
                        }
                        Some(Err(e)) => {
                            error!("File watcher error: {}", e);
                        }
                        None => {
                            error!("File system event channel closed");
                            break;
                        }
                    }
                }
            }
        }

        info!("Config watcher stopped");
        Ok(())
    }

    /// Watch a file
    fn watch_file(&mut self, file_path: &Path, context: &str) -> Result<()> {
        if let Some(ref mut watcher) = self.watcher {
            // Watch the parent directory to catch file modifications, creations, and deletions
            let watch_path = file_path.parent().unwrap_or(file_path);

            watcher
                .watch(watch_path, RecursiveMode::NonRecursive)
                .map_err(|e| {
                    RunceptError::ConfigError(format!(
                        "Failed to watch config directory {}: {}",
                        watch_path.display(),
                        e
                    ))
                })?;

            self.watched_files
                .insert(file_path.to_path_buf(), context.to_string());

            info!(
                "Now watching config file {} with context {}",
                file_path.display(),
                context
            );
        }
        Ok(())
    }

    /// Unwatch a file
    fn unwatch_file(&mut self, file_path: &Path) -> Result<()> {
        if let Some(ref mut watcher) = self.watcher {
            let watch_path = file_path.parent().unwrap_or(file_path);

            // Only unwatch if no other files in the same directory are being watched
            let same_dir_count = self
                .watched_files
                .keys()
                .filter(|path| path.parent() == Some(watch_path))
                .count();

            if same_dir_count <= 1 {
                watcher.unwatch(watch_path).map_err(|e| {
                    RunceptError::ConfigError(format!(
                        "Failed to unwatch config directory {}: {}",
                        watch_path.display(),
                        e
                    ))
                })?;
            }

            self.watched_files.remove(file_path);
            info!("Stopped watching config file {}", file_path.display());
        }
        Ok(())
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

            self.watched_files
                .insert(config_path.to_path_buf(), env_id.to_string());

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
            let same_dir_count = self
                .watched_files
                .keys()
                .filter(|path| path.parent() == Some(watch_path))
                .count();

            if same_dir_count <= 1 {
                watcher.unwatch(watch_path).map_err(|e| {
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

    /// Handle a file system event
    async fn handle_file_event(
        &self,
        event: Event,
        event_sender: &mpsc::Sender<ConfigChangeEvent>,
    ) -> Result<()> {
        debug!("Received file system event: {:?}", event);

        // Check if the event affects any of our watched files
        for path in &event.paths {
            // Check if this is a .runcept.toml file or affects one
            if path.file_name().and_then(|n| n.to_str()) == Some(".runcept.toml") {
                if let Some(context) = self.watched_files.get(path) {
                    let change_type = match event.kind {
                        EventKind::Modify(_) => ConfigChangeType::Modified,
                        EventKind::Create(_) => ConfigChangeType::Created,
                        EventKind::Remove(_) => ConfigChangeType::Deleted,
                        _ => continue, // Ignore other event types
                    };

                    let config_event = ConfigChangeEvent {
                        file_path: path.clone(),
                        context: context.clone(),
                        change_type,
                    };

                    info!(
                        "Config file {} {:?}, emitting event for context {}",
                        path.display(),
                        config_event.change_type,
                        context
                    );

                    if let Err(e) = event_sender.send(config_event).await {
                        error!("Failed to send config change event: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the list of currently watched files
    pub fn get_watched_files(&self) -> Vec<(PathBuf, String)> {
        self.watched_files
            .iter()
            .map(|(path, env_id)| (path.clone(), env_id.clone()))
            .collect()
    }
}
