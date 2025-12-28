//! Trace effect handler implementations.

use async_trait::async_trait;
use aura_core::effects::trace::{TraceEffects, TraceEvent, TraceSpanId};
use aura_core::hash::hash;
use std::collections::BTreeMap;

/// Stateless trace handler that logs structured events.
#[derive(Debug, Clone, Default)]
pub struct TraceHandler;

impl TraceHandler {
    /// Create a new trace handler.
    pub fn new() -> Self {
        Self
    }
}

fn span_id_from_event(event: &TraceEvent) -> TraceSpanId {
    let mut ordered: BTreeMap<&str, &str> = BTreeMap::new();
    for (key, value) in &event.fields {
        ordered.insert(key.as_str(), value.as_str());
    }

    let mut bytes = Vec::with_capacity(event.name.len() + ordered.len() * 8);
    bytes.extend_from_slice(event.name.as_bytes());
    for (key, value) in ordered {
        bytes.push(0u8);
        bytes.extend_from_slice(key.as_bytes());
        bytes.push(0u8);
        bytes.extend_from_slice(value.as_bytes());
    }

    let digest = hash(&bytes);
    let mut id_bytes = [0u8; 8];
    id_bytes.copy_from_slice(&digest[..8]);
    TraceSpanId(u64::from_le_bytes(id_bytes))
}

#[async_trait]
impl TraceEffects for TraceHandler {
    async fn trace_event(&self, event: TraceEvent) {
        tracing::debug!(trace_event = %event.name, fields = ?event.fields);
    }

    async fn trace_span(&self, event: TraceEvent) -> TraceSpanId {
        let span_id = span_id_from_event(&event);
        tracing::debug!(trace_span_start = %event.name, span_id = span_id.0, fields = ?event.fields);
        span_id
    }

    async fn trace_span_end(&self, span: TraceSpanId) {
        tracing::debug!(trace_span_end = span.0);
    }
}
