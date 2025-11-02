//! Console visualization effects
//!
//! This module treats console visualization as an algebraic effect,
//! allowing protocols to emit visualization events without directly
//! coupling to the console implementation.

use crate::protocols::choreographic::ChoreoEvent;
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
    async fn emit_choreo_event(&self, event: ChoreoEvent);

    /// Mark a protocol as started
    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str);

    /// Mark a protocol as completed
    async fn protocol_completed(&self, protocol_id: Uuid, success: bool);

    /// Emit a custom visualization marker
    async fn emit_marker(&self, marker_type: &str, data: serde_json::Value);

    /// Flush any buffered events
    async fn flush(&self);
}

/// No-op implementation for when console is not available
pub struct NoOpConsoleEffects;

#[async_trait]
impl ConsoleEffects for NoOpConsoleEffects {
    async fn emit_choreo_event(&self, _event: ChoreoEvent) {}
    async fn protocol_started(&self, _protocol_id: Uuid, _protocol_type: &str) {}
    async fn protocol_completed(&self, _protocol_id: Uuid, _success: bool) {}
    async fn emit_marker(&self, _marker_type: &str, _data: serde_json::Value) {}
    async fn flush(&self) {}
}

/// Channel-based console effects for async event streaming
pub struct ChannelConsoleEffects {
    sender: mpsc::UnboundedSender<ConsoleEffect>,
}

/// Console effect event types
#[derive(Debug, Clone)]
pub enum ConsoleEffect {
    ChoreoEvent(ChoreoEvent),
    ProtocolStarted {
        protocol_id: Uuid,
        protocol_type: String,
    },
    ProtocolCompleted {
        protocol_id: Uuid,
        success: bool,
    },
    Marker {
        marker_type: String,
        data: serde_json::Value,
    },
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
    async fn emit_choreo_event(&self, event: ChoreoEvent) {
        let _ = self.sender.send(ConsoleEffect::ChoreoEvent(event));
    }

    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str) {
        let _ = self.sender.send(ConsoleEffect::ProtocolStarted {
            protocol_id,
            protocol_type: protocol_type.to_string(),
        });
    }

    async fn protocol_completed(&self, protocol_id: Uuid, success: bool) {
        let _ = self.sender.send(ConsoleEffect::ProtocolCompleted {
            protocol_id,
            success,
        });
    }

    async fn emit_marker(&self, marker_type: &str, data: serde_json::Value) {
        let _ = self.sender.send(ConsoleEffect::Marker {
            marker_type: marker_type.to_string(),
            data,
        });
    }

    async fn flush(&self) {
        let _ = self.sender.send(ConsoleEffect::Flush);
    }
}

/// Recording console effects for testing and replay
pub struct RecordingConsoleEffects {
    events: Arc<tokio::sync::Mutex<Vec<ConsoleEffect>>>,
}

impl Default for RecordingConsoleEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingConsoleEffects {
    pub fn new() -> Self {
        Self {
            events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    pub async fn get_events(&self) -> Vec<ConsoleEffect> {
        self.events.lock().await.clone()
    }

    pub async fn clear(&self) {
        self.events.lock().await.clear();
    }
}

#[async_trait]
impl ConsoleEffects for RecordingConsoleEffects {
    async fn emit_choreo_event(&self, event: ChoreoEvent) {
        self.events
            .lock()
            .await
            .push(ConsoleEffect::ChoreoEvent(event));
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

    async fn protocol_completed(&self, protocol_id: Uuid, success: bool) {
        self.events
            .lock()
            .await
            .push(ConsoleEffect::ProtocolCompleted {
                protocol_id,
                success,
            });
    }

    async fn emit_marker(&self, marker_type: &str, data: serde_json::Value) {
        self.events.lock().await.push(ConsoleEffect::Marker {
            marker_type: marker_type.to_string(),
            data,
        });
    }

    async fn flush(&self) {
        // No-op for recording
    }
}
