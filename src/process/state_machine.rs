use crate::error::{Result, RunceptError};
use crate::process::traits::{ActivityEvent, ActivityType};
use crate::process::{Process, ProcessStatus};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;
use tracing::{debug, info};

/// Events that can trigger process state transitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessEvent {
    /// Start command initiated
    StartRequested { requested_by: String, force: bool },
    /// Process spawned successfully
    ProcessSpawned { pid: u32 },
    /// Process started successfully and is now running
    StartupCompleted,
    /// Stop command initiated
    StopRequested { requested_by: String, force: bool },
    /// Process gracefully stopped
    ProcessStopped { exit_code: Option<i32> },
    /// Process crashed unexpectedly
    ProcessCrashed {
        exit_code: Option<i32>,
        signal: Option<String>,
    },
    /// Process failed to start
    StartupFailed { error: String },
    /// Health check failed
    HealthCheckFailed { error: String },
    /// Process became unresponsive
    ProcessUnresponsive,
    /// Restart requested (usually after crash/failure)
    RestartRequested {
        requested_by: String,
        attempt_count: u32,
    },
}

impl fmt::Display for ProcessEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessEvent::StartRequested {
                requested_by,
                force,
            } => {
                write!(f, "Start requested by {} (force: {})", requested_by, force)
            }
            ProcessEvent::ProcessSpawned { pid } => {
                write!(f, "Process spawned with PID {}", pid)
            }
            ProcessEvent::StartupCompleted => {
                write!(f, "Startup completed successfully")
            }
            ProcessEvent::StopRequested {
                requested_by,
                force,
            } => {
                write!(f, "Stop requested by {} (force: {})", requested_by, force)
            }
            ProcessEvent::ProcessStopped { exit_code } => match exit_code {
                Some(code) => write!(f, "Process stopped with exit code {}", code),
                None => write!(f, "Process stopped"),
            },
            ProcessEvent::ProcessCrashed { exit_code, signal } => match (exit_code, signal) {
                (Some(code), Some(sig)) => write!(
                    f,
                    "Process crashed with exit code {} (signal: {})",
                    code, sig
                ),
                (Some(code), None) => write!(f, "Process crashed with exit code {}", code),
                (None, Some(sig)) => write!(f, "Process crashed with signal {}", sig),
                (None, None) => write!(f, "Process crashed"),
            },
            ProcessEvent::StartupFailed { error } => {
                write!(f, "Startup failed: {}", error)
            }
            ProcessEvent::HealthCheckFailed { error } => {
                write!(f, "Health check failed: {}", error)
            }
            ProcessEvent::ProcessUnresponsive => {
                write!(f, "Process became unresponsive")
            }
            ProcessEvent::RestartRequested {
                requested_by,
                attempt_count,
            } => {
                write!(
                    f,
                    "Restart requested by {} (attempt {})",
                    requested_by, attempt_count
                )
            }
        }
    }
}

/// Result of a state transition
#[derive(Debug, Clone)]
pub struct TransitionResult {
    pub success: bool,
    pub old_status: ProcessStatus,
    pub new_status: ProcessStatus,
    pub activity_events: Vec<ActivityEvent>,
    pub side_effects: Vec<SideEffect>,
}

/// Side effects that should be executed as a result of state transitions
#[derive(Debug, Clone)]
pub enum SideEffect {
    /// Update PID in database and process
    SetPid(u32),
    /// Clear PID from database and process
    ClearPid,
    /// Update process activity timestamp
    UpdateActivity,
    /// Schedule restart after delay
    ScheduleRestart { delay_seconds: u64 },
    /// Cancel pending restart
    CancelRestart,
    /// Send notification to monitoring systems
    SendNotification { message: String },
    /// Update process configuration
    UpdateConfig { field: String, value: String },
}

/// Formal process state machine
pub struct ProcessStateMachine {
    /// Current process state
    process: Process,
    /// Enable activity logging
    log_activities: bool,
}

