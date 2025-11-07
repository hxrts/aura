//! Operation Tracing Middleware
//!
//! Provides distributed tracing and logging capabilities for agent operations,
//! enabling debugging, monitoring, and audit trails.

use aura_types::{
    identifiers::{DeviceId, SessionId},
    AuraError, AuraResult as Result,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Unique identifier for a trace span
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TraceId(pub Uuid);

impl TraceId {
    /// Create a new trace ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a span within a trace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpanId(pub Uuid);

impl SpanId {
    /// Create a new span ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Log level for trace events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Trace => write!(f, "TRACE"),
        }
    }
}

/// A single trace event
#[derive(Debug, Clone)]
pub struct TraceEvent {
    /// Unique event identifier
    pub event_id: Uuid,
    /// Trace this event belongs to
    pub trace_id: TraceId,
    /// Span this event belongs to
    pub span_id: SpanId,
    /// Parent span if any
    pub parent_span_id: Option<SpanId>,
    /// Timestamp when event occurred
    pub timestamp: SystemTime,
    /// Log level
    pub level: LogLevel,
    /// Event message
    pub message: String,
    /// Operation name
    pub operation: String,
    /// Device that generated this event
    pub device_id: DeviceId,
    /// Session context if available
    pub session_id: Option<SessionId>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
    /// Duration for span events
    pub duration: Option<Duration>,
    /// Success/failure status for operation events
    pub success: Option<bool>,
}

impl TraceEvent {
    /// Create a new trace event
    pub fn new(
        trace_id: TraceId,
        span_id: SpanId,
        level: LogLevel,
        message: String,
        operation: String,
        device_id: DeviceId,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            trace_id,
            span_id,
            parent_span_id: None,
            timestamp: SystemTime::now(),
            level,
            message,
            operation,
            device_id,
            session_id: None,
            metadata: HashMap::new(),
            duration: None,
            success: None,
        }
    }

    /// Add metadata to the event
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set session context
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set parent span
    pub fn with_parent(mut self, parent_span_id: SpanId) -> Self {
        self.parent_span_id = Some(parent_span_id);
        self
    }

    /// Set duration for span completion events
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set success status
    pub fn with_success(mut self, success: bool) -> Self {
        self.success = Some(success);
        self
    }

    /// Format as structured log line
    pub fn format(&self) -> String {
        let timestamp_str = self
            .timestamp
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        let duration_str = self
            .duration
            .map(|d| format!(" duration={}ms", d.as_millis()))
            .unwrap_or_default();

        let success_str = self
            .success
            .map(|s| format!(" success={}", s))
            .unwrap_or_default();

        let session_str = self
            .session_id
            .as_ref()
            .map(|s| format!(" session_id={}", s))
            .unwrap_or_default();

        let parent_str = self
            .parent_span_id
            .map(|p| format!(" parent_span={}", p))
            .unwrap_or_default();

        let metadata_str = if !self.metadata.is_empty() {
            let pairs: Vec<String> = self
                .metadata
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            format!(" {}", pairs.join(" "))
        } else {
            String::new()
        };

        format!(
            "{} [{}] trace_id={} span_id={}{} device_id={} operation={} {}{}{}{}{}",
            timestamp_str,
            self.level,
            self.trace_id,
            self.span_id,
            parent_str,
            self.device_id,
            self.operation,
            self.message,
            session_str,
            duration_str,
            success_str,
            metadata_str
        )
    }
}

/// Active span tracking
#[derive(Debug, Clone)]
pub struct ActiveSpan {
    pub span_id: SpanId,
    pub trace_id: TraceId,
    pub operation: String,
    pub start_time: SystemTime,
    pub parent_span_id: Option<SpanId>,
    pub device_id: DeviceId,
}

impl ActiveSpan {
    /// Create a new active span
    pub fn new(
        span_id: SpanId,
        trace_id: TraceId,
        operation: String,
        device_id: DeviceId,
        parent_span_id: Option<SpanId>,
    ) -> Self {
        Self {
            span_id,
            trace_id,
            operation,
            start_time: SystemTime::now(),
            parent_span_id,
            device_id,
        }
    }

    /// Get the elapsed time since span started
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed().unwrap_or_default()
    }
}

/// Trace storage and management
#[derive(Debug)]
pub struct TraceStorage {
    /// All trace events
    events: Vec<TraceEvent>,
    /// Currently active spans
    active_spans: HashMap<SpanId, ActiveSpan>,
    /// Maximum number of events to store
    max_events: usize,
    /// Device ID for this storage
    device_id: DeviceId,
}

impl TraceStorage {
    /// Create new trace storage
    pub fn new(device_id: DeviceId, max_events: usize) -> Self {
        Self {
            events: Vec::new(),
            active_spans: HashMap::new(),
            max_events,
            device_id,
        }
    }

    /// Add an event to storage
    pub fn add_event(&mut self, event: TraceEvent) {
        self.events.push(event);

        // Trim events if we exceed the maximum
        if self.events.len() > self.max_events {
            let excess = self.events.len() - self.max_events;
            self.events.drain(0..excess);
        }
    }

