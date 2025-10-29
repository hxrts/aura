//! Trace event recording and management for simulation instrumentation
//!
//! This module provides efficient trace event recording, storage, and export
//! capabilities for the instrumented simulation engine.

use crate::testing::PropertyViolation;
use aura_console_types::{
    trace::CheckpointRef, NetworkTopology, SimulationTrace, TraceEvent, TraceMetadata,
};
use std::collections::HashMap;

/// Efficient trace event recorder with filtering and export capabilities
pub struct TraceRecorder {
    /// All recorded events in chronological order
    events: Vec<TraceEvent>,
    /// Event index by participant for efficient queries
    participant_index: HashMap<String, Vec<usize>>,
    /// Event index by tick for range queries
    tick_index: HashMap<u64, Vec<usize>>,
    /// Checkpoints created during recording
    checkpoints: Vec<CheckpointRef>,
    /// Property violations detected
    violations: Vec<PropertyViolation>,
    /// Metadata about the trace
    metadata: TraceMetadata,
}

impl TraceRecorder {
    /// Create a new trace recorder
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            participant_index: HashMap::new(),
            tick_index: HashMap::new(),
            checkpoints: Vec::new(),
            violations: Vec::new(),
            metadata: TraceMetadata {
                scenario_name: "simulation".to_string(),
                seed: 0,
                total_ticks: 0,
                properties_checked: Vec::new(),
                violations: Vec::new(),
            },
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
        }
    }

    /// Record a new trace event
    pub fn record(&mut self, event: TraceEvent) {
        let event_index = self.events.len();

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

        // Check for property violations
        if let aura_console_types::EventType::PropertyViolation {
            property,
            violation_details,
        } = &event.event_type
        {
            let violation = PropertyViolation {
                property_name: property.clone(),
                property_type: crate::results::PropertyViolationType::Safety,
                violation_state: crate::results::SimulationStateSnapshot {
                    tick: event.tick,
                    time: event.tick * 100, // Convert tick to time estimate
                    participant_count: 0,   // TODO: Extract from event data
                    active_sessions: 0,     // TODO: Extract from event data
                    completed_sessions: 0,  // TODO: Extract from event data
                    state_hash: "placeholder".to_string(), // TODO: Generate proper hash
                },
                violation_details: crate::results::ViolationDetails {
                    description: violation_details.clone(),
                    evidence: vec![format!("Participant: {}", event.participant)],
                    potential_causes: vec![],
                    severity: crate::results::ViolationSeverity::Medium,
                    remediation_suggestions: vec![],
                },
                confidence: 0.8,
                detected_at: event.tick,
            };
            self.violations.push(violation);
        }

        // Check for checkpoint events
        if let aura_console_types::EventType::CheckpointCreated {
            checkpoint_id,
            label,
        } = &event.event_type
        {
            let checkpoint = CheckpointRef {
                id: checkpoint_id.clone(),
                label: label.clone(),
                tick: event.tick,
            };
            self.checkpoints.push(checkpoint);
        }

        // Store the event
        self.events.push(event);

        // Update metadata
        self.metadata.total_ticks = self
            .metadata
            .total_ticks
            .max(self.events.last().map(|e| e.tick).unwrap_or(0));
        // TODO: Fix metadata violations field mismatch
        // self.metadata.violations = self.violations.clone();
    }

    /// Get events by participant
    pub fn get_events_by_participant(&self, participant: &str) -> Vec<&TraceEvent> {
        self.participant_index
            .get(participant)
            .map(|indices| indices.iter().map(|&i| &self.events[i]).collect())
            .unwrap_or_default()
    }

    /// Get events in tick range
    pub fn get_events_in_range(&self, start_tick: u64, end_tick: u64) -> Vec<&TraceEvent> {
        let mut result = Vec::new();
        for tick in start_tick..=end_tick {
            if let Some(indices) = self.tick_index.get(&tick) {
                for &index in indices {
                    result.push(&self.events[index]);
                }
            }
        }
        result
    }

    /// Get all events
    pub fn get_all_events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Get events at specific tick
    pub fn get_events_at_tick(&self, tick: u64) -> Vec<&TraceEvent> {
        self.tick_index
            .get(&tick)
            .map(|indices| indices.iter().map(|&i| &self.events[i]).collect())
            .unwrap_or_default()
    }

    /// Clear events from a specific tick onwards (for checkpoint restoration)
    pub fn clear_from_tick(&mut self, tick: u64) {
        // Remove events from the specified tick onwards
        let mut removal_indices = Vec::new();
        for (i, event) in self.events.iter().enumerate() {
            if event.tick >= tick {
                removal_indices.push(i);
            }
        }

        // Remove in reverse order to maintain indices
        for &i in removal_indices.iter().rev() {
            self.events.remove(i);
        }

        // Rebuild indices
        self.rebuild_indices();

        // Remove checkpoints and violations from that tick onwards
        self.checkpoints.retain(|cp| cp.tick < tick);
        self.violations.retain(|v| v.violation_state.tick < tick);

        // Update metadata
        self.metadata.total_ticks = self.events.last().map(|e| e.tick).unwrap_or(0);
        // TODO: Fix metadata violations field mismatch
        // self.metadata.violations = self.violations.clone();
    }

    /// Set trace metadata
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

    /// Add property check
    pub fn add_property_check(&mut self, property: String) {
        if !self.metadata.properties_checked.contains(&property) {
            self.metadata.properties_checked.push(property);
        }
    }

    /// Export simple simulation trace without external dependencies
    pub fn export_simple(&self, current_tick: u64) -> SimulationTrace {
        // Create final metadata
        let mut final_metadata = self.metadata.clone();
        final_metadata.total_ticks = current_tick;

        // Empty participants and network topology for simplified version
        let participant_info = HashMap::new();
        let network_topology = NetworkTopology {
            nodes: HashMap::new(),
            edges: Vec::new(),
            partitions: Vec::new(),
        };

        SimulationTrace {
            metadata: final_metadata,
            timeline: self.events.clone(),
            checkpoints: self.checkpoints.clone(),
            participants: participant_info,
            network_topology,
        }
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

    /// Get participant count
    pub fn participant_count(&self) -> usize {
        self.participant_index.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    // Private helper methods

    /// Rebuild all indices after event removal
    fn rebuild_indices(&mut self) {
        self.participant_index.clear();
        self.tick_index.clear();

        for (i, event) in self.events.iter().enumerate() {
            self.participant_index
                .entry(event.participant.clone())
                .or_default()
                .push(i);

            self.tick_index.entry(event.tick).or_default().push(i);
        }
    }
}

impl Default for TraceRecorder {
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
    fn test_trace_recorder_creation() {
        let recorder = TraceRecorder::new();
        assert!(recorder.is_empty());
        assert_eq!(recorder.event_count(), 0);
        assert_eq!(recorder.participant_count(), 0);
    }

    #[test]
    fn test_event_recording() {
        let mut recorder = TraceRecorder::new();

        let event1 = create_test_event(0, 0, "participant_1");
        let event2 = create_test_event(1, 1, "participant_2");

        recorder.record(event1);
        recorder.record(event2);

        assert_eq!(recorder.event_count(), 2);
        assert_eq!(recorder.participant_count(), 2);
    }

    #[test]
    fn test_participant_indexing() {
        let mut recorder = TraceRecorder::new();

        recorder.record(create_test_event(0, 0, "alice"));
        recorder.record(create_test_event(1, 1, "bob"));
        recorder.record(create_test_event(2, 2, "alice"));

        let alice_events = recorder.get_events_by_participant("alice");
        assert_eq!(alice_events.len(), 2);
        assert_eq!(alice_events[0].tick, 0);
        assert_eq!(alice_events[1].tick, 2);

        let bob_events = recorder.get_events_by_participant("bob");
        assert_eq!(bob_events.len(), 1);
        assert_eq!(bob_events[0].tick, 1);
    }

    #[test]
    fn test_tick_range_queries() {
        let mut recorder = TraceRecorder::new();

        recorder.record(create_test_event(0, 0, "alice"));
        recorder.record(create_test_event(2, 1, "bob"));
        recorder.record(create_test_event(5, 2, "alice"));

        let range_events = recorder.get_events_in_range(1, 3);
        assert_eq!(range_events.len(), 1);
        assert_eq!(range_events[0].tick, 2);

        let all_range = recorder.get_events_in_range(0, 10);
        assert_eq!(all_range.len(), 3);
    }

    #[test]
    fn test_clear_from_tick() {
        let mut recorder = TraceRecorder::new();

        recorder.record(create_test_event(0, 0, "alice"));
        recorder.record(create_test_event(1, 1, "bob"));
        recorder.record(create_test_event(2, 2, "alice"));

        recorder.clear_from_tick(1);

        assert_eq!(recorder.event_count(), 1);
        let remaining_events = recorder.get_all_events();
        assert_eq!(remaining_events[0].tick, 0);
    }

    #[test]
    fn test_property_violation_tracking() {
        let mut recorder = TraceRecorder::new();

        let violation_event = TraceEvent {
            tick: 5,
            event_id: 0,
            event_type: EventType::PropertyViolation {
                property: "safety".to_string(),
                violation_details: "Conflicting commits detected".to_string(),
            },
            participant: "alice".to_string(),
            causality: CausalityInfo {
                parent_events: vec![],
                happens_before: vec![],
                concurrent_with: vec![],
            },
        };

        recorder.record(violation_event);

        assert_eq!(recorder.violations().len(), 1);
        assert_eq!(recorder.violations()[0].property_name, "safety");
        assert_eq!(recorder.violations()[0].violation_state.tick, 5);
    }
}
