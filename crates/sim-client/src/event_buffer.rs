//! Event buffering and management for efficient trace event handling

use aura_console_types::TraceEvent;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use wasm_core::console_log;

/// Maximum number of events to buffer
const MAX_BUFFER_SIZE: usize = 10000;

/// Event buffer statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferStats {
    pub total_events: usize,
    pub buffer_size: usize,
    pub oldest_event_id: Option<u64>,
    pub newest_event_id: Option<u64>,
    pub memory_usage_estimate: usize,
}

/// Efficient event buffer for trace events
#[derive(Debug, Clone)]
pub struct EventBuffer {
    /// Circular buffer of events
    events: VecDeque<TraceEvent>,
    /// Total number of events received (for statistics)
    total_events_received: usize,
    /// Number of events dropped due to buffer overflow
    events_dropped: usize,
}

impl EventBuffer {
    /// Create a new event buffer
    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(MAX_BUFFER_SIZE),
            total_events_received: 0,
            events_dropped: 0,
        }
    }

    /// Add a new event to the buffer
    pub fn add_event(&mut self, event: TraceEvent) {
        self.total_events_received += 1;

        // If buffer is full, remove oldest event
        if self.events.len() >= MAX_BUFFER_SIZE {
            self.events.pop_front();
            self.events_dropped += 1;
        }

        self.events.push_back(event);
    }

    /// Get events since a specific event ID
    pub fn get_events_since(&self, since_event_id: Option<u64>) -> Vec<TraceEvent> {
        match since_event_id {
            None => self.events.iter().cloned().collect(),
            Some(since_id) => self
                .events
                .iter()
                .filter(|event| event.event_id > since_id)
                .cloned()
                .collect(),
        }
    }

    /// Get the last N events
    pub fn get_last_events(&self, count: usize) -> Vec<TraceEvent> {
        let start_index = if self.events.len() > count {
            self.events.len() - count
        } else {
            0
        };

        self.events.range(start_index..).cloned().collect()
    }

    /// Get events in a specific tick range
    pub fn get_events_in_tick_range(&self, start_tick: u64, end_tick: u64) -> Vec<TraceEvent> {
        self.events
            .iter()
            .filter(|event| event.tick >= start_tick && event.tick <= end_tick)
            .cloned()
            .collect()
    }

    /// Get events by participant
    pub fn get_events_by_participant(&self, participant: &str) -> Vec<TraceEvent> {
        self.events
            .iter()
            .filter(|event| event.participant == participant)
            .cloned()
            .collect()
    }

    /// Get buffer statistics
    pub fn get_stats(&self) -> BufferStats {
        let oldest_event_id = self.events.front().map(|e| e.event_id);
        let newest_event_id = self.events.back().map(|e| e.event_id);

        // Rough memory usage estimate (each event ~1KB on average)
        let memory_usage_estimate = self.events.len() * 1024;

        BufferStats {
            total_events: self.total_events_received,
            buffer_size: self.events.len(),
            oldest_event_id,
            newest_event_id,
            memory_usage_estimate,
        }
    }

    /// Clear all events from the buffer
    pub fn clear(&mut self) {
        self.events.clear();
        console_log!(
            "Event buffer cleared - {} events dropped",
            self.events_dropped
        );
    }

    /// Get the current buffer size
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get the number of events dropped due to overflow
    pub fn dropped_count(&self) -> usize {
        self.events_dropped
    }

    /// Compact the buffer by removing events older than a certain tick
    pub fn compact_before_tick(&mut self, tick_threshold: u64) {
        let original_len = self.events.len();

        self.events.retain(|event| event.tick >= tick_threshold);

        let removed = original_len - self.events.len();
        if removed > 0 {
            console_log!("Compacted buffer: removed {} old events", removed);
        }
    }

    /// Get events matching a filter predicate
    pub fn get_filtered_events<F>(&self, filter: F) -> Vec<TraceEvent>
    where
        F: Fn(&TraceEvent) -> bool,
    {
        self.events.iter().filter(|e| filter(e)).cloned().collect()
    }

    /// Get event count by participant
    pub fn get_participant_event_counts(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();

        for event in &self.events {
            *counts.entry(event.participant.clone()).or_insert(0) += 1;
        }

        counts
    }

    /// Get events grouped by tick
    pub fn get_events_by_tick(&self) -> std::collections::BTreeMap<u64, Vec<TraceEvent>> {
        let mut by_tick = std::collections::BTreeMap::new();

        for event in &self.events {
            by_tick
                .entry(event.tick)
                .or_insert_with(Vec::new)
                .push(event.clone());
        }

        by_tick
    }

    /// Get the tick range of buffered events
    pub fn get_tick_range(&self) -> Option<(u64, u64)> {
        if self.events.is_empty() {
            return None;
        }

        let min_tick = self.events.iter().map(|e| e.tick).min()?;
        let max_tick = self.events.iter().map(|e| e.tick).max()?;
        Some((min_tick, max_tick))
    }
}

