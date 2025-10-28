//! Standalone time travel debugger for simulation failure analysis
//!
//! This module provides a powerful standalone tool for debugging simulation
//! failures through checkpoint restoration and trace replay capabilities.

use crate::{
    testing::PropertyViolation, tick, CheckpointInfo, CheckpointManager, PassiveTraceRecorder,
    Result, SimError, WorldState,
};
use aura_console_types::{TraceEvent, TraceMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// Standalone time travel debugger for failure analysis
///
/// This tool can replay simulations from checkpoints to debug failures.
/// It operates independently of the core simulation logic by using the
/// pure tick() function and pre-recorded traces.
pub struct TimeTravelDebugger {
    /// Checkpoint manager for loading world states
    checkpoint_manager: CheckpointManager,
    /// Current world state being debugged
    current_world: Option<WorldState>,
    /// Trace recorder for current debugging session
    trace_recorder: PassiveTraceRecorder,
    /// Replay configuration
    config: ReplayConfig,
    /// Debugging session metadata
    session_info: Option<DebuggingSession>,
}

/// Configuration for replay debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    /// Stop on first property violation
    pub stop_on_violation: bool,
    /// Maximum ticks to replay before stopping
    pub max_replay_ticks: u64,
    /// Whether to record events during replay
    pub record_replay_events: bool,
    /// Step-by-step debugging mode
    pub step_by_step: bool,
    /// Properties to check during replay
    pub check_properties: Vec<String>,
    /// Participants to focus debugging on
    pub focus_participants: Vec<String>,
}

/// Information about a debugging session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingSession {
    /// Session ID
    pub id: String,
    /// Description of what is being debugged
    pub description: String,
    /// Checkpoint being used as starting point
    pub starting_checkpoint: String,
    /// Target tick for replay
    pub target_tick: Option<u64>,
    /// Failure being investigated
    pub failure_description: Option<String>,
    /// When session was started
    pub started_at: u64,
    /// Original trace metadata
    pub original_metadata: Option<TraceMetadata>,
}

/// Result of a debugging replay
#[derive(Debug, Clone)]
pub struct ReplayResult {
    /// Whether replay reached the target
    pub reached_target: bool,
    /// Final tick reached
    pub final_tick: u64,
    /// Events generated during replay
    pub replay_events: Vec<TraceEvent>,
    /// Property violations found during replay
    pub violations_found: Vec<PropertyViolation>,
    /// Reason replay stopped
    pub stop_reason: ReplayStopReason,
    /// Performance metrics
    pub metrics: ReplayMetrics,
}

/// Reason why replay stopped
#[derive(Debug, Clone)]
pub enum ReplayStopReason {
    /// Reached target tick successfully
    ReachedTarget,
    /// Property violation encountered
    PropertyViolation(PropertyViolation),
    /// Maximum replay ticks exceeded
    MaxTicksExceeded,
    /// Error occurred during replay
    Error(String),
    /// User requested stop (step-by-step mode)
    UserStop,
    /// Simulation naturally ended
    SimulationEnded,
}

/// Performance metrics for replay
#[derive(Debug, Clone)]
pub struct ReplayMetrics {
    /// Total replay time in milliseconds
    pub replay_time_ms: u64,
    /// Ticks per second during replay
    pub ticks_per_second: f64,
    /// Events per second during replay
    pub events_per_second: f64,
    /// Memory usage peak during replay
    pub peak_memory_mb: u64,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            stop_on_violation: true,
            max_replay_ticks: 10000,
            record_replay_events: true,
            step_by_step: false,
            check_properties: Vec::new(),
            focus_participants: Vec::new(),
        }
    }
}

impl TimeTravelDebugger {
    /// Create a new time travel debugger
    pub fn new<P: AsRef<Path>>(checkpoint_dir: P) -> Result<Self> {
        let checkpoint_manager = CheckpointManager::new(checkpoint_dir)?;

        Ok(Self {
            checkpoint_manager,
            current_world: None,
            trace_recorder: PassiveTraceRecorder::new(),
            config: ReplayConfig::default(),
            session_info: None,
        })
    }

