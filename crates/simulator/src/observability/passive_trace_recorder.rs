//! Passive trace recorder that observes simulation without coupling
//!
//! This module implements a TraceRecorder as a passive listener that records
//! events fed to it by external runners, without any knowledge of or coupling
//! to the core simulation logic.

use crate::world_state::WorldState;
use crate::{Result, SimError};
use aura_console_types::trace::CheckpointRef;
use crate::testing::PropertyViolation;
use aura_console_types::{
    NetworkTopology, ParticipantInfo, SimulationTrace, TraceEvent, TraceMetadata,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Passive trace recorder that acts as an external observer
///
/// This recorder receives events from external test runners and records them
/// without any knowledge of the simulation internals. It's a pure data store
/// with indexing and export capabilities.
pub struct PassiveTraceRecorder {
    /// All recorded events in chronological order
    events: Vec<TraceEvent>,
    /// Event index by participant for efficient queries
    participant_index: HashMap<String, Vec<usize>>,
    /// Event index by tick for range queries
    tick_index: HashMap<u64, Vec<usize>>,
    /// Checkpoints recorded during simulation
    checkpoints: Vec<CheckpointRef>,
    /// Property violations detected
    violations: Vec<PropertyViolation>,
    /// Simulation metadata
    metadata: TraceMetadata,
    /// Whether to enable automatic indexing
    auto_index: bool,
}

/// Recorded simulation session that can be saved/loaded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedSession {
    /// Session metadata
    pub metadata: TraceMetadata,
    /// Complete event timeline
    pub events: Vec<TraceEvent>,
    /// Checkpoints created during session
    pub checkpoints: Vec<CheckpointRef>,
    /// Property violations detected
    pub violations: Vec<PropertyViolation>,
    /// Recording timestamp
    pub recorded_at: u64,
}

