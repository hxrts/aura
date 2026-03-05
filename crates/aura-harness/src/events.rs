//! Event stream for harness execution tracing.
//!
//! Collects structured events during test execution including instance operations,
//! state transitions, and assertion results for post-run analysis and debugging.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessEvent {
    pub event_id: u64,
    pub event_type: String,
    pub operation: String,
    pub instance_id: Option<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct EventStream {
    next_event_id: Arc<AtomicU64>,
    events: Arc<Mutex<Vec<HarnessEvent>>>,
}

impl EventStream {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(
        &self,
        event_type: impl Into<String>,
        operation: impl Into<String>,
        instance_id: Option<String>,
        details: serde_json::Value,
    ) {
        let event_id = self.next_event_id.fetch_add(1, Ordering::Relaxed) + 1;
        let event = HarnessEvent {
            event_id,
            event_type: event_type.into(),
            operation: operation.into(),
            instance_id,
            details,
        };
        self.events.blocking_lock().push(event);
    }

    pub fn snapshot(&self) -> Vec<HarnessEvent> {
        self.events.blocking_lock().clone()
    }
}