    /// Configure the replay behavior
    pub fn configure(&mut self, config: ReplayConfig) {
        self.config = config;
    }

    /// Start a debugging session from a checkpoint
    pub fn start_session(
        &mut self,
        checkpoint_id: &str,
        description: String,
        target_tick: Option<u64>,
    ) -> Result<()> {
        // Load the checkpoint
        let world_state = self.checkpoint_manager.load(checkpoint_id)?;
        let checkpoint_info = self
            .checkpoint_manager
            .get_info(checkpoint_id)
            .ok_or_else(|| {
                SimError::RuntimeError(format!("Checkpoint {} not found", checkpoint_id))
            })?;

        // Create session info
        let session = DebuggingSession {
            id: uuid::Uuid::new_v4().to_string(),
            description,
            starting_checkpoint: checkpoint_id.to_string(),
            target_tick,
            failure_description: None,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            original_metadata: None,
        };

        // Set up debugging state
        self.current_world = Some(world_state);
        let session_id = session.id.clone();
        self.session_info = Some(session);
        self.trace_recorder = PassiveTraceRecorder::new();

        // Update trace recorder metadata
        self.trace_recorder
            .set_scenario_name(format!("debug_session_{}", &session_id[..8]));
        self.trace_recorder.set_seed(checkpoint_info.metadata.seed);

        println!(
            "Started debugging session from checkpoint at tick {}",
            checkpoint_info.tick
        );

        Ok(())
    }

    /// Load a recorded session for replay debugging
    pub fn load_session<P: AsRef<Path>>(&mut self, session_path: P) -> Result<()> {
        let session_recorder = PassiveTraceRecorder::load_session(session_path)?;

        // Extract the starting checkpoint from trace metadata
        let scenario_name = {
            let metadata = session_recorder.metadata();
            metadata.scenario_name.clone()
        };

        // For this demo, we'll assume the session has enough info to restart
        // In practice, you'd need the original checkpoint ID
        self.trace_recorder = session_recorder;

        println!("Loaded recorded session: {}", scenario_name);

        Ok(())
    }

