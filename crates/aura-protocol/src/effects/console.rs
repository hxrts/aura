//! Console visualization effects
//!
//! This module treats console visualization as an algebraic effect,
//! allowing protocols to emit visualization events without directly
//! coupling to the console implementation.

// Remove circular import - ConsoleEffect is defined below
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Console visualization effects interface
///
/// This trait defines the effect operations for emitting visualization
/// events to the console. Implementations can be swapped out for testing,
/// production use, or different visualization backends.
#[async_trait]
pub trait ConsoleEffects: Send + Sync {
    /// Emit a choreography event for visualization
    async fn emit_choreo_event(&self, event: ConsoleEffect);

    /// Mark a protocol as started
    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str);

    /// Mark a protocol as completed
    async fn protocol_completed(&self, protocol_id: Uuid, duration_ms: u64);

    /// Mark a protocol as failed
    async fn protocol_failed(&self, protocol_id: Uuid, error: &str);

    /// Log an info message
    async fn log_info(&self, message: &str);

    /// Log a warning message
    async fn log_warning(&self, message: &str);

    /// Log an error message
    async fn log_error(&self, message: &str);

    /// Flush any buffered events
    async fn flush(&self);
}

/// No-op implementation for when console is not available
#[derive(Debug, Clone, Default)]
pub struct NoOpConsoleEffects;

#[async_trait]
impl ConsoleEffects for NoOpConsoleEffects {
    async fn emit_choreo_event(&self, _event: ConsoleEffect) {}
    async fn protocol_started(&self, _protocol_id: Uuid, _protocol_type: &str) {}
    async fn protocol_completed(&self, _protocol_id: Uuid, _duration_ms: u64) {}
    async fn protocol_failed(&self, _protocol_id: Uuid, _error: &str) {}
    async fn log_info(&self, _message: &str) {}
    async fn log_warning(&self, _message: &str) {}
    async fn log_error(&self, _message: &str) {}
    async fn flush(&self) {}
}

/// Channel-based console effects for async event streaming
pub struct ChannelConsoleEffects {
    /// Channel sender for console events
    sender: mpsc::UnboundedSender<ConsoleEffect>,
}

/// Console effect event types
#[derive(Debug, Clone)]
pub enum ConsoleEffect {
    /// Choreography event with arbitrary JSON data
    ChoreoEvent(serde_json::Value),
    /// Protocol started event
    ProtocolStarted {
        /// Unique identifier for the protocol instance
        protocol_id: Uuid,
        /// Type of protocol being executed
        protocol_type: String,
    },
    /// Protocol completed successfully
    ProtocolCompleted {
        /// Unique identifier for the protocol instance
        protocol_id: Uuid,
        /// Total execution duration in milliseconds
        duration_ms: u64,
    },
    /// Protocol failed
    ProtocolFailed {
        /// Unique identifier for the protocol instance
        protocol_id: Uuid,
        /// Error message describing the failure
        error: String,
    },
    /// Informational log message
    LogInfo {
        /// The log message
        message: String,
    },
    /// Warning log message
    LogWarning {
        /// The warning message
        message: String,
    },
    /// Error log message
    LogError {
        /// The error message
        message: String,
    },
    /// Custom marker event
    Marker {
        /// Type of marker
        marker_type: String,
        /// Arbitrary JSON data for the marker
        data: serde_json::Value,
    },
    /// Flush buffered events
    Flush,
}

impl ChannelConsoleEffects {
    /// Create a new channel-based console effects provider
    pub fn new() -> (Self, mpsc::UnboundedReceiver<ConsoleEffect>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, receiver)
    }
}

#[async_trait]
impl ConsoleEffects for ChannelConsoleEffects {
    async fn emit_choreo_event(&self, event: ConsoleEffect) {
        let _ = self.sender.send(event);
    }

    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str) {
        let _ = self.sender.send(ConsoleEffect::ProtocolStarted {
            protocol_id,
            protocol_type: protocol_type.to_string(),
        });
    }

    async fn protocol_completed(&self, protocol_id: Uuid, duration_ms: u64) {
        let _ = self.sender.send(ConsoleEffect::ProtocolCompleted {
            protocol_id,
            duration_ms,
        });
    }

    async fn protocol_failed(&self, protocol_id: Uuid, error: &str) {
        let _ = self.sender.send(ConsoleEffect::ProtocolFailed {
            protocol_id,
            error: error.to_string(),
        });
    }

    async fn log_info(&self, message: &str) {
        let _ = self.sender.send(ConsoleEffect::LogInfo {
            message: message.to_string(),
        });
    }

    async fn log_warning(&self, message: &str) {
        let _ = self.sender.send(ConsoleEffect::LogWarning {
            message: message.to_string(),
        });
    }

    async fn log_error(&self, message: &str) {
        let _ = self.sender.send(ConsoleEffect::LogError {
            message: message.to_string(),
        });
    }

    async fn flush(&self) {
        let _ = self.sender.send(ConsoleEffect::Flush);
    }
}

/// Recording console effects for testing and replay
pub struct RecordingConsoleEffects {
    /// Recorded events
    events: Arc<tokio::sync::Mutex<Vec<ConsoleEffect>>>,
}

impl Default for RecordingConsoleEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingConsoleEffects {
    /// Create a new recording console effects provider
    pub fn new() -> Self {
        Self {
            events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    /// Get all recorded events
    pub async fn get_events(&self) -> Vec<ConsoleEffect> {
        self.events.lock().await.clone()
    }

    /// Clear all recorded events
    pub async fn clear(&self) {
        self.events.lock().await.clear()
    }
}

#[async_trait]
impl ConsoleEffects for RecordingConsoleEffects {
    async fn emit_choreo_event(&self, event: ConsoleEffect) {
        self.events.lock().await.push(event);
    }

    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str) {
        self.events
            .lock()
            .await
            .push(ConsoleEffect::ProtocolStarted {
                protocol_id,
                protocol_type: protocol_type.to_string(),
            });
    }

    async fn protocol_completed(&self, protocol_id: Uuid, duration_ms: u64) {
        self.events
            .lock()
            .await
            .push(ConsoleEffect::ProtocolCompleted {
                protocol_id,
                duration_ms,
            });
    }

    async fn protocol_failed(&self, protocol_id: Uuid, error: &str) {
        self.events
            .lock()
            .await
            .push(ConsoleEffect::ProtocolFailed {
                protocol_id,
                error: error.to_string(),
            });
    }

    async fn log_info(&self, message: &str) {
        self.events.lock().await.push(ConsoleEffect::LogInfo {
            message: message.to_string(),
        });
    }

    async fn log_warning(&self, message: &str) {
        self.events.lock().await.push(ConsoleEffect::LogWarning {
            message: message.to_string(),
        });
    }

    async fn log_error(&self, message: &str) {
        self.events.lock().await.push(ConsoleEffect::LogError {
            message: message.to_string(),
        });
    }

    async fn flush(&self) {
        // No-op for recording
    }
}