impl ProcessStateMachine {
    pub fn new(process: Process) -> Self {
        Self {
            process,
            log_activities: true,
        }
    }

    pub fn with_activity_logging(mut self, enabled: bool) -> Self {
        self.log_activities = enabled;
        self
    }

    pub fn process(&self) -> &Process {
        &self.process
    }

    pub fn into_process(self) -> Process {
        self.process
    }

    /// Apply an event to the state machine and return the transition result
    pub fn apply_event(&mut self, event: ProcessEvent) -> Result<TransitionResult> {
        let old_status = self.process.status.clone();

        // Determine the target state based on current state and event
        let target_status = self.determine_target_state(&event)?;

        // Validate the transition is allowed
        if !self.process.can_transition_to(&target_status) {
            return Err(RunceptError::ProcessError(format!(
                "Invalid state transition from {:?} to {:?} for event: {}",
                old_status, target_status, event
            )));
        }

        // Perform the transition
        self.process.transition_to(target_status.clone());

        // Generate activity events
        let mut activity_events = Vec::new();
        if self.log_activities {
            activity_events.push(self.create_activity_event(&event, &old_status, &target_status));
        }

        // Determine side effects
        let side_effects = self.determine_side_effects(&event, &old_status, &target_status);

        // Apply immediate side effects to the process
        for effect in &side_effects {
            self.apply_side_effect(effect)?;
        }

        info!(
            "Process '{}' transitioned from {:?} to {:?} due to event: {}",
            self.process.name, old_status, target_status, event
        );

        Ok(TransitionResult {
            success: true,
            old_status,
            new_status: target_status,
            activity_events,
            side_effects,
        })
    }

    /// Determine target state based on current state and event
    fn determine_target_state(&self, event: &ProcessEvent) -> Result<ProcessStatus> {
        use ProcessEvent::*;
        use ProcessStatus::*;

        let target = match (&self.process.status, event) {
            // Start transitions
            (Stopped, StartRequested { .. }) => Starting,
            (Failed, StartRequested { .. }) => Starting,
            (Crashed, StartRequested { .. }) => Starting,
            (Stopped, RestartRequested { .. }) => Starting,
            (Failed, RestartRequested { .. }) => Starting,
            (Crashed, RestartRequested { .. }) => Starting,

            // Starting transitions
            (Starting, ProcessSpawned { .. }) => Starting, // Still starting until health check passes
            (Starting, StartupCompleted) => Running,
            (Starting, StartupFailed { .. }) => Failed,
            (Starting, ProcessCrashed { .. }) => Crashed,

            // Running transitions
            (Running, StopRequested { .. }) => Stopping,
            (Running, ProcessCrashed { .. }) => Crashed,
            (Running, HealthCheckFailed { .. }) => Failed,
            (Running, ProcessUnresponsive) => Failed,

            // Stopping transitions
            (Stopping, ProcessStopped { .. }) => Stopped,
            (Stopping, ProcessCrashed { .. }) => Crashed, // Process crashed while stopping

            // Invalid transitions
            _ => {
                return Err(RunceptError::ProcessError(format!(
                    "No valid state transition from {:?} for event: {}",
                    self.process.status, event
                )));
            }
        };

        Ok(target)
    }

