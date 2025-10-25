//! Production logging implementation
//!
//! Provides a LogSink implementation suitable for production deployment
//! with structured logging, metrics collection, and error reporting.

use super::{AuraSpan, LogLevel, LogSink, LogValue, SpanOutcome};
use aura_journal::DeviceId;
use std::collections::BTreeMap;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

/// Production log sink that integrates with standard logging infrastructure
pub struct ProductionLogSink {
    /// Device ID for this sink
    _device_id: DeviceId,
    /// Minimum log level to emit
    min_level: LogLevel,
    /// Whether to include structured fields in logs
    include_fields: bool,
}

impl ProductionLogSink {
    /// Create a new production log sink
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            _device_id: device_id,
            min_level: LogLevel::Info,
            include_fields: true,
        }
    }
    
    /// Set the minimum log level
    pub fn with_min_level(mut self, level: LogLevel) -> Self {
        self.min_level = level;
        self
    }
    
    /// Set whether to include structured fields
    pub fn with_fields(mut self, include_fields: bool) -> Self {
        self.include_fields = include_fields;
        self
    }
    
    /// Format fields for logging
    fn format_fields(&self, fields: &BTreeMap<String, LogValue>) -> String {
        if !self.include_fields || fields.is_empty() {
            return String::new();
        }
        
        let field_strings: Vec<String> = fields
            .iter()
            .map(|(k, v)| format!("{}={}", k, v.to_display_string()))
            .collect();
        
        format!(" [{}]", field_strings.join(", "))
    }
    
    /// Create a tracing span for the AuraSpan
    fn create_tracing_span(&self, span: &AuraSpan) -> tracing::Span {
        
        if let Some(protocol) = &span.protocol {
            tracing::info_span!(
                "aura_operation",
                operation = %span.operation,
                device_id = %span.device_id.0,
                protocol = %protocol,
                span_id = %span.span_id,
                session_id = ?span.session_id
            )
        } else {
            tracing::info_span!(
                "aura_operation",
                operation = %span.operation,
                device_id = %span.device_id.0,
                span_id = %span.span_id,
                session_id = ?span.session_id
            )
        }
    }
}

impl LogSink for ProductionLogSink {
    fn log_event(
        &self,
        level: LogLevel,
        span: &AuraSpan,
        message: String,
        fields: BTreeMap<String, LogValue>,
    ) {
        if !self.is_enabled(level) {
            return;
        }
        
        let formatted_fields = self.format_fields(&fields);
        let full_message = format!(
            "[{}::{}] {}{}",
            span.device_id.0, span.operation, message, formatted_fields
        );
        
        // Create a tracing span for context
        let tracing_span = self.create_tracing_span(span);
        let _enter = tracing_span.enter();
        
        // Log at appropriate level
        match level {
            LogLevel::Trace => trace!(message = %full_message, span_id = %span.span_id),
            LogLevel::Debug => debug!(message = %full_message, span_id = %span.span_id),
            LogLevel::Info => info!(message = %full_message, span_id = %span.span_id),
            LogLevel::Warn => warn!(message = %full_message, span_id = %span.span_id),
            LogLevel::Error => error!(message = %full_message, span_id = %span.span_id),
        }
        
        // Special handling for Byzantine behavior
        if message.contains("Byzantine") {
            error!(
                byzantine_behavior = true,
                device_id = %span.device_id.0,
                message = %message
            );
        }
    }
    
    fn enter_span(&self, span: AuraSpan) {
        if self.is_enabled(LogLevel::Debug) {
            let tracing_span = self.create_tracing_span(&span);
            let _enter = tracing_span.enter();
            
            debug!(
                "Entering span: {} [{}]",
                span.operation,
                span.span_id
            );
        }
    }
    
    fn exit_span(&self, span_id: Uuid, outcome: SpanOutcome) {
        if self.is_enabled(LogLevel::Debug) {
            match outcome {
                SpanOutcome::Success => {
                    debug!("Span completed successfully [{}]", span_id);
                }
                SpanOutcome::Error(ref error) => {
                    error!(
                        "Span completed with error [{}]: {:?}",
                        span_id, error
                    );
                }
                SpanOutcome::Byzantine(device_id, ref evidence) => {
                    error!(
                        byzantine_behavior = true,
                        accused_device = %device_id.0,
                        "Span detected Byzantine behavior [{}]: {:?}",
                        span_id, evidence
                    );
                }
                SpanOutcome::Cancelled => {
                    warn!("Span was cancelled [{}]", span_id);
                }
            }
        }
    }
    
    fn is_enabled(&self, level: LogLevel) -> bool {
        level >= self.min_level
    }
}

/// Null log sink that discards all events (for testing)
pub struct NullLogSink;

impl LogSink for NullLogSink {
    fn log_event(&self, _level: LogLevel, _span: &AuraSpan, _message: String, _fields: BTreeMap<String, LogValue>) {}
    fn enter_span(&self, _span: AuraSpan) {}
    fn exit_span(&self, _span_id: Uuid, _outcome: SpanOutcome) {}
    fn is_enabled(&self, _level: LogLevel) -> bool { false }
}

