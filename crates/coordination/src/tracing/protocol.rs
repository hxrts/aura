//! Protocol-aware tracing for session types
//!
//! Provides specialized tracing for choreographic protocols with state
//! transition logging and Byzantine behavior detection.

use super::{AuraSpan, LogLevel, LogSink, LogValue, SpanBuilder, TracedOperation};
use aura_crypto::Effects;
use aura_journal::{AuraErrorKind, ByzantineEvidence, ByzantineSeverity, DeviceId, ProtocolType};
use aura_session_types::SessionState;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

/// Protocol tracer with session-type awareness
pub struct ProtocolTracer {
    log_sink: Arc<dyn LogSink>,
    device_id: DeviceId,
    effects: Effects,
    current_spans: BTreeMap<Uuid, AuraSpan>,
}

impl ProtocolTracer {
    /// Create a new protocol tracer
    pub fn new(log_sink: Arc<dyn LogSink>, device_id: DeviceId, effects: Effects) -> Self {
        Self {
            log_sink,
            device_id,
            effects,
            current_spans: BTreeMap::new(),
        }
    }
    
    /// Start tracing a protocol operation
    pub fn start_operation(&mut self, operation: &str, protocol: ProtocolType) -> TracedOperation {
        let span = SpanBuilder::new(operation, self.device_id)
            .with_protocol(protocol)
            .build(self.effects.now().unwrap_or(0));
        
        let span_id = span.span_id;
        self.current_spans.insert(span_id, span.clone());
        
        TracedOperation::new(span, self.log_sink.clone())
    }
    
    /// Start tracing a session-based operation
    pub fn start_session_operation(
        &mut self, 
        operation: &str, 
        protocol: ProtocolType, 
        session_id: Uuid
    ) -> TracedOperation {
        let span = SpanBuilder::new(operation, self.device_id)
            .with_protocol(protocol)
            .with_session(session_id)
            .build(self.effects.now().unwrap_or(0));
        
        let span_id = span.span_id;
        self.current_spans.insert(span_id, span.clone());
        
        TracedOperation::new(span, self.log_sink.clone())
    }
    
    /// Trace a state transition in session types
    pub fn trace_state_transition<S1, S2>(
        &self, 
        session_id: Uuid, 
        from: &S1, 
        to: &S2, 
        reason: &str
    ) where 
        S1: SessionState + SessionStateTracing,
        S2: SessionState + SessionStateTracing,
    {
        if let Some(span) = self.current_spans.values().find(|s| s.session_id == Some(session_id)) {
            let fields = crate::btreemap! {
                "from_state" => LogValue::String(from.state_name().to_string()),
                "to_state" => LogValue::String(to.state_name().to_string()),
                "reason" => LogValue::String(reason.to_string()),
                "session_id" => LogValue::SessionId(session_id),
            };
            
            self.log_sink.log_event(
                LogLevel::Info,
                span,
                format!("State transition: {} -> {}", from.state_name(), to.state_name()),
                fields,
            );
        }
    }
    
    /// Trace a message send operation
    pub fn trace_message_send(
        &self, 
        recipient: DeviceId, 
        message_type: &str, 
        size: usize,
        session_id: Option<Uuid>
    ) {
        let span = self.get_or_create_span(session_id, "message_send");
        
        let fields = crate::btreemap! {
            "recipient" => LogValue::DeviceId(recipient),
            "message_type" => LogValue::String(message_type.to_string()),
            "size_bytes" => LogValue::Number(size as i64),
        };
        
        self.log_sink.log_event(
            LogLevel::Debug,
            &span,
            format!("Sending {} to {}", message_type, recipient.0),
            fields,
        );
    }
    
    /// Trace a message receive operation
    pub fn trace_message_receive(
        &self, 
        sender: DeviceId, 
        message_type: &str, 
        size: usize,
        session_id: Option<Uuid>
    ) {
        let span = self.get_or_create_span(session_id, "message_receive");
        
        let fields = crate::btreemap! {
            "sender" => LogValue::DeviceId(sender),
            "message_type" => LogValue::String(message_type.to_string()),
            "size_bytes" => LogValue::Number(size as i64),
        };
        
        self.log_sink.log_event(
            LogLevel::Debug,
            &span,
            format!("Received {} from {}", message_type, sender.0),
            fields,
        );
    }
    
    /// Trace Byzantine behavior detection
    pub fn trace_byzantine_detection(
        &self, 
        accused: DeviceId, 
        evidence: ByzantineEvidence, 
        severity: ByzantineSeverity,
        session_id: Option<Uuid>
    ) {
        let span = self.get_or_create_span(session_id, "byzantine_detection");
        
        let fields = crate::btreemap! {
            "accused_device" => LogValue::DeviceId(accused),
            "evidence_type" => LogValue::String(format!("{:?}", evidence)),
            "severity" => LogValue::String(format!("{:?}", severity)),
        };
        
        self.log_sink.log_event(
            LogLevel::Error,
            &span,
            format!("Byzantine behavior detected: {} - {:?}", accused.0, evidence),
            fields,
        );
    }
    