    /// Start a new span
    pub fn start_span(
        &mut self,
        span_id: SpanId,
        trace_id: TraceId,
        operation: String,
        parent_span_id: Option<SpanId>,
    ) {
        let span = ActiveSpan::new(
            span_id,
            trace_id,
            operation.clone(),
            self.device_id,
            parent_span_id,
        );
        self.active_spans.insert(span_id, span);

        // Create span start event
        let event = TraceEvent::new(
            trace_id,
            span_id,
            LogLevel::Debug,
            "span_start".to_string(),
            operation,
            self.device_id,
        )
        .with_parent(parent_span_id.unwrap_or(span_id));

        self.add_event(event);
    }

    /// End a span
    pub fn end_span(&mut self, span_id: SpanId, success: bool) -> Result<()> {
        let span = self
            .active_spans
            .remove(&span_id)
            .ok_or_else(|| AuraError::not_found("Span not found"))?;

        let duration = span.elapsed();

        // Create span end event
        let event = TraceEvent::new(
            span.trace_id,
            span_id,
            LogLevel::Debug,
            "span_end".to_string(),
            span.operation,
            self.device_id,
        )
        .with_duration(duration)
        .with_success(success)
        .with_parent(span.parent_span_id.unwrap_or(span_id));

        self.add_event(event);

        Ok(())
    }

    /// Get events for a specific trace
    pub fn get_trace_events(&self, trace_id: TraceId) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| e.trace_id == trace_id)
            .collect()
    }

    /// Get events for a specific operation
    pub fn get_operation_events(&self, operation: &str) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| e.operation == operation)
            .collect()
    }

    /// Get recent events
    pub fn get_recent_events(&self, limit: usize) -> Vec<&TraceEvent> {
        let start = self.events.len().saturating_sub(limit);
        self.events[start..].iter().collect()
    }

    /// Get all active spans
    pub fn get_active_spans(&self) -> Vec<&ActiveSpan> {
        self.active_spans.values().collect()
    }

    /// Clear all stored events
    pub fn clear(&mut self) {
        self.events.clear();
        self.active_spans.clear();
    }

    /// Get storage statistics
    pub fn stats(&self) -> TraceStorageStats {
        TraceStorageStats {
            total_events: self.events.len(),
            active_spans: self.active_spans.len(),
            max_events: self.max_events,
            device_id: self.device_id,
        }
    }
}

/// Statistics about trace storage
#[derive(Debug, Clone)]
pub struct TraceStorageStats {
    pub total_events: usize,
    pub active_spans: usize,
    pub max_events: usize,
    pub device_id: DeviceId,
}

/// Operation tracer for high-level tracing operations
pub struct OperationTracer {
    /// Current trace context
    current_trace: Option<TraceId>,
    /// Current span context
    current_span: Option<SpanId>,
    /// Device ID
    device_id: DeviceId,
}

impl OperationTracer {
    /// Create new operation tracer
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            current_trace: None,
            current_span: None,
            device_id,
        }
    }

    /// Start a new trace
    pub fn start_trace(&mut self) -> TraceId {
        let trace_id = TraceId::new();
        self.current_trace = Some(trace_id);
        trace_id
    }

    /// Start a new span in the current trace
    pub fn start_span(&mut self, operation: String) -> SpanId {
        let trace_id = self.current_trace.unwrap_or_else(|| self.start_trace());
        let span_id = SpanId::new();
        let parent_span_id = self.current_span;

        self.current_span = Some(span_id);

        span_id
    }

    /// End the current span
    pub fn end_span(&mut self) {
        // In a full implementation, this would interact with trace storage
        self.current_span = None;
    }

    /// Get current trace context
    pub fn current_trace(&self) -> Option<TraceId> {
        self.current_trace
    }

    /// Get current span context
    pub fn current_span(&self) -> Option<SpanId> {
        self.current_span
    }
}

/// Tracing middleware for agent operations
pub struct TracingMiddleware {
    /// Trace storage
    storage: Arc<RwLock<TraceStorage>>,
    /// Device ID
    device_id: DeviceId,
    /// Tracer instance
    tracer: Arc<RwLock<OperationTracer>>,
}

impl TracingMiddleware {
    /// Create new tracing middleware
    pub async fn new(device_id: DeviceId) -> Result<Self> {
        let storage = TraceStorage::new(device_id, 10000); // Store up to 10k events
        let tracer = OperationTracer::new(device_id);

        Ok(Self {
            storage: Arc::new(RwLock::new(storage)),
            device_id,
            tracer: Arc::new(RwLock::new(tracer)),
        })
    }

    /// Start tracing an operation
    pub async fn start_operation(&self, operation_name: &str) -> Result<TraceId> {
        let mut tracer = self.tracer.write().await;
        let trace_id = tracer.start_trace();
        let span_id = tracer.start_span(operation_name.to_string());

        let mut storage = self.storage.write().await;
        storage.start_span(span_id, trace_id, operation_name.to_string(), None);

        Ok(trace_id)
    }

