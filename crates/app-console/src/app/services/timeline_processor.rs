/// Timeline Event Processing Service
///
/// Handles event conversion, filtering, sorting, and time-series data processing.
/// Pure business logic separated from visualization concerns.
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    pub timestamp: f64,
    pub event_type: String,
    pub description: String,
    pub node_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[allow(dead_code)]
pub struct TimelineProcessor;

#[allow(dead_code)]
impl TimelineProcessor {
    /// Convert a JSON envelope to a timeline event
    pub fn convert_envelope_to_event(
        envelope: &serde_json::Value,
        index: usize,
    ) -> Result<TimelineEvent, String> {
        let id = index.to_string();
        let timestamp = envelope
            .get("timestamp")
            .and_then(|t| t.as_f64())
            .unwrap_or(index as f64);

        let event_type = envelope
            .get("message_type")
            .and_then(|mt| mt.as_str())
            .unwrap_or("unknown")
            .to_string();

        let description = envelope
            .get("payload")
            .and_then(|p| p.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or(&format!("{} event occurred", event_type))
            .to_string();

        let node_id = envelope
            .get("payload")
            .and_then(|p| p.get("node_id"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());

        let metadata = envelope.get("payload").cloned();

        Ok(TimelineEvent {
            id,
            timestamp,
            event_type,
            description,
            node_id,
            metadata,
        })
    }

    /// Convert multiple envelopes to timeline events
    pub fn convert_envelopes(envelopes: &[serde_json::Value]) -> Vec<TimelineEvent> {
        envelopes
            .iter()
            .enumerate()
            .filter_map(|(idx, env)| Self::convert_envelope_to_event(env, idx).ok())
            .collect()
    }

    /// Filter events by time range
    pub fn filter_by_time_range(
        events: &[TimelineEvent],
        start: f64,
        end: f64,
    ) -> Vec<TimelineEvent> {
        events
            .iter()
            .filter(|e| e.timestamp >= start && e.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Filter events by node ID
    pub fn filter_by_node(events: &[TimelineEvent], node_id: &str) -> Vec<TimelineEvent> {
        events
            .iter()
            .filter(|e| {
                if let Some(ref nid) = e.node_id {
                    nid == node_id
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }

    /// Filter events by type
    pub fn filter_by_type(events: &[TimelineEvent], event_type: &str) -> Vec<TimelineEvent> {
        events
            .iter()
            .filter(|e| e.event_type == event_type)
            .cloned()
            .collect()
    }

    /// Sort events by timestamp (ascending)
    pub fn sort_by_timestamp(events: &mut [TimelineEvent]) {
        events.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap());
    }

    /// Get events within N seconds before a timestamp
    pub fn get_events_before(
        events: &[TimelineEvent],
        timestamp: f64,
        window_seconds: f64,
    ) -> Vec<TimelineEvent> {
        let start = timestamp - window_seconds;
        Self::filter_by_time_range(events, start, timestamp)
    }

    /// Get events within N seconds after a timestamp
    pub fn get_events_after(
        events: &[TimelineEvent],
        timestamp: f64,
        window_seconds: f64,
    ) -> Vec<TimelineEvent> {
        let end = timestamp + window_seconds;
        Self::filter_by_time_range(events, timestamp, end)
    }

    /// Get all unique event types in the event list
    pub fn get_unique_event_types(events: &[TimelineEvent]) -> Vec<String> {
        let mut types: Vec<String> = events.iter().map(|e| e.event_type.clone()).collect();
        types.sort();
        types.dedup();
        types
    }

    /// Get all unique node IDs in the event list
    pub fn get_unique_nodes(events: &[TimelineEvent]) -> Vec<String> {
        let mut nodes: Vec<String> = events.iter().filter_map(|e| e.node_id.clone()).collect();
        nodes.sort();
        nodes.dedup();
        nodes
    }

    /// Get event duration statistics
    pub fn get_time_range(events: &[TimelineEvent]) -> Option<(f64, f64)> {
        if events.is_empty() {
            return None;
        }

        let min = events
            .iter()
            .map(|e| e.timestamp)
            .fold(f64::INFINITY, f64::min);
        let max = events
            .iter()
            .map(|e| e.timestamp)
            .fold(f64::NEG_INFINITY, f64::max);

        Some((min, max))
    }

    /// Format timestamp as human-readable string
    pub fn format_timestamp(timestamp: f64) -> String {
        format!("{:.2}s", timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_event(id: &str, timestamp: f64, node_id: Option<&str>) -> TimelineEvent {
        TimelineEvent {
            id: id.to_string(),
            timestamp,
            event_type: "TestEvent".to_string(),
            description: "Test event".to_string(),
            node_id: node_id.map(|s| s.to_string()),
            metadata: None,
        }
    }

    #[test]
    fn test_convert_envelope() {
        let envelope = serde_json::json!({
            "timestamp": 1.5,
            "message_type": "KeyGen",
            "payload": {
                "description": "Key generation started",
                "node_id": "alice"
            }
        });

        let event = TimelineProcessor::convert_envelope_to_event(&envelope, 0).unwrap();
        assert_eq!(event.timestamp, 1.5);
        assert_eq!(event.event_type, "KeyGen");
        assert_eq!(event.node_id, Some("alice".to_string()));
    }

    #[test]
    fn test_filter_by_time_range() {
        let events = vec![
            create_test_event("1", 0.0, None),
            create_test_event("2", 1.0, None),
            create_test_event("3", 2.0, None),
            create_test_event("4", 3.0, None),
        ];

        let filtered = TimelineProcessor::filter_by_time_range(&events, 1.0, 2.5);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_node() {
        let events = vec![
            create_test_event("1", 0.0, Some("alice")),
            create_test_event("2", 1.0, Some("bob")),
            create_test_event("3", 2.0, Some("alice")),
        ];

        let filtered = TimelineProcessor::filter_by_node(&events, "alice");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_get_unique_nodes() {
        let events = vec![
            create_test_event("1", 0.0, Some("alice")),
            create_test_event("2", 1.0, Some("bob")),
            create_test_event("3", 2.0, Some("alice")),
            create_test_event("4", 3.0, Some("charlie")),
        ];

        let nodes = TimelineProcessor::get_unique_nodes(&events);
        assert_eq!(nodes.len(), 3);
        assert!(nodes.contains(&"alice".to_string()));
    }

    #[test]
    fn test_get_time_range() {
        let events = vec![
            create_test_event("1", 0.5, None),
            create_test_event("2", 2.5, None),
            create_test_event("3", 1.5, None),
        ];

        let (min, max) = TimelineProcessor::get_time_range(&events).unwrap();
        assert_eq!(min, 0.5);
        assert_eq!(max, 2.5);
    }

    #[test]
    fn test_get_time_range_empty() {
        let events: Vec<TimelineEvent> = vec![];
        assert!(TimelineProcessor::get_time_range(&events).is_none());
    }

    #[test]
    fn test_format_timestamp() {
        let formatted = TimelineProcessor::format_timestamp(1.23456);
        assert_eq!(formatted, "1.23s");
    }
}
