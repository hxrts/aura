//! Console effects for logging and debugging

use crate::DeviceId;
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
#[derive(Debug, Clone)]
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
pub trait ConsoleEffects {
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

/// Production console effects using real logging
///
/// Outputs all logs and events to stdout with timestamps and severity levels.
pub struct ProductionConsoleEffects;

impl ProductionConsoleEffects {
    /// Create a new production console effects instance
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProductionConsoleEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleEffects for ProductionConsoleEffects {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        println!("[TRACE] {}: {:?}", message, fields);
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        println!("[DEBUG] {}: {:?}", message, fields);
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        println!("[INFO] {}: {:?}", message, fields);
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        println!("[WARN] {}: {:?}", message, fields);
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        println!("[ERROR] {}: {:?}", message, fields);
    }

    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            println!("[EVENT] {:?}", event);
        })
    }
}

/// Test console effects that capture output for verification
///
/// Captures all logs and events in memory for use in tests. All output is
/// stored and can be retrieved for assertion and verification.
pub struct TestConsoleEffects {
    /// Captured logs with their severity level
    logs: std::sync::Arc<std::sync::Mutex<Vec<(LogLevel, String)>>>,
    /// Captured console events
    events: std::sync::Arc<std::sync::Mutex<Vec<ConsoleEvent>>>,
}

impl TestConsoleEffects {
    /// Create a new test console effects instance with empty logs and events
    pub fn new() -> Self {
        Self {
            logs: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            events: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Get all captured logs as a cloned vector
    pub fn get_logs(&self) -> Vec<(LogLevel, String)> {
        self.logs.lock().unwrap().clone()
    }

    /// Get all captured events as a cloned vector
    pub fn get_events(&self) -> Vec<ConsoleEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Clear all captured logs and events
    pub fn clear(&self) {
        self.logs.lock().unwrap().clear();
        self.events.lock().unwrap().clear();
    }
}

impl ConsoleEffects for TestConsoleEffects {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        self.logs
            .lock()
            .unwrap()
            .push((LogLevel::Trace, format!("{}: {:?}", message, fields)));
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        self.logs
            .lock()
            .unwrap()
            .push((LogLevel::Debug, format!("{}: {:?}", message, fields)));
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        self.logs
            .lock()
            .unwrap()
            .push((LogLevel::Info, format!("{}: {:?}", message, fields)));
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        self.logs
            .lock()
            .unwrap()
            .push((LogLevel::Warn, format!("{}: {:?}", message, fields)));
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        self.logs
            .lock()
            .unwrap()
            .push((LogLevel::Error, format!("{}: {:?}", message, fields)));
    }

    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.events.lock().unwrap().push(event);
        Box::pin(async move {})
    }
}