    /// End tracing an operation
    pub async fn end_operation(&self, trace_id: TraceId, success: bool) -> Result<()> {
        let tracer = self.tracer.read().await;
        if let Some(span_id) = tracer.current_span() {
            let mut storage = self.storage.write().await;
            storage.end_span(span_id, success)?;
        }

        Ok(())
    }

    /// Log an event in the current trace context
    pub async fn log_event(
        &self,
        level: LogLevel,
        message: String,
        operation: String,
    ) -> Result<()> {
        let tracer = self.tracer.read().await;

        let trace_id = tracer.current_trace().unwrap_or_else(TraceId::new);
        let span_id = tracer.current_span().unwrap_or_else(SpanId::new);

        let event = TraceEvent::new(trace_id, span_id, level, message, operation, self.device_id);

        let mut storage = self.storage.write().await;
        storage.add_event(event);

        Ok(())
    }

    /// Get recent trace events
    pub async fn get_recent_events(&self, limit: usize) -> Vec<TraceEvent> {
        let storage = self.storage.read().await;
        storage
            .get_recent_events(limit)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get events for a specific operation
    pub async fn get_operation_events(&self, operation: &str) -> Vec<TraceEvent> {
        let storage = self.storage.read().await;
        storage
            .get_operation_events(operation)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get storage statistics
    pub async fn get_stats(&self) -> TraceStorageStats {
        let storage = self.storage.read().await;
        storage.stats()
    }

    /// Clear all trace data
    pub async fn clear(&self) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.clear();
        Ok(())
    }

    /// Export trace data as JSON
    pub async fn export_traces(&self) -> Result<String> {
        let storage = self.storage.read().await;
        let recent_events = storage.get_recent_events(1000);

        // Simple JSON export - could use serde for more sophisticated serialization
        let mut export = String::from("[\n");

        for (i, event) in recent_events.iter().enumerate() {
            if i > 0 {
                export.push_str(",\n");
            }

            export.push_str(&format!(
                "  {{\"event_id\":\"{}\",\"trace_id\":\"{}\",\"span_id\":\"{}\",\"timestamp\":{},\"level\":\"{}\",\"message\":\"{}\",\"operation\":\"{}\"}}",
                event.event_id,
                event.trace_id,
                event.span_id,
                event.timestamp.duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0),
                event.level,
                event.message.replace('"', "\\\""),
                event.operation
            ));
        }

        export.push_str("\n]");
        Ok(export)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_event_creation() {
        let device_id = DeviceId::new();
        let trace_id = TraceId::new();
        let span_id = SpanId::new();

        let event = TraceEvent::new(
            trace_id,
            span_id,
            LogLevel::Info,
            "Test message".to_string(),
            "test_operation".to_string(),
            device_id,
        );

        assert_eq!(event.trace_id, trace_id);
        assert_eq!(event.span_id, span_id);
        assert_eq!(event.level, LogLevel::Info);
        assert_eq!(event.message, "Test message");
        assert_eq!(event.operation, "test_operation");
    }

    #[test]
    fn test_trace_event_formatting() {
        let device_id = DeviceId::new();
        let trace_id = TraceId::new();
        let span_id = SpanId::new();

        let event = TraceEvent::new(
            trace_id,
            span_id,
            LogLevel::Error,
            "Error occurred".to_string(),
            "error_op".to_string(),
            device_id,
        )
        .with_success(false)
        .with_duration(Duration::from_millis(100));

        let formatted = event.format();
        assert!(formatted.contains("ERROR"));
        assert!(formatted.contains("Error occurred"));
        assert!(formatted.contains("duration=100ms"));
        assert!(formatted.contains("success=false"));
    }

    #[tokio::test]
    async fn test_trace_storage() {
        let device_id = DeviceId::new();
        let mut storage = TraceStorage::new(device_id, 100);

        let trace_id = TraceId::new();
        let span_id = SpanId::new();

        // Start a span
        storage.start_span(span_id, trace_id, "test_op".to_string(), None);
        assert_eq!(storage.active_spans.len(), 1);

        // End the span
        storage.end_span(span_id, true).unwrap();
        assert_eq!(storage.active_spans.len(), 0);

        // Should have start and end events
        let trace_events = storage.get_trace_events(trace_id);
        assert_eq!(trace_events.len(), 2);
    }

    #[tokio::test]
    async fn test_tracing_middleware() {
        let device_id = DeviceId::new();
        let middleware = TracingMiddleware::new(device_id).await.unwrap();

        // Start an operation
        let trace_id = middleware.start_operation("test_operation").await.unwrap();

        // Log an event
        middleware
            .log_event(
                LogLevel::Info,
                "Operation started".to_string(),
                "test_operation".to_string(),
            )
            .await
            .unwrap();

        // End the operation
        middleware.end_operation(trace_id, true).await.unwrap();

        // Check that events were recorded
        let recent_events = middleware.get_recent_events(10).await;
        assert!(!recent_events.is_empty());

        let stats = middleware.get_stats().await;
        assert!(stats.total_events > 0);
    }
}
