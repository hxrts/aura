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
use tracing::{debug, info, trace};

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
#[derive(Clone, Debug)]
pub enum EventFilter {
    /// Match any event
    Any,
    /// Match specific event type variant
    Type(EventTypeFilter),
    /// Match session ID
    SessionId(uuid::Uuid),
}

#[derive(Clone, Debug)]
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
        info!("Creating new event watcher with 100ms poll interval");
        EventWatcher {
            ledger,
            last_processed_index: 0,
            callbacks: Vec::new(),
            poll_interval: Duration::from_millis(100), // 10 Hz
        }
    }

    /// Register a callback for specific events
    pub fn register(&mut self, filter: EventFilter, callback: EventCallback) {
        debug!("Registering event callback for filter: {:?}", filter);
        self.callbacks.push((filter, callback));
    }

    /// Start watching for events
    ///
    /// This runs in a loop, polling the ledger and invoking callbacks.
    /// Returns when stopped.
    pub async fn watch(&mut self) {
        info!(
            "Starting event watcher with {} registered callbacks",
            self.callbacks.len()
        );
        let mut interval = interval(self.poll_interval);
        let mut poll_count = 0;

        loop {
            interval.tick().await;
            poll_count += 1;

            // Trace poll activity every 100 polls to avoid spam
            if poll_count % 100 == 0 {
                trace!(
                    "Event watcher poll #{}, last_processed_index: {}",
                    poll_count,
                    self.last_processed_index
                );
            }

            // Read new events
            let events = {
                let ledger = self.ledger.read().await;
                let event_log = ledger.event_log();

                if event_log.len() <= self.last_processed_index {
                    // No new events
                    continue;
                }

                debug!(
                    "Found {} new events to process (total events: {}, last processed: {})",
                    event_log.len() - self.last_processed_index,
                    event_log.len(),
                    self.last_processed_index
                );

                // Get new events
                event_log[self.last_processed_index..].to_vec()
            };

            // Process new events
            for (i, event) in events.iter().enumerate() {
                debug!(
                    "Processing event {}/{}: {:?}",
                    i + 1,
                    events.len(),
                    event.event_type
                );
                self.process_event(event);
            }

            // Update last processed index
            self.last_processed_index += events.len();

            if !events.is_empty() {
                info!(
                    "Processed {} events, updated last_processed_index to {}",
                    events.len(),
                    self.last_processed_index
                );
            }
        }
    }

    /// Process a single event through all callbacks
    fn process_event(&self, event: &Event) {
        let mut matched_callbacks = 0;

        for (i, (filter, callback)) in self.callbacks.iter().enumerate() {
            if self.matches_filter(event, filter) {
                debug!(
                    "Event matches filter #{}, invoking callback for event: {:?}",
                    i, event.event_id
                );
                matched_callbacks += 1;

                let should_continue = callback(event);
                if !should_continue {
                    debug!("Callback #{} returned false, stopping event processing", i);
                    break;
                }
            }
        }

        if matched_callbacks == 0 {
            trace!("Event {:?} matched no registered callbacks", event.event_id);
        } else {
            debug!(
                "Event {:?} matched {} callbacks",
                event.event_id, matched_callbacks
            );
        }
    }

    /// Check if event matches filter
    fn matches_filter(&self, event: &Event, filter: &EventFilter) -> bool {
        let matches = match filter {
            EventFilter::Any => true,
            EventFilter::Type(type_filter) => self.matches_type(event, type_filter),
            EventFilter::SessionId(session_id) => self.matches_session(event, session_id),
        };

        if matches {
            trace!("Event {:?} matches filter {:?}", event.event_id, filter);
        }

        matches
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
        let extracted_session_id = match &event.event_type {
            EventType::InitiateDkdSession(e) => Some(&e.session_id),
            EventType::RecordDkdCommitment(e) => Some(&e.session_id),
            EventType::RevealDkdPoint(e) => Some(&e.session_id),
            EventType::FinalizeDkdSession(e) => Some(&e.session_id),
            EventType::AbortDkdSession(e) => Some(&e.session_id),
            EventType::GrantOperationLock(e) => Some(&e.session_id),
            EventType::ReleaseOperationLock(e) => Some(&e.session_id),
            _ => None,
        };

        match extracted_session_id {
            Some(extracted_id) => {
                let matches = extracted_id == session_id;
                if !matches {
                    trace!(
                        "Event session ID {:?} does not match target session ID {:?}",
                        extracted_id,
                        session_id
                    );
                }
                matches
            }
            None => {
                trace!("Event {:?} does not contain a session ID", event.event_type);
                false
            }
        }
    }
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
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