impl PassiveTraceRecorder {
    /// Create a new passive trace recorder
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            participant_index: HashMap::new(),
            tick_index: HashMap::new(),
            checkpoints: Vec::new(),
            violations: Vec::new(),
            metadata: TraceMetadata {
                scenario_name: "passive_recording".to_string(),
                seed: 0,
                total_ticks: 0,
                properties_checked: Vec::new(),
                violations: Vec::new(),
            },
            auto_index: true,
        }
    }

    /// Create with initial metadata
    pub fn with_metadata(metadata: TraceMetadata) -> Self {
        Self {
            events: Vec::new(),
            participant_index: HashMap::new(),
            tick_index: HashMap::new(),
            checkpoints: Vec::new(),
            violations: Vec::new(),
            metadata,
            auto_index: true,
        }
    }

    /// Create with indexing disabled for high-performance recording
    pub fn without_indexing() -> Self {
        let mut recorder = Self::new();
        recorder.auto_index = false;
        recorder
    }

    /// Record events from a simulation tick
    ///
    /// This is the main interface used by external test runners.
    /// The runner calls tick(), gets events, and feeds them here.
    pub fn record_tick_events(&mut self, events: &[TraceEvent]) {
        for event in events {
            self.record_event(event.clone());
        }
    }

    /// Record a single event (private implementation)
    fn record_event(&mut self, event: TraceEvent) {
        let event_index = self.events.len();

        // Handle special event types
        self.handle_special_event(&event);

        // Store the event
        self.events.push(event.clone());

        // Update indices if enabled
        if self.auto_index {
            self.update_indices(event_index, &event);
        }

        // Update metadata
        self.update_metadata();
    }

    /// Handle special event types (checkpoints, violations)
    fn handle_special_event(&mut self, event: &TraceEvent) {
        match &event.event_type {
            aura_console_types::EventType::PropertyViolation {
                property,
                violation_details,
            } => {
                let violation = PropertyViolation {
                    property_name: property.clone(),
                    property_type: crate::testing::PropertyViolationType::Safety,
                    violation_state: crate::testing::SimulationState {
                        tick: event.tick,
                        time: event.tick * 100, // Convert tick to time estimate
                        participants: vec![],
                        protocol_state: crate::testing::ProtocolMonitoringState {
                            active_sessions: vec![],
                            completed_sessions: vec![],
                            queued_protocols: vec![],
                        },
                        network_state: crate::testing::NetworkStateSnapshot {
                            partitions: vec![],
                            message_stats: crate::testing::MessageDeliveryStats {
                                total_sent: 0,
                                total_delivered: 0,
                                total_dropped: 0,
                                average_latency_ms: 0.0,
                            },
                            failure_conditions: crate::testing::NetworkFailureConditions {
                                drop_rate: 0.0,
                                latency_range: (0, 0),
                                partition_count: 0,
                            },
                        },
                    },
                    violation_details: crate::testing::ViolationDetails {
                        description: violation_details.clone(),
                        evidence: vec![format!("Participant: {}", event.participant)],
                        potential_causes: vec![],
                        severity: crate::testing::ViolationSeverity::Medium,
                        remediation_suggestions: vec![],
                    },
                    confidence: 0.8,
                    detected_at: event.tick,
                };
                self.violations.push(violation);
            }
            aura_console_types::EventType::CheckpointCreated {
                checkpoint_id,
                label,
            } => {
                let checkpoint = CheckpointRef {
                    id: checkpoint_id.clone(),
                    label: label.clone(),
                    tick: event.tick,
                };
                self.checkpoints.push(checkpoint);
            }
            _ => {} // Regular events, no special handling needed
        }
    }

    /// Update indices for fast queries
    fn update_indices(&mut self, event_index: usize, event: &TraceEvent) {
        // Update participant index
        self.participant_index
            .entry(event.participant.clone())
            .or_default()
            .push(event_index);

        // Update tick index
        self.tick_index
            .entry(event.tick)
            .or_default()
            .push(event_index);
    }

    /// Update metadata based on current events
    fn update_metadata(&mut self) {
        if let Some(last_event) = self.events.last() {
            self.metadata.total_ticks = self.metadata.total_ticks.max(last_event.tick);
        }
        // TODO: Fix metadata violations field mismatch
        // self.metadata.violations = self.violations.clone();
    }

    /// Build indices if they were disabled during recording
    pub fn build_indices(&mut self) {
        if self.auto_index {
            return; // Already indexed
        }

        self.participant_index.clear();
        self.tick_index.clear();

        // Collect indices to update separately to avoid borrow conflict
        let updates: Vec<(usize, TraceEvent)> = self
            .events
            .iter()
            .enumerate()
            .map(|(i, event)| (i, event.clone()))
            .collect();

        for (i, event) in updates {
            self.update_indices(i, &event);
        }

        self.auto_index = true;
    }

    /// Record a checkpoint reference
    ///
    /// Called by external checkpoint managers to record checkpoint creation
    pub fn record_checkpoint(&mut self, checkpoint: CheckpointRef) {
        self.checkpoints.push(checkpoint);
    }

    /// Record a property violation
    ///
    /// Called by external property checkers to record violations
    pub fn record_violation(&mut self, violation: PropertyViolation) {
        self.violations.push(violation);
    }

    /// Set simulation metadata
    pub fn set_metadata(&mut self, metadata: TraceMetadata) {
        self.metadata = metadata;
    }

    /// Update scenario name
    pub fn set_scenario_name(&mut self, name: String) {
        self.metadata.scenario_name = name;
    }

    /// Update seed
    pub fn set_seed(&mut self, seed: u64) {
        self.metadata.seed = seed;
    }

    /// Add property that was checked
    pub fn add_property_check(&mut self, property: String) {
        if !self.metadata.properties_checked.contains(&property) {
            self.metadata.properties_checked.push(property);
        }
    }

    /// Get events by participant (requires indexing)
    pub fn get_events_by_participant(&self, participant: &str) -> Result<Vec<&TraceEvent>> {
        if !self.auto_index {
            return Err(SimError::RuntimeError(
                "Indexing disabled - call build_indices() first".to_string(),
            ));
        }

        Ok(self
            .participant_index
            .get(participant)
            .map(|indices| indices.iter().map(|&i| &self.events[i]).collect())
            .unwrap_or_default())
    }

    /// Get events in tick range (requires indexing)
    pub fn get_events_in_range(&self, start_tick: u64, end_tick: u64) -> Result<Vec<&TraceEvent>> {
        if !self.auto_index {
            return Err(SimError::RuntimeError(
                "Indexing disabled - call build_indices() first".to_string(),
            ));
        }

        let mut result = Vec::new();
        for tick in start_tick..=end_tick {
            if let Some(indices) = self.tick_index.get(&tick) {
                for &index in indices {
                    result.push(&self.events[index]);
                }
            }
        }
        Ok(result)
    }

    /// Get events at specific tick (requires indexing)
    pub fn get_events_at_tick(&self, tick: u64) -> Result<Vec<&TraceEvent>> {
        if !self.auto_index {
            return Err(SimError::RuntimeError(
                "Indexing disabled - call build_indices() first".to_string(),
            ));
        }

        Ok(self
            .tick_index
            .get(&tick)
            .map(|indices| indices.iter().map(|&i| &self.events[i]).collect())
            .unwrap_or_default())
    }

    /// Get all events (no indexing required)
    pub fn get_all_events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Get current metadata
    pub fn metadata(&self) -> &TraceMetadata {
        &self.metadata
    }

    /// Get recorded checkpoints
    pub fn checkpoints(&self) -> &[CheckpointRef] {
        &self.checkpoints
    }

    /// Get property violations
    pub fn violations(&self) -> &[PropertyViolation] {
        &self.violations
    }

    /// Get event count
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Get participant count (requires indexing)
    pub fn participant_count(&self) -> usize {
        if self.auto_index {
            self.participant_index.len()
        } else {
            // Count unique participants without indexing
            let mut participants = std::collections::HashSet::new();
            for event in &self.events {
                participants.insert(&event.participant);
            }
            participants.len()
        }
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Clear all recorded data
    pub fn clear(&mut self) {
        self.events.clear();
        self.participant_index.clear();
        self.tick_index.clear();
        self.checkpoints.clear();
        self.violations.clear();
        self.metadata.total_ticks = 0;
        // TODO: Fix metadata violations field mismatch  
        // self.metadata.violations.clear();
    }

    /// Export complete simulation trace
    pub fn export_trace(&self, world_state: Option<&WorldState>) -> SimulationTrace {
        let participants = if let Some(world) = world_state {
            // Build participant info from world state
            world
                .participants
                .iter()
                .map(|(id, p)| {
                    (
                        id.clone(),
                        ParticipantInfo {
                            device_id: p.device_id.clone(),
                            participant_type: p.participant_type,
                            status: p.status,
                        },
                    )
                })
                .collect()
        } else {
            HashMap::new()
        };

        let network_topology = if let Some(world) = world_state {
            // Build network topology from world state
            NetworkTopology {
                nodes: world
                    .participants
                    .iter()
                    .map(|(id, p)| {
                        (
                            id.clone(),
                            aura_console_types::NodeInfo {
                                device_id: p.device_id.clone(),
                                participant_type: p.participant_type,
                                status: p.status,
                                message_count: p.message_count,
                            },
                        )
                    })
                    .collect(),
                edges: Vec::new(), // Would be built from message events
                partitions: world
                    .network
                    .partitions
                    .iter()
                    .map(|p| aura_console_types::PartitionInfo {
                        devices: p.participants.clone(),
                        created_at_tick: p.started_at / 100, // Convert time to tick estimate
                    })
                    .collect(),
            }
        } else {
            NetworkTopology {
                nodes: HashMap::new(),
                edges: Vec::new(),
                partitions: Vec::new(),
            }
        };

        SimulationTrace {
            metadata: self.metadata.clone(),
            timeline: self.events.clone(),
            checkpoints: self.checkpoints.clone(),
            participants,
            network_topology,
        }
    }

    /// Save recorded session to file
    pub fn save_session<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let session = RecordedSession {
            metadata: self.metadata.clone(),
            events: self.events.clone(),
            checkpoints: self.checkpoints.clone(),
            violations: self.violations.clone(),
            recorded_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let json = serde_json::to_string_pretty(&session)
            .map_err(|e| SimError::RuntimeError(format!("Failed to serialize session: {}", e)))?;

        fs::write(path, json)
            .map_err(|e| SimError::RuntimeError(format!("Failed to write session file: {}", e)))?;

        Ok(())
    }

    /// Load recorded session from file
    pub fn load_session<P: AsRef<Path>>(path: P) -> Result<Self> {
        let json = fs::read_to_string(path)
            .map_err(|e| SimError::RuntimeError(format!("Failed to read session file: {}", e)))?;

        let session: RecordedSession = serde_json::from_str(&json)
            .map_err(|e| SimError::RuntimeError(format!("Failed to deserialize session: {}", e)))?;

        let mut recorder = Self::with_metadata(session.metadata);
        recorder.events = session.events;
        recorder.checkpoints = session.checkpoints;
        recorder.violations = session.violations;

        // Build indices
        recorder.build_indices();

        Ok(recorder)
    }
}