    /// Replay from current checkpoint to target tick
    pub fn replay_to_tick(&mut self, target_tick: u64) -> Result<ReplayResult> {
        let start_time = std::time::Instant::now();
        let start_tick = {
            let world = self
                .current_world
                .as_ref()
                .ok_or_else(|| SimError::RuntimeError("No debugging session active".to_string()))?;
            world.current_tick
        };

        let mut replay_events = Vec::new();
        let mut violations_found = Vec::new();

        println!("Replaying from tick {} to tick {}", start_tick, target_tick);

        // Replay loop using pure tick() function
        loop {
            let current_tick = {
                let world = self.current_world.as_ref().unwrap();
                world.current_tick
            };

            if current_tick >= target_tick {
                break;
            }

            // Check if we should stop
            if current_tick - start_tick > self.config.max_replay_ticks {
                let final_tick = current_tick;
                let metrics = self.calculate_metrics(start_time, start_tick, &replay_events);
                return Ok(ReplayResult {
                    reached_target: false,
                    final_tick,
                    replay_events,
                    violations_found,
                    stop_reason: ReplayStopReason::MaxTicksExceeded,
                    metrics,
                });
            }

            // Execute one tick using pure function
            let tick_result = {
                let world = self.current_world.as_mut().unwrap();
                tick(world)
            };

            match tick_result {
                Ok(events) => {
                    // Record events if enabled
                    if self.config.record_replay_events {
                        replay_events.extend(events.iter().cloned());
                        self.trace_recorder.record_tick_events(&events);
                    }

                    // Check for property violations
                    for event in &events {
                        if let aura_console_types::EventType::PropertyViolation {
                            property,
                            violation_details,
                        } = &event.event_type
                        {
                            let violation = PropertyViolation {
                                property_name: property.clone(),
                                property_type: crate::testing::PropertyViolationType::Invariant,
                                violation_state: crate::testing::SimulationState {
                                    tick: event.tick,
                                    time: 0, // TraceEvent doesn't have time field, default to 0
                                    variables: std::collections::HashMap::new(),
                                    participants: Vec::new(),
                                    protocol_state: crate::testing::ProtocolMonitoringState {
                                        active_sessions: Vec::new(),
                                        completed_sessions: Vec::new(),
                                        queued_protocols: Vec::new(),
                                    },
                                    network_state: crate::testing::NetworkStateSnapshot {
                                        partitions: Vec::new(),
                                        message_stats: crate::testing::MessageDeliveryStats {
                                            messages_sent: 0,
                                            messages_delivered: 0,
                                            messages_dropped: 0,
                                            average_latency_ms: 0.0,
                                        },
                                        failure_conditions:
                                            crate::testing::NetworkFailureConditions {
                                                drop_rate: 0.0,
                                                latency_range_ms: (0, 100),
                                                partitions_active: false,
                                            },
                                    },
                                },
                                violation_details: crate::testing::ViolationDetails {
                                    description: violation_details.clone(),
                                    evidence: Vec::new(),
                                    potential_causes: Vec::new(),
                                    severity: crate::testing::ViolationSeverity::Medium,
                                    remediation_suggestions: Vec::new(),
                                },
                                confidence: 1.0,
                                detected_at: event.tick,
                            };
                            violations_found.push(violation.clone());

                            if self.config.stop_on_violation {
                                println!(
                                    "[ERROR] Property violation at tick {}: {}",
                                    event.tick, property
                                );
                                let final_tick = {
                                    let world = self.current_world.as_ref().unwrap();
                                    world.current_tick
                                };
                                let metrics =
                                    self.calculate_metrics(start_time, start_tick, &replay_events);
                                return Ok(ReplayResult {
                                    reached_target: false,
                                    final_tick,
                                    replay_events,
                                    violations_found,
                                    stop_reason: ReplayStopReason::PropertyViolation(violation),
                                    metrics,
                                });
                            }
                        }
                    }

                    // Step-by-step debugging
                    if self.config.step_by_step {
                        let (tick, event_count) = {
                            let world = self.current_world.as_ref().unwrap();
                            (world.current_tick, events.len())
                        };
                        println!("Tick {}: {} events", tick, event_count);
                        // In a real implementation, you'd wait for user input here
                    }
                }
                Err(e) => {
                    let final_tick = {
                        let world = self.current_world.as_ref().unwrap();
                        world.current_tick
                    };
                    let metrics = self.calculate_metrics(start_time, start_tick, &replay_events);
                    return Ok(ReplayResult {
                        reached_target: false,
                        final_tick,
                        replay_events,
                        violations_found,
                        stop_reason: ReplayStopReason::Error(e.to_string()),
                        metrics,
                    });
                }
            }
        }

        let final_tick = {
            let world = self.current_world.as_ref().unwrap();
            world.current_tick
        };
        let metrics = self.calculate_metrics(start_time, start_tick, &replay_events);
        println!("[OK] Successfully replayed to tick {}", target_tick);

        Ok(ReplayResult {
            reached_target: true,
            final_tick,
            replay_events,
            violations_found,
            stop_reason: ReplayStopReason::ReachedTarget,
            metrics,
        })
    }

    /// Step forward one tick in debugging mode
    pub fn step_forward(&mut self) -> Result<Vec<TraceEvent>> {
        let world = self
            .current_world
            .as_mut()
            .ok_or_else(|| SimError::RuntimeError("No debugging session active".to_string()))?;

        println!(
            "Stepping from tick {} to {}",
            world.current_tick,
            world.current_tick + 1
        );

        let events = tick(world)?;

        if self.config.record_replay_events {
            self.trace_recorder.record_tick_events(&events);
        }

        println!(
            "Generated {} events at tick {}",
            events.len(),
            world.current_tick - 1
        );

        Ok(events)
    }

