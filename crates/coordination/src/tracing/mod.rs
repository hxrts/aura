//! Structured logging framework for Aura
//!
//! Provides injectable logging that works seamlessly in both production
//! and simulation environments with rich context and protocol awareness.

pub mod production;
pub mod protocol;

use aura_journal::ProtocolType;
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

pub use production::*;
pub use protocol::*;

/// A structured logging span with rich context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraSpan {
    /// Unique span identifier
    pub span_id: Uuid,
    /// Parent span for hierarchical tracing
    pub parent_id: Option<Uuid>,
    /// Operation being performed
    pub operation: String,
    /// Protocol if applicable
    pub protocol: Option<ProtocolType>,
    /// Session ID if in a session
    pub session_id: Option<Uuid>,
    /// Device performing the operation
    pub device_id: DeviceId,
    /// When the span started
    pub started_at: u64,
    /// Structured fields
    pub fields: BTreeMap<String, LogValue>,
}

/// Structured values for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogValue {
    /// String value
    String(String),
    /// Numeric value
    Number(i64),
    /// Boolean value
    Boolean(bool),
    /// Device identifier
    DeviceId(DeviceId),
    /// Session identifier
    SessionId(Uuid),
    /// Hash value
    Hash([u8; 32]),
    /// Hexadecimal bytes
    Bytes(Vec<u8>),
}

/// Logging levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LogLevel {
    /// Trace level - very detailed
    Trace,
    /// Debug level - detailed
    Debug,
    /// Info level - general information
    Info,
    /// Warning level - potential issues
    Warn,
    /// Error level - errors
    Error,
}

/// Outcome when a span completes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanOutcome {
    /// Successful completion
    Success,
    /// Completed with error
    Error(aura_types::AuraError),
    /// Byzantine behavior detected
    Byzantine(DeviceId, aura_journal::ByzantineEvidence),
    /// Cancelled or interrupted
    Cancelled,
}

/// Injectable logging interface
pub trait LogSink: Send + Sync {
    /// Log a structured event
    fn log_event(
        &self,
        level: LogLevel,
        span: &AuraSpan,
        message: String,
        fields: BTreeMap<String, LogValue>,
    );

    /// Enter a new span
    fn enter_span(&self, span: AuraSpan);

    /// Exit a span with outcome
    fn exit_span(&self, span_id: Uuid, outcome: SpanOutcome);

    /// Check if a log level is enabled
    fn is_enabled(&self, level: LogLevel) -> bool;
}

/// Builder for creating spans
pub struct SpanBuilder {
    operation: String,
    device_id: DeviceId,
    protocol: Option<ProtocolType>,
    session_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    fields: BTreeMap<String, LogValue>,
}

impl SpanBuilder {
    /// Create a new span builder
    pub fn new(operation: &str, device_id: DeviceId) -> Self {
        Self {
            operation: operation.to_string(),
            device_id,
            protocol: None,
            session_id: None,
            parent_id: None,
            fields: BTreeMap::new(),
        }
    }

    /// Set the protocol for this span
    pub fn with_protocol(mut self, protocol: ProtocolType) -> Self {
        self.protocol = Some(protocol);
        self
    }

    /// Set the session ID for this span
    pub fn with_session(mut self, session_id: Uuid) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set the parent span
    pub fn with_parent(mut self, parent_id: Uuid) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    /// Add a field to the span
    pub fn with_field(mut self, key: &str, value: LogValue) -> Self {
        self.fields.insert(key.to_string(), value);
        self
    }

    /// Build the span
    pub fn build(self, started_at: u64) -> AuraSpan {
        AuraSpan {
            span_id: Uuid::new_v4(),
            parent_id: self.parent_id,
            operation: self.operation,
            protocol: self.protocol,
            session_id: self.session_id,
            device_id: self.device_id,
            started_at,
            fields: self.fields,
        }
    }
}

/// A traced operation that automatically logs entry and exit
pub struct TracedOperation {
    span: AuraSpan,
    log_sink: Arc<dyn LogSink>,
    completed: bool,
}

