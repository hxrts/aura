//! Event stream for harness execution tracing.
//!
//! Collects structured events during test execution including instance operations,
//! state transitions, and assertion results for post-run analysis and debugging.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HarnessEventValue {
    Bool(bool),
    U64(u64),
    U128(u128),
    String(String),
    StringList(Vec<String>),
}

impl From<bool> for HarnessEventValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<u64> for HarnessEventValue {
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}

impl From<u16> for HarnessEventValue {
    fn from(value: u16) -> Self {
        Self::U64(u64::from(value))
    }
}

impl From<usize> for HarnessEventValue {
    fn from(value: usize) -> Self {
        Self::U64(value as u64)
    }
}

impl From<u128> for HarnessEventValue {
    fn from(value: u128) -> Self {
        Self::U128(value)
    }
}

impl From<String> for HarnessEventValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for HarnessEventValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<Vec<String>> for HarnessEventValue {
    fn from(value: Vec<String>) -> Self {
        Self::StringList(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HarnessEventDetails {
    pub fields: BTreeMap<String, HarnessEventValue>,
}

impl HarnessEventDetails {
    #[must_use]
    pub fn with_field(
        mut self,
        key: impl Into<String>,
        value: impl Into<HarnessEventValue>,
    ) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

#[macro_export]
macro_rules! event_details {
    () => {
        $crate::events::HarnessEventDetails::default()
    };
    ({ $($key:literal => $value:expr),+ $(,)? }) => {{
        let mut details = $crate::events::HarnessEventDetails::default();
        $(
            details = details.with_field($key, $value);
        )+
        details
    }};
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessEvent {
    pub event_id: u64,
    pub event_type: String,
    pub operation: String,
    pub instance_id: Option<String>,
    pub details: HarnessEventDetails,
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
        details: HarnessEventDetails,
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

#[cfg(test)]
mod tests {
    use super::{EventStream, HarnessEventValue};

    #[test]
    fn event_details_serialize_with_typed_scalar_fields() {
        let stream = EventStream::new();
        stream.push(
            "observation",
            "typed",
            Some("alice".to_string()),
            event_details!({
                "bytes" => 42_u64,
                "source" => "backend",
                "ok" => true
            }),
        );

        let event = stream
            .snapshot()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("expected one event"));
        assert_eq!(
            event.details.fields.get("bytes"),
            Some(&HarnessEventValue::U64(42))
        );
        assert_eq!(
            event.details.fields.get("source"),
            Some(&HarnessEventValue::String("backend".to_string()))
        );
        assert_eq!(
            event.details.fields.get("ok"),
            Some(&HarnessEventValue::Bool(true))
        );
    }
}