    /// Get current world state for inspection
    pub fn current_world_state(&self) -> Option<&WorldState> {
        self.current_world.as_ref()
    }

    /// Get current debugging session info
    pub fn session_info(&self) -> Option<&DebuggingSession> {
        self.session_info.as_ref()
    }

    /// Get trace recorder for event analysis
    pub fn trace_recorder(&self) -> &PassiveTraceRecorder {
        &self.trace_recorder
    }

    /// List available checkpoints for debugging
    pub fn list_available_checkpoints(&self) -> Vec<&CheckpointInfo> {
        self.checkpoint_manager.list_checkpoints()
    }

    /// Find checkpoints near a specific tick
    pub fn find_checkpoints_near_tick(&self, target_tick: u64, range: u64) -> Vec<&CheckpointInfo> {
        let start_tick = target_tick.saturating_sub(range);
        let end_tick = target_tick + range;
        self.checkpoint_manager
            .list_checkpoints_in_range(start_tick, end_tick)
    }

    /// Get the closest checkpoint before a failure tick
    pub fn find_checkpoint_before_failure(&self, failure_tick: u64) -> Option<&CheckpointInfo> {
        self.checkpoint_manager
            .find_closest_checkpoint(failure_tick)
    }

    /// Analyze events around a specific tick
    pub fn analyze_events_around_tick(
        &self,
        target_tick: u64,
        range: u64,
    ) -> Result<EventAnalysis> {
        if !self.trace_recorder.metadata().total_ticks > 0 {
            return Err(SimError::RuntimeError(
                "No trace data available for analysis".to_string(),
            ));
        }

        let start_tick = target_tick.saturating_sub(range);
        let end_tick = target_tick + range;

        let events = self
            .trace_recorder
            .get_events_in_range(start_tick, end_tick)?;

        // Analyze events by participant
        let mut participant_activity = HashMap::new();
        let mut event_types = HashMap::new();

        for event in &events {
            *participant_activity
                .entry(event.participant.clone())
                .or_insert(0) += 1;
            let event_type_name = format!("{:?}", event.event_type)
                .split(' ')
                .next()
                .unwrap_or("Unknown")
                .to_string();
            *event_types.entry(event_type_name).or_insert(0) += 1;
        }

        Ok(EventAnalysis {
            target_tick,
            range,
            total_events: events.len(),
            participant_activity,
            event_types,
            violations: self
                .trace_recorder
                .violations()
                .iter()
                .filter(|v| {
                    v.violation_state.tick >= start_tick && v.violation_state.tick <= end_tick
                })
                .cloned()
                .collect(),
        })
    }

    /// Save current debugging session
    pub fn save_session<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.trace_recorder.save_session(path)
    }

    /// Export debugging report
    pub fn export_report<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let session = self
            .session_info
            .as_ref()
            .ok_or_else(|| SimError::RuntimeError("No active session".to_string()))?;

        let report = DebuggingReport {
            session: session.clone(),
            config: self.config.clone(),
            total_events: self.trace_recorder.event_count(),
            violations: self.trace_recorder.violations().to_vec(),
            checkpoints_used: self.checkpoint_manager.list_checkpoints().len(),
            final_tick: self
                .current_world
                .as_ref()
                .map(|w| w.current_tick)
                .unwrap_or(0),
            generated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let json = serde_json::to_string_pretty(&report)
            .map_err(|e| SimError::RuntimeError(format!("Failed to serialize report: {}", e)))?;

        fs::write(path, json)
            .map_err(|e| SimError::RuntimeError(format!("Failed to write report: {}", e)))?;

        Ok(())
    }

    // Private helper methods

    /// Calculate performance metrics for replay
    fn calculate_metrics(
        &self,
        start_time: std::time::Instant,
        start_tick: u64,
        events: &[TraceEvent],
    ) -> ReplayMetrics {
        let elapsed = start_time.elapsed();
        let elapsed_ms = elapsed.as_millis() as u64;
        let elapsed_secs = elapsed.as_secs_f64();

        let current_tick = self
            .current_world
            .as_ref()
            .map(|w| w.current_tick)
            .unwrap_or(start_tick);
        let ticks_processed = current_tick.saturating_sub(start_tick);

        ReplayMetrics {
            replay_time_ms: elapsed_ms,
            ticks_per_second: if elapsed_secs > 0.0 {
                ticks_processed as f64 / elapsed_secs
            } else {
                0.0
            },
            events_per_second: if elapsed_secs > 0.0 {
                events.len() as f64 / elapsed_secs
            } else {
                0.0
            },
            peak_memory_mb: 0, // Would need actual memory tracking
        }
    }
}

