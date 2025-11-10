//! Console effects for logging and debugging

use aura_core::DeviceId;
use std::future::Future;

/// Log levels for console output
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    /// Trace level - detailed diagnostic information
    Trace,
    /// Debug level - debugging information
    Debug,
    /// Info level - informational messages
    Info,
    /// Warn level - warning messages
    Warn,
    /// Error level - error messages
    Error,
}

/// Console events for debugging and monitoring
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ConsoleEvent {
    /// Protocol has started execution
    ProtocolStarted {
        /// Unique identifier for this protocol instance
        protocol_id: String,
        /// Type of protocol being executed
        protocol_type: String,
        /// Device running the protocol
        device_id: DeviceId,
    },
    /// Protocol has completed execution
    ProtocolCompleted {
        /// Unique identifier for this protocol instance
        protocol_id: String,
        /// Whether the protocol succeeded
        success: bool,
        /// Device that ran the protocol
        device_id: DeviceId,
    },
    /// Message was sent between devices
    MessageSent {
        /// Source device
        from: DeviceId,
        /// Destination device
        to: DeviceId,
        /// Type of message being sent
        message_type: String,
    },
    /// Message was received by a device
    MessageReceived {
        /// Source device
        from: DeviceId,
        /// Destination device
        to: DeviceId,
        /// Type of message received
        message_type: String,
    },
    /// Component state has changed
    StateChanged {
        /// Component that changed state
        component: String,
        /// Previous state value
        old_state: String,
        /// New state value
        new_state: String,
    },
    /// Error occurred in a component
    Error {
        /// Component where error occurred
        component: String,
        /// Error description
        error: String,
    },
    /// Custom event with user-defined data
    Custom {
        /// Type of custom event
        event_type: String,
        /// JSON data for the event
        data: serde_json::Value,
    },
}

/// Console effects interface for logging and event emission
pub trait ConsoleEffects: Send + Sync {
    /// Log a trace message with structured fields
    ///
    /// # Arguments
    /// * `message` - The trace message to log
    /// * `fields` - Key-value pairs of structured data to include with the message
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]);

    /// Log a debug message with structured fields
    ///
    /// # Arguments
    /// * `message` - The debug message to log
    /// * `fields` - Key-value pairs of structured data to include with the message
    fn log_debug(&self, message: &str, fields: &[(&str, &str)]);

    /// Log an info message with structured fields
    ///
    /// # Arguments
    /// * `message` - The info message to log
    /// * `fields` - Key-value pairs of structured data to include with the message
    fn log_info(&self, message: &str, fields: &[(&str, &str)]);

    /// Log a warning message with structured fields
    ///
    /// # Arguments
    /// * `message` - The warning message to log
    /// * `fields` - Key-value pairs of structured data to include with the message
    fn log_warn(&self, message: &str, fields: &[(&str, &str)]);

    /// Log an error message with structured fields
    ///
    /// # Arguments
    /// * `message` - The error message to log
    /// * `fields` - Key-value pairs of structured data to include with the message
    fn log_error(&self, message: &str, fields: &[(&str, &str)]);

    /// Emit a structured event for debugging/monitoring
    ///
    /// # Arguments
    /// * `event` - The console event to emit
    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}