impl Default for EventBuffer {
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
    fn test_event_buffer_creation() {
        let buffer = EventBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_add_events() {
        let mut buffer = EventBuffer::new();

        let event1 = create_test_event(0, 1, "alice");
        let event2 = create_test_event(1, 2, "bob");

        buffer.add_event(event1);
        buffer.add_event(event2);

        assert_eq!(buffer.len(), 2);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_get_events_since() {
        let mut buffer = EventBuffer::new();

        buffer.add_event(create_test_event(0, 1, "alice"));
        buffer.add_event(create_test_event(1, 2, "bob"));
        buffer.add_event(create_test_event(2, 3, "alice"));

        let events_since_1 = buffer.get_events_since(Some(1));
        assert_eq!(events_since_1.len(), 2);
        assert_eq!(events_since_1[0].event_id, 2);
        assert_eq!(events_since_1[1].event_id, 3);

        let all_events = buffer.get_events_since(None);
        assert_eq!(all_events.len(), 3);
    }

    #[test]
    fn test_get_events_by_participant() {
        let mut buffer = EventBuffer::new();

        buffer.add_event(create_test_event(0, 1, "alice"));
        buffer.add_event(create_test_event(1, 2, "bob"));
        buffer.add_event(create_test_event(2, 3, "alice"));

        let alice_events = buffer.get_events_by_participant("alice");
        assert_eq!(alice_events.len(), 2);

        let bob_events = buffer.get_events_by_participant("bob");
        assert_eq!(bob_events.len(), 1);
    }

    #[test]
    fn test_buffer_overflow() {
        let mut buffer = EventBuffer::new();

        // Add more events than buffer capacity
        for i in 0..(MAX_BUFFER_SIZE + 100) {
            buffer.add_event(create_test_event(i as u64, i as u64, "test"));
        }

        assert_eq!(buffer.len(), MAX_BUFFER_SIZE);
        assert_eq!(buffer.dropped_count(), 100);

        let stats = buffer.get_stats();
        assert_eq!(stats.buffer_size, MAX_BUFFER_SIZE);
        assert_eq!(stats.total_events, MAX_BUFFER_SIZE + 100);
    }

    #[test]
    fn test_tick_range() {
        let mut buffer = EventBuffer::new();

        buffer.add_event(create_test_event(5, 1, "alice"));
        buffer.add_event(create_test_event(2, 2, "bob"));
        buffer.add_event(create_test_event(8, 3, "alice"));

        let (min_tick, max_tick) = buffer.get_tick_range().unwrap();
        assert_eq!(min_tick, 2);
        assert_eq!(max_tick, 8);
    }

    #[test]
    fn test_events_in_tick_range() {
        let mut buffer = EventBuffer::new();

        buffer.add_event(create_test_event(1, 1, "alice"));
        buffer.add_event(create_test_event(3, 2, "bob"));
        buffer.add_event(create_test_event(5, 3, "alice"));
        buffer.add_event(create_test_event(7, 4, "bob"));

        let events_2_to_5 = buffer.get_events_in_tick_range(2, 5);
        assert_eq!(events_2_to_5.len(), 2);
        assert_eq!(events_2_to_5[0].tick, 3);
        assert_eq!(events_2_to_5[1].tick, 5);
    }
}