/// Analysis of events around a specific tick
#[derive(Debug, Clone)]
pub struct EventAnalysis {
    /// Target tick being analyzed
    pub target_tick: u64,
    /// Range around target tick
    pub range: u64,
    /// Total events in range
    pub total_events: usize,
    /// Activity count by participant
    pub participant_activity: HashMap<String, usize>,
    /// Event type distribution
    pub event_types: HashMap<String, usize>,
    /// Property violations in range
    pub violations: Vec<PropertyViolation>,
}

/// Complete debugging session report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingReport {
    /// Session information
    pub session: DebuggingSession,
    /// Configuration used
    pub config: ReplayConfig,
    /// Total events processed
    pub total_events: usize,
    /// Violations found
    pub violations: Vec<PropertyViolation>,
    /// Number of checkpoints used
    pub checkpoints_used: usize,
    /// Final tick reached
    pub final_tick: u64,
    /// When report was generated
    pub generated_at: u64,
}

/// Debug session with checkpoint navigation and failure analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugSession {
    /// Unique session identifier
    pub session_id: String,
    /// Human-readable session name
    pub session_name: String,
    /// When the session was created
    pub created_at: u64,
    /// Simulation being debugged
    pub simulation_id: Uuid,
    /// Available checkpoints for navigation
    pub checkpoints: Vec<String>,
    /// Current position in the session
    pub current_position: SessionPosition,
    /// Violations detected during debugging
    pub detected_violations: Vec<PropertyViolation>,
    /// Failure analyses performed
    pub failure_analyses: Vec<String>,
    /// Session metadata
    pub metadata: SessionMetadata,
    /// Navigation path taken during debugging
    pub navigation_path: Vec<String>,
}

/// Current position within a debug session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPosition {
    /// Current checkpoint being debugged
    pub current_checkpoint: Option<String>,
    /// Current tick position
    pub current_tick: u64,
    /// Current simulation time
    pub current_time: u64,
    /// Index in checkpoint list
    pub checkpoint_index: usize,
}

/// Metadata for a debug session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Violation that triggered this debug session
    pub trigger_violation: Option<PropertyViolation>,
    /// Target scenario being debugged
    pub target_scenario: Option<String>,
    /// Debug objectives
    pub objectives: Vec<String>,
    /// Session tags for organization
    pub tags: Vec<String>,
    /// Priority level of this debug session
    pub priority: DebugPriority,
}

