//! Trace effects for structured runtime instrumentation.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Structured trace event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Event name or span name.
    pub name: String,
    /// Structured fields for the event/span.
    pub fields: HashMap<String, String>,
}

/// Opaque span identifier returned from `trace_span`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceSpanId(pub u64);

#[async_trait]
pub trait TraceEffects: Send + Sync {
    /// Emit a structured trace event.
    async fn trace_event(&self, event: TraceEvent);

    /// Begin a structured trace span and return its identifier.
    async fn trace_span(&self, event: TraceEvent) -> TraceSpanId;

    /// End a previously started span.
    async fn trace_span_end(&self, span: TraceSpanId);
}

#[async_trait]
impl<T: TraceEffects + ?Sized> TraceEffects for std::sync::Arc<T> {
    async fn trace_event(&self, event: TraceEvent) {
        (**self).trace_event(event).await;
    }

    async fn trace_span(&self, event: TraceEvent) -> TraceSpanId {
        (**self).trace_span(event).await
    }

    async fn trace_span_end(&self, span: TraceSpanId) {
        (**self).trace_span_end(span).await;
    }
}
