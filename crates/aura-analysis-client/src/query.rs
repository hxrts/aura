//! Efficient querying interface for trace analysis

use aura_console_types::{EventType, TraceEvent};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::console_log;

/// Index for efficient trace querying
#[derive(Debug, Clone)]
pub struct TraceIndex {
    /// Events indexed by tick
    by_tick: BTreeMap<u64, Vec<TraceEvent>>,
    /// Events indexed by participant
    by_participant: IndexMap<String, Vec<TraceEvent>>,
    /// Events indexed by event ID
    by_event_id: HashMap<u64, TraceEvent>,
    /// Events indexed by event type
    by_event_type: IndexMap<String, Vec<TraceEvent>>,
    /// Tick range for fast range queries
    tick_range: Option<(u64, u64)>,
}

/// Query result statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryStats {
    pub total_events: usize,
    pub matched_events: usize,
    pub query_time_ms: f64,
    pub tick_range: Option<(u64, u64)>,
}

/// Query filter for event selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
    pub tick_range: Option<(u64, u64)>,
    pub participants: Option<Vec<String>>,
    pub event_types: Option<Vec<String>>,
    pub event_ids: Option<Vec<u64>>,
    pub limit: Option<usize>,
}

impl TraceIndex {
    /// Build a new trace index from events
    pub fn build(events: &[TraceEvent]) -> Self {
        console_log!("Building trace index for {} events", events.len());

        let mut by_tick = BTreeMap::new();
        let mut by_participant = IndexMap::new();
        let mut by_event_id = HashMap::new();
        let mut by_event_type = IndexMap::new();

        let mut min_tick = u64::MAX;
        let mut max_tick = 0;

        for event in events {
            // Index by tick
            by_tick
                .entry(event.tick)
                .or_insert_with(Vec::new)
                .push(event.clone());

            // Index by participant
            by_participant
                .entry(event.participant.clone())
                .or_insert_with(Vec::new)
                .push(event.clone());

            // Index by event ID
            by_event_id.insert(event.event_id, event.clone());

            // Index by event type
            let event_type_str = match &event.event_type {
                EventType::EffectExecuted { effect_type, .. } => {
                    format!("EffectExecuted::{}", effect_type)
                }
                EventType::MessageSent { message_type, .. } => {
                    format!("MessageSent::{}", message_type)
                }
                EventType::MessageReceived { message_type, .. } => {
                    format!("MessageReceived::{}", message_type)
                }
                EventType::ProtocolStateTransition {
                    from_state,
                    to_state,
                    ..
                } => format!("StateTransition::{}->{}", from_state, to_state),
                EventType::MessageDropped { reason, .. } => format!("MessageDropped::{:?}", reason),
                EventType::CrdtMerge { from_replica, .. } => format!("CrdtMerge::{}", from_replica),
                EventType::CheckpointCreated { label, .. } => format!("Checkpoint::{}", label),
                EventType::PropertyViolation { property, .. } => {
                    format!("PropertyViolation::{}", property)
                }
            };

            by_event_type
                .entry(event_type_str)
                .or_insert_with(Vec::new)
                .push(event.clone());

            // Update tick range
            min_tick = min_tick.min(event.tick);
            max_tick = max_tick.max(event.tick);
        }

        let tick_range = if events.is_empty() {
            None
        } else {
            Some((min_tick, max_tick))
        };

        console_log!(
            "Built trace index: {} ticks, {} participants, {} event types",
            by_tick.len(),
            by_participant.len(),
            by_event_type.len()
        );

        TraceIndex {
            by_tick,
            by_participant,
            by_event_id,
            by_event_type,
            tick_range,
        }
    }

    /// Query events in a specific tick range
    pub fn query_range(&self, start_tick: u64, end_tick: u64) -> Vec<TraceEvent> {
        let mut results = Vec::new();

        for (_tick, events) in self.by_tick.range(start_tick..=end_tick) {
            results.extend_from_slice(events);
        }

        // Sort by event_id to ensure deterministic ordering
        results.sort_by_key(|e| e.event_id);
        results
    }

    /// Query events by participant
    pub fn query_by_participant(&self, participant: &str) -> Vec<TraceEvent> {
        self.by_participant
            .get(participant).cloned()
            .unwrap_or_default()
    }

    /// Query events by event type pattern
    pub fn query_by_event_type(&self, pattern: &str) -> Vec<TraceEvent> {
        let mut results = Vec::new();

        for (event_type, events) in &self.by_event_type {
            if event_type.contains(pattern) {
                results.extend_from_slice(events);
            }
        }

        // Sort by event_id to ensure deterministic ordering
        results.sort_by_key(|e| e.event_id);
        results
    }

    /// Get event by ID
    pub fn get_event(&self, event_id: u64) -> Option<&TraceEvent> {
        self.by_event_id.get(&event_id)
    }

