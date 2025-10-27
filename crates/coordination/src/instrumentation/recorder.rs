//! Trace Event Recorder
//!
//! Records protocol execution events for dev console visualization.

use super::events::ConsoleEvent;
use DeviceId;
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Records trace events for dev console integration
pub struct TraceRecorder {
    /// Buffer of recorded events
    events: VecDeque<ConsoleEvent>,
    /// Maximum number of events to buffer
    max_events: usize,
    /// Next event ID
    next_event_id: u64,
    /// Recording enabled flag
    enabled: bool,
}

impl TraceRecorder {
    /// Create a new trace recorder
    pub fn new() -> Self {
        Self {
            events: VecDeque::new(),
            max_events: 10000, // Reasonable default for debugging
            next_event_id: 1,
            enabled: false,
        }
    }

    /// Create a trace recorder with custom buffer size
    pub fn with_capacity(max_events: usize) -> Self {
        Self {
            events: VecDeque::new(),
            max_events,
            next_event_id: 1,
            enabled: false,
        }
    }

    /// Enable event recording
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable event recording
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Check if recording is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Record a protocol event
    pub fn record_protocol_event(
        &mut self,
        session_id: Uuid,
        device_id: DeviceId,
        event_type: &str,
        details: Option<serde_json::Value>,
    ) {
        if !self.enabled {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = ConsoleEvent::Protocol {
            event_id: self.next_event_id,
            timestamp,
            session_id,
            device_id,
            event_type: event_type.to_string(),
            details,
        };

        self.record_event(event);
    }

    /// Record a state change event
    pub fn record_state_change(
        &mut self,
        device_id: DeviceId,
        state_type: &str,
        old_state: Option<serde_json::Value>,
        new_state: serde_json::Value,
    ) {
        if !self.enabled {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = ConsoleEvent::StateChange {
            event_id: self.next_event_id,
            timestamp,
            device_id,
            state_type: state_type.to_string(),
            old_state,
            new_state,
        };

        self.record_event(event);
    }

    /// Record a network message event
    pub fn record_network_event(
        &mut self,
        from_device: DeviceId,
        to_device: DeviceId,
        message_type: &str,
        size_bytes: usize,
    ) {
        if !self.enabled {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = ConsoleEvent::Network {
            event_id: self.next_event_id,
            timestamp,
            from_device,
            to_device,
            message_type: message_type.to_string(),
            size_bytes,
        };

        self.record_event(event);
    }

    /// Record an error event
    pub fn record_error(
        &mut self,
        device_id: DeviceId,
        error_type: &str,
        error_message: &str,
        context: Option<serde_json::Value>,
    ) {
        if !self.enabled {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = ConsoleEvent::Error {
            event_id: self.next_event_id,
            timestamp,
            device_id,
            error_type: error_type.to_string(),
            error_message: error_message.to_string(),
            context,
        };

        self.record_event(event);
    }

    /// Internal method to record an event
    fn record_event(&mut self, event: ConsoleEvent) {
        // Add event to buffer
        self.events.push_back(event);
        self.next_event_id += 1;

        // Maintain buffer size limit
        while self.events.len() > self.max_events {
            self.events.pop_front();
        }
    }

    /// Get all recorded events
    pub fn get_events(&self) -> Vec<ConsoleEvent> {
        self.events.iter().cloned().collect()
    }

    /// Get events since a specific event ID
    pub fn get_events_since(&self, since_event_id: u64) -> Vec<ConsoleEvent> {
        self.events
            .iter()
            .filter(|event| event.event_id() > since_event_id)
            .cloned()
            .collect()
    }

    /// Get the most recent N events
    pub fn get_recent_events(&self, count: usize) -> Vec<ConsoleEvent> {
        self.events
            .iter()
            .rev()
            .take(count)
            .rev()
            .cloned()
            .collect()
    }

    /// Clear all recorded events
    pub fn clear(&mut self) {
        self.events.clear();
        self.next_event_id = 1;
    }

    /// Export trace data for dev console
    pub fn export_trace(&self) -> Vec<u8> {
        // Serialize events to binary format for efficient transfer
        match postcard::to_allocvec(&self.get_events()) {
            Ok(data) => data,
            Err(_) => {
                // Fallback to JSON if postcard fails
                serde_json::to_vec(&self.get_events()).unwrap_or_default()
            }
        }
    }

    /// Get current buffer statistics
    pub fn stats(&self) -> RecorderStats {
        RecorderStats {
            total_events: self.events.len(),
            max_capacity: self.max_events,
            next_event_id: self.next_event_id,
            enabled: self.enabled,
        }
    }
}

impl Default for TraceRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the trace recorder
#[derive(Debug, Clone)]
pub struct RecorderStats {
    pub total_events: usize,
    pub max_capacity: usize,
    pub next_event_id: u64,
    pub enabled: bool,
}