    /// Determine what side effects should occur for this transition
    fn determine_side_effects(
        &self,
        event: &ProcessEvent,
        _old_status: &ProcessStatus,
        new_status: &ProcessStatus,
    ) -> Vec<SideEffect> {
        let mut effects = Vec::new();

        // Always update activity on state change
        effects.push(SideEffect::UpdateActivity);

        match (event, new_status) {
            // Set PID when process spawns
            (ProcessEvent::ProcessSpawned { pid }, _) => {
                effects.push(SideEffect::SetPid(*pid));
            }

            // Schedule restart and send notifications for crash events
            (ProcessEvent::ProcessCrashed { .. }, ProcessStatus::Crashed) => {
                effects.push(SideEffect::ClearPid);
                effects.push(SideEffect::SendNotification {
                    message: format!("Process '{}' crashed", self.process.name),
                });
                if self.process.auto_restart {
                    effects.push(SideEffect::ScheduleRestart { delay_seconds: 5 });
                }
            }

            // Schedule restart and send notifications for startup failures
            (ProcessEvent::StartupFailed { error }, ProcessStatus::Failed) => {
                effects.push(SideEffect::ClearPid);
                effects.push(SideEffect::SendNotification {
                    message: format!("Process '{}' failed to start: {}", self.process.name, error),
                });
                if self.process.auto_restart {
                    effects.push(SideEffect::ScheduleRestart { delay_seconds: 10 });
                }
            }

            // Clear PID when process stops or enters terminal states (catch-all)
            (_, ProcessStatus::Stopped)
            | (_, ProcessStatus::Crashed)
            | (_, ProcessStatus::Failed) => {
                effects.push(SideEffect::ClearPid);
            }

            _ => {}
        }

        effects
    }

    /// Apply immediate side effects to the process
    fn apply_side_effect(&mut self, effect: &SideEffect) -> Result<()> {
        match effect {
            SideEffect::SetPid(pid) => {
                self.process.set_pid(*pid);
            }
            SideEffect::ClearPid => {
                self.process.clear_pid();
            }
            SideEffect::UpdateActivity => {
                self.process.update_activity();
            }
            // These side effects are handled by external systems
            SideEffect::ScheduleRestart { .. } => {
                debug!("Restart scheduled for process '{}'", self.process.name);
            }
            SideEffect::CancelRestart => {
                debug!("Restart cancelled for process '{}'", self.process.name);
            }
            SideEffect::SendNotification { message } => {
                info!("Notification: {}", message);
            }
            SideEffect::UpdateConfig { field, value } => {
                debug!("Config update requested: {} = {}", field, value);
            }
        }
        Ok(())
    }

    /// Create an activity event for logging
    fn create_activity_event(
        &self,
        event: &ProcessEvent,
        old_status: &ProcessStatus,
        new_status: &ProcessStatus,
    ) -> ActivityEvent {
        let activity_type = match event {
            ProcessEvent::StartRequested { .. } | ProcessEvent::RestartRequested { .. } => {
                if matches!(new_status, ProcessStatus::Starting) {
                    ActivityType::ProcessStarted
                } else {
                    ActivityType::StatusChanged {
                        from: old_status.clone(),
                        to: new_status.clone(),
                    }
                }
            }
            ProcessEvent::ProcessSpawned { .. } => ActivityType::StatusChanged {
                from: old_status.clone(),
                to: new_status.clone(),
            },
            ProcessEvent::StartupCompleted => ActivityType::StatusChanged {
                from: old_status.clone(),
                to: new_status.clone(),
            },
            ProcessEvent::StopRequested { .. } => ActivityType::StatusChanged {
                from: old_status.clone(),
                to: new_status.clone(),
            },
            ProcessEvent::ProcessStopped { .. } => ActivityType::ProcessStopped,
            ProcessEvent::ProcessCrashed { .. } => ActivityType::ProcessCrashed,
            ProcessEvent::StartupFailed { .. } => ActivityType::StatusChanged {
                from: old_status.clone(),
                to: new_status.clone(),
            },
            ProcessEvent::HealthCheckFailed { .. } => ActivityType::HealthCheckFailed,
            ProcessEvent::ProcessUnresponsive => ActivityType::StatusChanged {
                from: old_status.clone(),
                to: new_status.clone(),
            },
        };

        ActivityEvent {
            process_id: Some(self.process.id.clone()),
            environment_id: Some(self.process.environment_id.clone()),
            activity_type,
            timestamp: Utc::now(),
            details: serde_json::json!({
                "event": format!("{}", event),
                "old_status": format!("{}", old_status),
                "new_status": format!("{}", new_status),
                "process_name": self.process.name,
            }),
        }
    }

