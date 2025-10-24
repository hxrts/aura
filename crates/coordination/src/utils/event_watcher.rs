// Event watcher - monitors CRDT ledger for events and triggers orchestrator actions
//
// Reference: 080_architecture_protocol_integration.md - Part 3: CRDT Choreography
//
// The event watcher polls the ledger for new events and notifies the orchestrator
// when relevant events occur (e.g., lock grants, peer commitments, reveals).
// This enables reactive protocol progression based on CRDT state.

use aura_journal::{AccountLedger, Event, EventType};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
// TODO: Add debug and warn logging once event watcher is fully implemented
// use tracing::{debug, warn};

/// Callback for event notifications
pub type EventCallback = Arc<dyn Fn(&Event) -> bool + Send + Sync>;

/// Event watcher - monitors ledger for new events
///
/// Polls the ledger at regular intervals and invokes callbacks
/// when matching events are found.
pub struct EventWatcher {
    /// Ledger to watch
    ledger: Arc<RwLock<AccountLedger>>,

    /// Last processed event index
    last_processed_index: usize,

    /// Registered callbacks for specific event types
    callbacks: Vec<(EventFilter, EventCallback)>,

    /// Polling interval
    poll_interval: Duration,
}

/// Filter for event types
#[derive(Clone)]
pub enum EventFilter {
    /// Match any event
    Any,
    /// Match specific event type variant
    Type(EventTypeFilter),
    /// Match session ID
    SessionId(uuid::Uuid),
}

#[derive(Clone)]
pub enum EventTypeFilter {
    GrantOperationLock,
    RecordDkdCommitment,
    RevealDkdPoint,
    FinalizeDkdSession,
    AbortDkdSession,
}

impl EventWatcher {
    /// Create a new event watcher
    pub fn new(ledger: Arc<RwLock<AccountLedger>>) -> Self {
        EventWatcher {
            ledger,
            last_processed_index: 0,
            callbacks: Vec::new(),
            poll_interval: Duration::from_millis(100), // 10 Hz
        }
    }

    /// Register a callback for specific events
    pub fn register(&mut self, filter: EventFilter, callback: EventCallback) {
        self.callbacks.push((filter, callback));
    }

    /// Start watching for events
    ///
    /// This runs in a loop, polling the ledger and invoking callbacks.
    /// Returns when stopped.
    pub async fn watch(&mut self) {
        let mut interval = interval(self.poll_interval);

        loop {
            interval.tick().await;

            // Read new events
            let events = {
                let ledger = self.ledger.read().await;
                let event_log = ledger.event_log();

                if event_log.len() <= self.last_processed_index {
                    // No new events
                    continue;
                }

                // Get new events
                event_log[self.last_processed_index..].to_vec()
            };

            // Process new events
            for event in &events {
                self.process_event(event);
            }

            // Update last processed index
            self.last_processed_index += events.len();
        }
    }

    /// Process a single event through all callbacks
    fn process_event(&self, event: &Event) {
        for (filter, callback) in &self.callbacks {
            if self.matches_filter(event, filter) {
                let should_continue = callback(event);
                if !should_continue {
                    // Callback returned false, stop processing this event
                    break;
                }
            }
        }
    }

    /// Check if event matches filter
    fn matches_filter(&self, event: &Event, filter: &EventFilter) -> bool {
        match filter {
            EventFilter::Any => true,
            EventFilter::Type(type_filter) => self.matches_type(event, type_filter),
            EventFilter::SessionId(session_id) => self.matches_session(event, session_id),
        }
    }

    /// Check if event matches type filter
    fn matches_type(&self, event: &Event, type_filter: &EventTypeFilter) -> bool {
        matches!(
            (&event.event_type, type_filter),
            (
                EventType::GrantOperationLock(_),
                EventTypeFilter::GrantOperationLock
            ) | (
                EventType::RecordDkdCommitment(_),
                EventTypeFilter::RecordDkdCommitment
            ) | (
                EventType::RevealDkdPoint(_),
                EventTypeFilter::RevealDkdPoint
            ) | (
                EventType::FinalizeDkdSession(_),
                EventTypeFilter::FinalizeDkdSession
            ) | (
                EventType::AbortDkdSession(_),
                EventTypeFilter::AbortDkdSession
            )
        )
    }

    /// Check if event matches session ID
    fn matches_session(&self, event: &Event, session_id: &uuid::Uuid) -> bool {
        match &event.event_type {
            EventType::InitiateDkdSession(e) => &e.session_id == session_id,
            EventType::RecordDkdCommitment(e) => &e.session_id == session_id,
            EventType::RevealDkdPoint(e) => &e.session_id == session_id,
            EventType::FinalizeDkdSession(e) => &e.session_id == session_id,
            EventType::AbortDkdSession(e) => &e.session_id == session_id,
            EventType::GrantOperationLock(e) => &e.session_id == session_id,
            EventType::ReleaseOperationLock(e) => &e.session_id == session_id,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_filter_matching() {
        // Test that filter types are correctly defined
        let _any = EventFilter::Any;
        let _type_filter = EventFilter::Type(EventTypeFilter::GrantOperationLock);
        let _session_filter = EventFilter::SessionId(uuid::Uuid::new_v4());
    }
}