impl Default for PassiveTraceRecorder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_console_types::{CausalityInfo, EventType};

    fn create_test_event(tick: u64, event_id: u64, participant: &str) -> TraceEvent {
        TraceEvent {
            tick,
            event_id,
            event_type: EventType::EffectExecuted {
                effect_type: "test".to_string(),
                effect_data: vec![],
            },
            participant: participant.to_string(),
            causality: CausalityInfo {
                parent_events: vec![],
                happens_before: vec![],
                concurrent_with: vec![],
            },
        }
    }

    #[test]
    fn test_passive_recorder_basic() {
        let mut recorder = PassiveTraceRecorder::new();
        assert!(recorder.is_empty());
        assert_eq!(recorder.event_count(), 0);

        let events = vec![
            create_test_event(0, 0, "alice"),
            create_test_event(1, 1, "bob"),
        ];

        recorder.record_tick_events(&events);

        assert_eq!(recorder.event_count(), 2);
        assert_eq!(recorder.participant_count(), 2);
    }

    #[test]
    fn test_passive_recorder_without_indexing() {
        let mut recorder = PassiveTraceRecorder::without_indexing();

        let events = vec![
            create_test_event(0, 0, "alice"),
            create_test_event(1, 1, "bob"),
        ];

        recorder.record_tick_events(&events);

        // Should fail without indexing
        assert!(recorder.get_events_by_participant("alice").is_err());

        // Build indices
        recorder.build_indices();

        // Should work after building indices
        let alice_events = recorder.get_events_by_participant("alice").unwrap();
        assert_eq!(alice_events.len(), 1);
        assert_eq!(alice_events[0].participant, "alice");
    }

    #[test]
    fn test_checkpoint_recording() {
        let mut recorder = PassiveTraceRecorder::new();

        let checkpoint = CheckpointRef {
            id: "checkpoint_1".to_string(),
            label: "test checkpoint".to_string(),
            tick: 5,
        };

        recorder.record_checkpoint(checkpoint.clone());

        assert_eq!(recorder.checkpoints().len(), 1);
        assert_eq!(recorder.checkpoints()[0].id, "checkpoint_1");
    }

    #[test]
    fn test_violation_recording() {
        let mut recorder = PassiveTraceRecorder::new();

        let violation = PropertyViolation {
            property_name: "safety".to_string(),
            property_type: crate::testing::PropertyViolationType::Safety,
            violation_state: crate::testing::SimulationState {
                tick: 10,
                time: 1000,
                participants: vec![],
                protocol_state: crate::testing::ProtocolExecutionState {
                    active_sessions: vec![],
                    completed_sessions: vec![],
                    queued_protocols: vec![],
                },
                network_state: crate::testing::NetworkStateSnapshot {
                    partitions: vec![],
                    message_stats: crate::testing::MessageDeliveryStats {
                        total_sent: 0,
                        total_delivered: 0,
                        total_dropped: 0,
                        average_latency_ms: 0.0,
                    },
                    failure_conditions: crate::testing::NetworkFailureConditions {
                        drop_rate: 0.0,
                        latency_range: (0, 0),
                        partition_count: 0,
                    },
                },
            },
            violation_details: crate::testing::ViolationDetails {
                description: "Safety violation detected".to_string(),
                evidence: vec![format!("Participant: alice")],
                potential_causes: vec![],
                severity: crate::testing::ViolationSeverity::Medium,
                remediation_suggestions: vec![],
            },
            confidence: 0.8,
            detected_at: 10,
        };

        recorder.record_violation(violation.clone());

        assert_eq!(recorder.violations().len(), 1);
        assert_eq!(recorder.violations()[0].property_name, "safety");
    }

    #[test]
    fn test_session_save_load() {
        let mut recorder = PassiveTraceRecorder::new();
        recorder.set_scenario_name("test_scenario".to_string());
        recorder.set_seed(42);

        let events = vec![
            create_test_event(0, 0, "alice"),
            create_test_event(1, 1, "bob"),
        ];

        recorder.record_tick_events(&events);

        // Save session
        let temp_file = "/tmp/test_session.json";
        recorder.save_session(temp_file).unwrap();

        // Load session
        let loaded_recorder = PassiveTraceRecorder::load_session(temp_file).unwrap();

        assert_eq!(loaded_recorder.event_count(), 2);
        assert_eq!(loaded_recorder.metadata().scenario_name, "test_scenario");
        assert_eq!(loaded_recorder.metadata().seed, 42);

        // Clean up
        let _ = std::fs::remove_file(temp_file);
    }

    #[test]
    fn test_trace_export() {
        let mut recorder = PassiveTraceRecorder::new();
        recorder.set_scenario_name("export_test".to_string());

        let events = vec![create_test_event(0, 0, "alice")];
        recorder.record_tick_events(&events);

        let trace = recorder.export_trace(None);

        assert_eq!(trace.metadata.scenario_name, "export_test");
        assert_eq!(trace.timeline.len(), 1);
        assert!(trace.participants.is_empty()); // No world state provided
    }
}