    /// Trace threshold signature contribution
    pub fn trace_threshold_contribution(
        &self,
        participant_id: crate::ParticipantId,
        contribution_type: &str,
        session_id: Option<Uuid>
    ) {
        let span = self.get_or_create_span(session_id, "threshold_signature");
        
        let fields = crate::btreemap! {
            "participant_id" => LogValue::Number(participant_id.as_u16() as i64),
            "contribution_type" => LogValue::String(contribution_type.to_string()),
        };
        
        self.log_sink.log_event(
            LogLevel::Debug,
            &span,
            format!("Threshold contribution: {} from participant {}", contribution_type, participant_id.as_u16()),
            fields,
        );
    }
    
    /// Trace CRDT operation
    pub fn trace_crdt_operation(
        &self,
        operation: &str,
        object_id: &str,
        success: bool,
        session_id: Option<Uuid>
    ) {
        let span = self.get_or_create_span(session_id, "crdt_operation");
        
        let fields = crate::btreemap! {
            "operation" => LogValue::String(operation.to_string()),
            "object_id" => LogValue::String(object_id.to_string()),
            "success" => LogValue::Boolean(success),
        };
        
        let level = if success { LogLevel::Debug } else { LogLevel::Warn };
        let message = if success {
            format!("CRDT operation succeeded: {} on {}", operation, object_id)
        } else {
            format!("CRDT operation failed: {} on {}", operation, object_id)
        };
        
        self.log_sink.log_event(level, &span, message, fields);
    }
    
    /// Trace capability check
    pub fn trace_capability_check(
        &self,
        required_capability: &str,
        granted: bool,
        device_id: DeviceId,
        session_id: Option<Uuid>
    ) {
        let span = self.get_or_create_span(session_id, "capability_check");
        
        let fields = crate::btreemap! {
            "required_capability" => LogValue::String(required_capability.to_string()),
            "granted" => LogValue::Boolean(granted),
            "checked_device" => LogValue::DeviceId(device_id),
        };
        
        let level = if granted { LogLevel::Debug } else { LogLevel::Warn };
        let message = if granted {
            format!("Capability granted: {} for {}", required_capability, device_id.0)
        } else {
            format!("Capability denied: {} for {}", required_capability, device_id.0)
        };
        
        self.log_sink.log_event(level, &span, message, fields);
    }
    
    /// Trace error occurrence
    pub fn trace_error(&self, error: &AuraErrorKind, session_id: Option<Uuid>) {
        let span = self.get_or_create_span(session_id, "error");
        
        let fields = crate::btreemap! {
            "error_type" => LogValue::String(format!("{:?}", error)),
            "recoverable" => LogValue::Boolean(self.is_error_recoverable(error)),
        };
        
        self.log_sink.log_event(
            LogLevel::Error,
            &span,
            format!("Error occurred: {:?}", error),
            fields,
        );
    }
    
    /// Get or create a span for the given session
    pub fn get_or_create_span(&self, session_id: Option<Uuid>, operation: &str) -> AuraSpan {
        if let Some(sid) = session_id {
            if let Some(span) = self.current_spans.values().find(|s| s.session_id == Some(sid)) {
                return span.clone();
            }
        }
        
        // Create temporary span
        SpanBuilder::new(operation, self.device_id)
            .with_session(session_id.unwrap_or_else(Uuid::new_v4))
            .build(self.effects.now().unwrap_or(0))
    }
    
    /// Check if an error is recoverable
    fn is_error_recoverable(&self, error: &AuraErrorKind) -> bool {
        match error {
            AuraErrorKind::Network { .. } => true,
            AuraErrorKind::Resource { .. } => true,
            AuraErrorKind::Authorization { .. } => true,
            AuraErrorKind::Authentication { .. } => true,
            AuraErrorKind::Choreography { .. } => true,
            AuraErrorKind::Byzantine { .. } => false,
            AuraErrorKind::Corruption { .. } => false,
            AuraErrorKind::ProtocolViolation { .. } => false,
        }
    }
    
    /// Clean up completed spans
    pub fn cleanup_span(&mut self, span_id: Uuid) {
        self.current_spans.remove(&span_id);
    }
    
    /// Get access to the log sink for advanced logging
    pub fn log_sink(&self) -> &Arc<dyn LogSink> {
        &self.log_sink
    }
}

/// Session state trait extension for tracing
pub trait SessionStateTracing {
    /// Get the state name for tracing
    fn state_name(&self) -> &'static str;
}

// Implement for common session states
impl SessionStateTracing for () {
    fn state_name(&self) -> &'static str {
        "Unit"
    }
}

/// Macro for implementing SessionStateTracing for session types
#[macro_export]
macro_rules! impl_session_state_tracing {
    ($type:ty, $name:expr) => {
        impl SessionStateTracing for $type {
            fn state_name(&self) -> &'static str {
                $name
            }
        }
    };
}

/// Trace a session state transition using the macro
#[macro_export]
macro_rules! trace_transition {
    ($tracer:expr, $session_id:expr, $from:expr, $to:expr, $reason:expr) => {
        $tracer.trace_state_transition($session_id, &$from, &$to, $reason)
    };
}