    /// Execute a complex filter query
    pub fn query(&self, filter: &EventFilter) -> (Vec<TraceEvent>, QueryStats) {
        let start_time = js_sys::Date::now();
        let mut candidates = Vec::new();

        // Start with all events or apply initial filters
        if let Some((start, end)) = filter.tick_range {
            candidates = self.query_range(start, end);
        } else if let Some(ref participants) = filter.participants {
            for participant in participants {
                candidates.extend(self.query_by_participant(participant));
            }
        } else if let Some(ref event_types) = filter.event_types {
            for event_type in event_types {
                candidates.extend(self.query_by_event_type(event_type));
            }
        } else if let Some(ref event_ids) = filter.event_ids {
            for &event_id in event_ids {
                if let Some(event) = self.get_event(event_id) {
                    candidates.push(event.clone());
                }
            }
        } else {
            // No filters - return all events
            candidates = self.by_event_id.values().cloned().collect();
        }

        // Apply additional filters
        candidates.retain(|event| {
            // Tick range filter
            if let Some((start, end)) = filter.tick_range {
                if event.tick < start || event.tick > end {
                    return false;
                }
            }

            // Participant filter
            if let Some(ref participants) = filter.participants {
                if !participants.contains(&event.participant) {
                    return false;
                }
            }

            // Event type filter
            if let Some(ref event_types) = filter.event_types {
                let event_type_str = match &event.event_type {
                    EventType::EffectExecuted { effect_type, .. } => {
                        format!("EffectExecuted::{}", effect_type)
                    }
                    EventType::MessageSent { message_type, .. } => {
                        format!("MessageSent::{}", message_type)
                    }
                    EventType::MessageReceived { message_type, .. } => {
                        format!("MessageReceived::{}", message_type)
                    }
                    EventType::ProtocolStateTransition {
                        from_state,
                        to_state,
                        ..
                    } => format!("StateTransition::{}->{}", from_state, to_state),
                    EventType::MessageDropped { reason, .. } => {
                        format!("MessageDropped::{:?}", reason)
                    }
                    EventType::CrdtMerge { from_replica, .. } => {
                        format!("CrdtMerge::{}", from_replica)
                    }
                    EventType::CheckpointCreated { label, .. } => format!("Checkpoint::{}", label),
                    EventType::PropertyViolation { property, .. } => {
                        format!("PropertyViolation::{}", property)
                    }
                };

                if !event_types
                    .iter()
                    .any(|pattern| event_type_str.contains(pattern))
                {
                    return false;
                }
            }

            // Event ID filter
            if let Some(ref event_ids) = filter.event_ids {
                if !event_ids.contains(&event.event_id) {
                    return false;
                }
            }

            true
        });

        // Sort by event_id for deterministic results
        candidates.sort_by_key(|e| e.event_id);

        // Apply limit
        if let Some(limit) = filter.limit {
            candidates.truncate(limit);
        }

        let query_time_ms = js_sys::Date::now() - start_time;

        let stats = QueryStats {
            total_events: self.by_event_id.len(),
            matched_events: candidates.len(),
            query_time_ms,
            tick_range: self.tick_range,
        };

        (candidates, stats)
    }

    /// Get summary statistics about the trace
    pub fn get_summary(&self) -> TraceSummary {
        let mut protocol_counts = HashMap::new();
        let mut participant_activity = HashMap::new();

        for event in self.by_event_id.values() {
            // Count protocol activities
            let protocol = match &event.event_type {
                EventType::ProtocolStateTransition { protocol, .. } => Some(protocol.clone()),
                _ => None,
            };

            if let Some(protocol) = protocol {
                *protocol_counts.entry(protocol).or_insert(0) += 1;
            }

            // Count participant activity
            *participant_activity
                .entry(event.participant.clone())
                .or_insert(0) += 1;
        }

        TraceSummary {
            total_events: self.by_event_id.len(),
            tick_range: self.tick_range,
            participant_count: self.by_participant.len(),
            event_type_count: self.by_event_type.len(),
            protocol_counts,
            participant_activity,
        }
    }

    /// Get list of all participants
    pub fn get_participants(&self) -> Vec<String> {
        self.by_participant.keys().cloned().collect()
    }

    /// Get list of all event types
    pub fn get_event_types(&self) -> Vec<String> {
        self.by_event_type.keys().cloned().collect()
    }
}

/// Summary statistics about a trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub total_events: usize,
    pub tick_range: Option<(u64, u64)>,
    pub participant_count: usize,
    pub event_type_count: usize,
    pub protocol_counts: HashMap<String, usize>,
    pub participant_activity: HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_console_types::CausalityInfo;
    use std::collections::HashMap;

    fn create_test_event(
        tick: u64,
        event_id: u64,
        participant: &str,
        effect_type: &str,
    ) -> TraceEvent {
        TraceEvent {
            tick,
            event_id,
            event_type: EventType::EffectExecuted {
                effect_type: effect_type.to_string(),
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
    fn test_trace_index_build() {
        let events = vec![
            create_test_event(0, 1, "alice", "test1"),
            create_test_event(1, 2, "bob", "test2"),
            create_test_event(2, 3, "alice", "test1"),
        ];

        let index = TraceIndex::build(&events);

        assert_eq!(index.by_event_id.len(), 3);
        assert_eq!(index.by_participant.len(), 2);
        assert_eq!(index.tick_range, Some((0, 2)));
    }

    #[test]
    fn test_query_range() {
        let events = vec![
            create_test_event(0, 1, "alice", "test1"),
            create_test_event(1, 2, "bob", "test2"),
            create_test_event(2, 3, "alice", "test1"),
            create_test_event(5, 4, "bob", "test3"),
        ];

        let index = TraceIndex::build(&events);
        let results = index.query_range(1, 2);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].event_id, 2);
        assert_eq!(results[1].event_id, 3);
    }

    #[test]
    fn test_query_by_participant() {
        let events = vec![
            create_test_event(0, 1, "alice", "test1"),
            create_test_event(1, 2, "bob", "test2"),
            create_test_event(2, 3, "alice", "test1"),
        ];

        let index = TraceIndex::build(&events);
        let alice_events = index.query_by_participant("alice");

        assert_eq!(alice_events.len(), 2);
        assert_eq!(alice_events[0].participant, "alice");
        assert_eq!(alice_events[1].participant, "alice");
    }
}