/// Priority levels for debug sessions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DebugPriority {
    /// Low priority debugging
    Low,
    /// Normal priority debugging
    Normal,
    /// High priority debugging
    High,
    /// Critical debugging session
    Critical,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_state::WorldState;
    use tempfile::TempDir;

    fn create_test_world_state() -> WorldState {
        crate::test_utils::two_party_world_state()
    }

    #[test]
    fn test_debugger_creation() {
        let temp_dir = TempDir::new().unwrap();
        let debugger = TimeTravelDebugger::new(temp_dir.path()).unwrap();

        assert!(debugger.current_world.is_none());
        assert!(debugger.session_info.is_none());
    }

    #[test]
    fn test_session_start() {
        let temp_dir = TempDir::new().unwrap();
        let mut checkpoint_manager = CheckpointManager::new(temp_dir.path()).unwrap();

        // Create a checkpoint first
        let world_state = create_test_world_state();
        let checkpoint_id = checkpoint_manager
            .save(&world_state, Some("test".to_string()))
            .unwrap();

        // Now test debugger
        let mut debugger = TimeTravelDebugger::new(temp_dir.path()).unwrap();
        debugger
            .start_session(&checkpoint_id, "test session".to_string(), Some(10))
            .unwrap();

        assert!(debugger.current_world.is_some());
        assert!(debugger.session_info.is_some());

        let session = debugger.session_info().unwrap();
        assert_eq!(session.description, "test session");
        assert_eq!(session.target_tick, Some(10));
    }

    #[test]
    fn test_step_forward() {
        let temp_dir = TempDir::new().unwrap();
        let mut checkpoint_manager = CheckpointManager::new(temp_dir.path()).unwrap();

        let world_state = create_test_world_state();
        let checkpoint_id = checkpoint_manager.save(&world_state, None).unwrap();

        let mut debugger = TimeTravelDebugger::new(temp_dir.path()).unwrap();
        debugger
            .start_session(&checkpoint_id, "step test".to_string(), None)
            .unwrap();

        let initial_tick = debugger.current_world_state().unwrap().current_tick;
        let events = debugger.step_forward().unwrap();
        let final_tick = debugger.current_world_state().unwrap().current_tick;

        assert_eq!(final_tick, initial_tick + 1);
        assert!(events.len() >= 0); // May or may not generate events
    }

    #[test]
    fn test_replay_to_tick() {
        let temp_dir = TempDir::new().unwrap();
        let mut checkpoint_manager = CheckpointManager::new(temp_dir.path()).unwrap();

        let world_state = create_test_world_state();
        let checkpoint_id = checkpoint_manager.save(&world_state, None).unwrap();

        let mut debugger = TimeTravelDebugger::new(temp_dir.path()).unwrap();
        debugger
            .start_session(&checkpoint_id, "replay test".to_string(), Some(5))
            .unwrap();

        let result = debugger.replay_to_tick(5).unwrap();

        assert!(result.reached_target);
        assert_eq!(result.final_tick, 5);
        assert!(matches!(
            result.stop_reason,
            ReplayStopReason::ReachedTarget
        ));
    }

    #[test]
    fn test_checkpoint_finding() {
        let temp_dir = TempDir::new().unwrap();
        let mut checkpoint_manager = CheckpointManager::new(temp_dir.path()).unwrap();

        // Create checkpoints at different ticks
        let mut world_state = create_test_world_state();
        world_state.current_tick = 10;
        let _cp1 = checkpoint_manager
            .save(&world_state, Some("checkpoint 1".to_string()))
            .unwrap();

        world_state.current_tick = 30;
        let _cp2 = checkpoint_manager
            .save(&world_state, Some("checkpoint 2".to_string()))
            .unwrap();

        let debugger = TimeTravelDebugger::new(temp_dir.path()).unwrap();

        let closest = debugger.find_checkpoint_before_failure(25);
        assert!(closest.is_some());
        assert_eq!(closest.unwrap().tick, 10);

        let near_checkpoints = debugger.find_checkpoints_near_tick(20, 15);
        assert_eq!(near_checkpoints.len(), 2);
    }

    #[test]
    fn test_configuration() {
        let temp_dir = TempDir::new().unwrap();
        let mut debugger = TimeTravelDebugger::new(temp_dir.path()).unwrap();

        let config = ReplayConfig {
            stop_on_violation: false,
            max_replay_ticks: 1000,
            step_by_step: true,
            ..Default::default()
        };

        debugger.configure(config.clone());

        assert_eq!(debugger.config.stop_on_violation, false);
        assert_eq!(debugger.config.max_replay_ticks, 1000);
        assert_eq!(debugger.config.step_by_step, true);
    }
}