impl TracedOperation {
    /// Create a new traced operation
    pub fn new(span: AuraSpan, log_sink: Arc<dyn LogSink>) -> Self {
        log_sink.enter_span(span.clone());
        Self {
            span,
            log_sink,
            completed: false,
        }
    }

    /// Log an event within this operation
    pub fn log(&self, level: LogLevel, message: &str) {
        if self.log_sink.is_enabled(level) {
            self.log_sink
                .log_event(level, &self.span, message.to_string(), BTreeMap::new());
        }
    }

    /// Log an event with structured fields
    pub fn log_with_fields(
        &self,
        level: LogLevel,
        message: &str,
        fields: BTreeMap<String, LogValue>,
    ) {
        if self.log_sink.is_enabled(level) {
            self.log_sink
                .log_event(level, &self.span, message.to_string(), fields);
        }
    }

    /// Complete the operation successfully
    pub fn complete(mut self) {
        self.log_sink
            .exit_span(self.span.span_id, SpanOutcome::Success);
        self.completed = true;
    }

    /// Complete the operation with an error
    pub fn complete_with_error(mut self, error: aura_types::AuraError) {
        self.log_sink
            .exit_span(self.span.span_id, SpanOutcome::Error(error));
        self.completed = true;
    }

    /// Complete the operation with Byzantine evidence
    pub fn complete_with_byzantine(
        mut self,
        accused: DeviceId,
        evidence: aura_journal::ByzantineEvidence,
    ) {
        self.log_sink
            .exit_span(self.span.span_id, SpanOutcome::Byzantine(accused, evidence));
        self.completed = true;
    }

    /// Get the span ID
    pub fn span_id(&self) -> Uuid {
        self.span.span_id
    }

    /// Get a reference to the span
    pub fn span(&self) -> &AuraSpan {
        &self.span
    }
}

impl Drop for TracedOperation {
    fn drop(&mut self) {
        if !self.completed {
            self.log_sink
                .exit_span(self.span.span_id, SpanOutcome::Cancelled);
        }
    }
}

/// Convenience macros for logging
#[macro_export]
macro_rules! btreemap {
    () => { BTreeMap::new() };
    ($($key:expr => $value:expr),+ $(,)?) => {
        {
            let mut map = BTreeMap::new();
            $(map.insert($key.to_string(), $value);)+
            map
        }
    };
}

impl LogLevel {
    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

impl LogValue {
    /// Convert to display string
    pub fn to_display_string(&self) -> String {
        match self {
            LogValue::String(s) => s.clone(),
            LogValue::Number(n) => n.to_string(),
            LogValue::Boolean(b) => b.to_string(),
            LogValue::DeviceId(id) => format!("Device({})", id.0),
            LogValue::SessionId(id) => format!("Session({})", id),
            LogValue::Hash(h) => format!("Hash({})", hex::encode(&h[..8])),
            LogValue::Bytes(b) => format!("Bytes({})", hex::encode(&b[..8.min(b.len())])),
        }
    }
}

impl From<String> for LogValue {
    fn from(s: String) -> Self {
        LogValue::String(s)
    }
}

impl From<&str> for LogValue {
    fn from(s: &str) -> Self {
        LogValue::String(s.to_string())
    }
}

impl From<i64> for LogValue {
    fn from(n: i64) -> Self {
        LogValue::Number(n)
    }
}

impl From<u64> for LogValue {
    fn from(n: u64) -> Self {
        LogValue::Number(n as i64)
    }
}

impl From<bool> for LogValue {
    fn from(b: bool) -> Self {
        LogValue::Boolean(b)
    }
}

impl From<DeviceId> for LogValue {
    fn from(id: DeviceId) -> Self {
        LogValue::DeviceId(id)
    }
}

impl From<Uuid> for LogValue {
    fn from(id: Uuid) -> Self {
        LogValue::SessionId(id)
    }
}

impl From<[u8; 32]> for LogValue {
    fn from(hash: [u8; 32]) -> Self {
        LogValue::Hash(hash)
    }
}

impl From<Vec<u8>> for LogValue {
    fn from(bytes: Vec<u8>) -> Self {
        LogValue::Bytes(bytes)
    }
}