/// Console log sink for development (prints to stdout/stderr)
pub struct ConsoleLogSink {
    min_level: LogLevel,
    colored: bool,
}

impl ConsoleLogSink {
    /// Create a new console log sink
    pub fn new() -> Self {
        Self {
            min_level: LogLevel::Debug,
            colored: true,
        }
    }
    
    /// Set minimum log level
    pub fn with_min_level(mut self, level: LogLevel) -> Self {
        self.min_level = level;
        self
    }
    
    /// Set whether to use colored output
    pub fn with_colors(mut self, colored: bool) -> Self {
        self.colored = colored;
        self
    }
    
    /// Get color code for log level
    fn level_color(&self, level: LogLevel) -> &'static str {
        if !self.colored {
            return "";
        }
        
        match level {
            LogLevel::Trace => "\x1b[90m", // Dark gray
            LogLevel::Debug => "\x1b[34m", // Blue
            LogLevel::Info => "\x1b[32m",  // Green
            LogLevel::Warn => "\x1b[33m",  // Yellow
            LogLevel::Error => "\x1b[31m", // Red
        }
    }
    
    /// Get reset color code
    fn reset_color(&self) -> &'static str {
        if self.colored { "\x1b[0m" } else { "" }
    }
}

impl LogSink for ConsoleLogSink {
    fn log_event(
        &self,
        level: LogLevel,
        span: &AuraSpan,
        message: String,
        fields: BTreeMap<String, LogValue>,
    ) {
        if !self.is_enabled(level) {
            return;
        }
        
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let level_str = level.as_str().to_uppercase();
        let color = self.level_color(level);
        let reset = self.reset_color();
        
        let mut output = format!(
            "{}{} [{}] [{}::{}] {}{}",
            color, timestamp, level_str, span.device_id.0, span.operation, message, reset
        );
        
        // Add fields if present
        if !fields.is_empty() {
            output.push_str(" [");
            for (i, (key, value)) in fields.iter().enumerate() {
                if i > 0 {
                    output.push_str(", ");
                }
                output.push_str(&format!("{}={}", key, value.to_display_string()));
            }
            output.push(']');
        }
        
        // Add session/protocol context
        if let Some(session_id) = span.session_id {
            output.push_str(&format!(" session={}", session_id));
        }
        if let Some(protocol) = &span.protocol {
            output.push_str(&format!(" protocol={}", protocol));
        }
        
        println!("{}", output);
    }
    
    fn enter_span(&self, span: AuraSpan) {
        if self.is_enabled(LogLevel::Debug) {
            let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let color = self.level_color(LogLevel::Debug);
            let reset = self.reset_color();
            
            println!(
                "{}{}  --> [{}::{}] span_id={}{}",
                color, timestamp, span.device_id.0, span.operation, span.span_id, reset
            );
        }
    }
    
    fn exit_span(&self, span_id: Uuid, outcome: SpanOutcome) {
        if self.is_enabled(LogLevel::Debug) {
            let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let (color, outcome_str) = match outcome {
                SpanOutcome::Success => (self.level_color(LogLevel::Debug), "SUCCESS"),
                SpanOutcome::Error(_) => (self.level_color(LogLevel::Error), "ERROR"),
                SpanOutcome::Byzantine(_, _) => (self.level_color(LogLevel::Error), "BYZANTINE"),
                SpanOutcome::Cancelled => (self.level_color(LogLevel::Warn), "CANCELLED"),
            };
            let reset = self.reset_color();
            
            println!(
                "{}{}  <-- {} span_id={}{}",
                color, timestamp, outcome_str, span_id, reset
            );
        }
    }
    
    fn is_enabled(&self, level: LogLevel) -> bool {
        level >= self.min_level
    }
}

impl Default for ConsoleLogSink {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use aura_journal::ProtocolType;
    use aura_crypto::Effects;
    
    #[test]
    fn test_console_log_sink() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let sink = ConsoleLogSink::new().with_min_level(LogLevel::Info);
        
        let span = crate::tracing::SpanBuilder::new("test_operation", device_id)
            .with_protocol(ProtocolType::Dkd)
            .build(0);
        
        sink.enter_span(span.clone());
        
        sink.log_event(
            LogLevel::Info,
            &span,
            "Test message".to_string(),
            crate::btreemap! {
                "key1" => LogValue::String("value1".to_string()),
                "key2" => LogValue::Number(42),
            }
        );
        
        sink.exit_span(span.span_id, SpanOutcome::Success);
    }
    
    #[test]
    fn test_null_log_sink() {
        let sink = NullLogSink;
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        
        let span = crate::tracing::SpanBuilder::new("test", device_id).build(0);
        
        // Should not panic
        sink.log_event(LogLevel::Error, &span, "test".to_string(), BTreeMap::new());
        sink.enter_span(span.clone());
        sink.exit_span(span.span_id, SpanOutcome::Success);
    }
}