    /// Check if an event can be applied (without actually applying it)
    pub fn can_apply_event(&self, event: &ProcessEvent) -> bool {
        match self.determine_target_state(event) {
            Ok(target_status) => self.process.can_transition_to(&target_status),
            Err(_) => false,
        }
    }

    /// Get all valid events for the current state
    pub fn valid_events(&self) -> Vec<ProcessEvent> {
        let mut events = Vec::new();

        match self.process.status {
            ProcessStatus::Stopped => {
                events.push(ProcessEvent::StartRequested {
                    requested_by: "system".to_string(),
                    force: false,
                });
            }
            ProcessStatus::Starting => {
                events.push(ProcessEvent::ProcessSpawned { pid: 1234 }); // Example PID
                events.push(ProcessEvent::StartupCompleted);
                events.push(ProcessEvent::StartupFailed {
                    error: "example error".to_string(),
                });
                events.push(ProcessEvent::ProcessCrashed {
                    exit_code: Some(1),
                    signal: None,
                });
            }
            ProcessStatus::Running => {
                events.push(ProcessEvent::StopRequested {
                    requested_by: "system".to_string(),
                    force: false,
                });
                events.push(ProcessEvent::ProcessCrashed {
                    exit_code: Some(1),
                    signal: None,
                });
                events.push(ProcessEvent::HealthCheckFailed {
                    error: "example error".to_string(),
                });
                events.push(ProcessEvent::ProcessUnresponsive);
            }
            ProcessStatus::Stopping => {
                events.push(ProcessEvent::ProcessStopped { exit_code: Some(0) });
                events.push(ProcessEvent::ProcessCrashed {
                    exit_code: Some(1),
                    signal: None,
                });
            }
            ProcessStatus::Failed | ProcessStatus::Crashed => {
                events.push(ProcessEvent::StartRequested {
                    requested_by: "system".to_string(),
                    force: false,
                });
                events.push(ProcessEvent::RestartRequested {
                    requested_by: "system".to_string(),
                    attempt_count: 1,
                });
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Process;
    use std::collections::HashMap;

    fn create_test_process() -> Process {
        Process {
            id: "env1:test-process".to_string(),
            name: "test-process".to_string(),
            command: "echo hello".to_string(),
            working_dir: "/tmp".to_string(),
            environment_id: "env1".to_string(),
            pid: None,
            status: ProcessStatus::Stopped,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_activity: None,
            auto_restart: false,
            health_check_url: None,
            health_check_interval: None,
            depends_on: Vec::new(),
            env_vars: HashMap::new(),
        }
    }

    #[test]
    fn test_state_machine_start_process() {
        let process = create_test_process();
        let mut sm = ProcessStateMachine::new(process);

        // Start process
        let start_event = ProcessEvent::StartRequested {
            requested_by: "user".to_string(),
            force: false,
        };

        let result = sm.apply_event(start_event).unwrap();
        assert!(result.success);
        assert_eq!(result.old_status, ProcessStatus::Stopped);
        assert_eq!(result.new_status, ProcessStatus::Starting);
        assert_eq!(sm.process().status, ProcessStatus::Starting);
        assert!(!result.activity_events.is_empty());
    }

    #[test]
    fn test_state_machine_process_spawned() {
        let process = create_test_process();
        let mut sm = ProcessStateMachine::new(process);

        // First start the process
        sm.apply_event(ProcessEvent::StartRequested {
            requested_by: "user".to_string(),
            force: false,
        })
        .unwrap();

        // Then indicate process spawned
        let spawn_event = ProcessEvent::ProcessSpawned { pid: 1234 };
        let result = sm.apply_event(spawn_event).unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, ProcessStatus::Starting);
        assert_eq!(sm.process().pid, Some(1234));

        // Check side effects
        assert!(
            result
                .side_effects
                .iter()
                .any(|e| matches!(e, SideEffect::SetPid(1234)))
        );
    }

    #[test]
    fn test_state_machine_startup_completed() {
        let process = create_test_process();
        let mut sm = ProcessStateMachine::new(process);

        // Start process and spawn
        sm.apply_event(ProcessEvent::StartRequested {
            requested_by: "user".to_string(),
            force: false,
        })
        .unwrap();

        sm.apply_event(ProcessEvent::ProcessSpawned { pid: 1234 })
            .unwrap();

        // Complete startup
        let complete_event = ProcessEvent::StartupCompleted;
        let result = sm.apply_event(complete_event).unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, ProcessStatus::Running);
        assert_eq!(sm.process().status, ProcessStatus::Running);
    }

    #[test]
    fn test_state_machine_process_crash() {
        let process = create_test_process();
        let mut sm = ProcessStateMachine::new(process);

        // Start and run process
        sm.apply_event(ProcessEvent::StartRequested {
            requested_by: "user".to_string(),
            force: false,
        })
        .unwrap();
        sm.apply_event(ProcessEvent::ProcessSpawned { pid: 1234 })
            .unwrap();
        sm.apply_event(ProcessEvent::StartupCompleted).unwrap();

        // Crash the process
        let crash_event = ProcessEvent::ProcessCrashed {
            exit_code: Some(1),
            signal: Some("SIGSEGV".to_string()),
        };
        let result = sm.apply_event(crash_event).unwrap();

        assert!(result.success);
        assert_eq!(result.new_status, ProcessStatus::Crashed);
        assert_eq!(sm.process().pid, None); // PID should be cleared

        // Check side effects
        assert!(
            result
                .side_effects
                .iter()
                .any(|e| matches!(e, SideEffect::ClearPid))
        );
    }

    #[test]
    fn test_state_machine_auto_restart() {
        let mut process = create_test_process();
        process.auto_restart = true;
        let mut sm = ProcessStateMachine::new(process);

        // Start and run process
        sm.apply_event(ProcessEvent::StartRequested {
            requested_by: "user".to_string(),
            force: false,
        })
        .unwrap();
        sm.apply_event(ProcessEvent::ProcessSpawned { pid: 1234 })
            .unwrap();
        sm.apply_event(ProcessEvent::StartupCompleted).unwrap();

        // Crash the process with auto-restart enabled
        let crash_event = ProcessEvent::ProcessCrashed {
            exit_code: Some(1),
            signal: None,
        };
        let result = sm.apply_event(crash_event).unwrap();

        // Should schedule restart
        assert!(
            result
                .side_effects
                .iter()
                .any(|e| matches!(e, SideEffect::ScheduleRestart { .. }))
        );
    }

    #[test]
    fn test_state_machine_invalid_transition() {
        let process = create_test_process();
        let mut sm = ProcessStateMachine::new(process);

        // Try to stop a stopped process - should fail
        let stop_event = ProcessEvent::StopRequested {
            requested_by: "user".to_string(),
            force: false,
        };

        let result = sm.apply_event(stop_event);
        assert!(result.is_err());
    }

    #[test]
    fn test_state_machine_can_apply_event() {
        let process = create_test_process();
        let sm = ProcessStateMachine::new(process);

        let start_event = ProcessEvent::StartRequested {
            requested_by: "user".to_string(),
            force: false,
        };
        assert!(sm.can_apply_event(&start_event));

        let stop_event = ProcessEvent::StopRequested {
            requested_by: "user".to_string(),
            force: false,
        };
        assert!(!sm.can_apply_event(&stop_event)); // Can't stop a stopped process
    }

    #[test]
    fn test_state_machine_valid_events() {
        let process = create_test_process();
        let sm = ProcessStateMachine::new(process);

        let events = sm.valid_events();
        assert!(!events.is_empty());

        // Stopped process should be able to start
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ProcessEvent::StartRequested { .. }))
        );
    }
}
