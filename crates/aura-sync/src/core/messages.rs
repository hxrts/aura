//! Common message framework for aura-sync protocols
//!
//! This module provides unified message patterns and building blocks used across
//! all sync protocols. It eliminates duplication and provides consistent interfaces.

use aura_core::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

/// Common trait for protocol message pairs
pub trait ProtocolMessage: Send + Sync + Clone + Debug {
    /// Request message type
    type Request: Send + Sync + Clone + Debug + for<'de> Deserialize<'de> + Serialize;
    /// Response message type
    type Response: Send + Sync + Clone + Debug + for<'de> Deserialize<'de> + Serialize;
    /// Error type for protocol operations
    type Error: std::error::Error + Send + Sync;
}

/// Session-scoped message wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage<T> {
    /// Session identifier for this message exchange
    pub session_id: SessionId,
    /// The actual message payload
    pub payload: T,
}

impl<T> SessionMessage<T> {
    /// Create a new session message
    pub fn new(session_id: SessionId, payload: T) -> Self {
        Self { session_id, payload }
    }

    /// Extract the payload, consuming the wrapper
    pub fn into_payload(self) -> T {
        self.payload
    }

    /// Get a reference to the payload
    pub fn payload(&self) -> &T {
        &self.payload
    }

    /// Map the payload to a different type
    pub fn map<U, F>(self, f: F) -> SessionMessage<U>
    where
        F: FnOnce(T) -> U,
    {
        SessionMessage {
            session_id: self.session_id,
            payload: f(self.payload),
        }
    }
}

/// Timestamped message for ordering and replay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampedMessage<T> {
    /// Unix timestamp when message was created
    pub timestamp: u64,
    /// The actual message payload
    pub payload: T,
}

impl<T> TimestampedMessage<T> {
    /// Create a new timestamped message with current time
    pub fn new(payload: T) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            payload,
        }
    }

    /// Create a timestamped message with specific time
    pub fn with_timestamp(timestamp: u64, payload: T) -> Self {
        Self { timestamp, payload }
    }

    /// Extract the payload
    pub fn into_payload(self) -> T {
        self.payload
    }

    /// Get age of this message in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.timestamp)
    }
}

/// Peer-addressed message for multi-device protocols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMessage<T> {
    /// Source device
    pub from: DeviceId,
    /// Destination device
    pub to: DeviceId,
    /// The message payload
    pub payload: T,
}

impl<T> PeerMessage<T> {
    /// Create a new peer message
    pub fn new(from: DeviceId, to: DeviceId, payload: T) -> Self {
        Self { from, to, payload }
    }

    /// Create a reply message by swapping from/to
    pub fn reply<U>(self, reply_payload: U) -> PeerMessage<U> {
        PeerMessage {
            from: self.to,
            to: self.from,
            payload: reply_payload,
        }
    }
}

/// Request/Response pattern with correlation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMessage<T> {
    /// Unique request identifier for correlation
    pub request_id: Uuid,
    /// Source device making the request
    pub from: DeviceId,
    /// Destination device
    pub to: DeviceId,
    /// Request payload
    pub payload: T,
}

impl<T> RequestMessage<T> {
    /// Create a new request message
    ///
    /// Note: Callers should generate UUIDs via `RandomEffects::random_uuid()` and use `with_id()`
    pub fn new(from: DeviceId, to: DeviceId, payload: T, request_uuid: Uuid) -> Self {
        Self {
            request_id: request_uuid,
            from,
            to,
            payload,
        }
    }

    /// Create request with specific ID (for testing)
    pub fn with_id(request_id: Uuid, from: DeviceId, to: DeviceId, payload: T) -> Self {
        Self {
            request_id,
            from,
            to,
            payload,
        }
    }
}

/// Response message correlated to a request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage<T> {
    /// Request ID this response corresponds to
    pub request_id: Uuid,
    /// Device sending the response
    pub from: DeviceId,
    /// Device that made the original request
    pub to: DeviceId,
    /// Response payload (success or error)
    pub payload: ResponsePayload<T>,
}

impl<T> ResponseMessage<T> {
    /// Create a success response
    pub fn success(request: &RequestMessage<impl Clone>, payload: T) -> Self {
        Self {
            request_id: request.request_id,
            from: request.to,
            to: request.from,
            payload: ResponsePayload::Success(payload),
        }
    }

    /// Create an error response
    pub fn error(request: &RequestMessage<impl Clone>, error: String) -> Self {
        Self {
            request_id: request.request_id,
            from: request.to,
            to: request.from,
            payload: ResponsePayload::Error(error),
        }
    }

    /// Check if this response is successful
    pub fn is_success(&self) -> bool {
        matches!(self.payload, ResponsePayload::Success(_))
    }

    /// Extract success payload if present
    pub fn into_success(self) -> Option<T> {
        match self.payload {
            ResponsePayload::Success(payload) => Some(payload),
            ResponsePayload::Error(_) => None,
        }
    }
}

/// Response payload that can be success or error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponsePayload<T> {
    /// Successful response with data
    Success(T),
    /// Error response with message
    Error(String),
}

/// Common sync result pattern used across protocols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult<T> {
    /// Whether the sync operation succeeded
    pub success: bool,
    /// Number of operations/items synchronized
    pub operations_synced: usize,
    /// Optional protocol-specific data
    pub data: Option<T>,
    /// Error message if sync failed
    pub error: Option<String>,
    /// Duration of the sync operation in milliseconds
    pub duration_ms: u64,
}

impl<T> SyncResult<T> {
    /// Create a successful sync result
    pub fn success(operations_synced: usize, data: Option<T>, duration_ms: u64) -> Self {
        Self {
            success: true,
            operations_synced,
            data,
            error: None,
            duration_ms,
        }
    }

    /// Create a failed sync result
    pub fn failure(error: String, duration_ms: u64) -> Self {
        Self {
            success: false,
            operations_synced: 0,
            data: None,
            error: Some(error),
            duration_ms,
        }
    }

    /// Create a partial success result
    pub fn partial(operations_synced: usize, error: String, duration_ms: u64) -> Self {
        Self {
            success: false,
            operations_synced,
            data: None,
            error: Some(error),
            duration_ms,
        }
    }

    /// Convert to a different data type
    pub fn map<U, F>(self, f: F) -> SyncResult<U>
    where
        F: FnOnce(T) -> U,
    {
        SyncResult {
            success: self.success,
            operations_synced: self.operations_synced,
            data: self.data.map(f),
            error: self.error,
            duration_ms: self.duration_ms,
        }
    }
}

impl<T> Default for SyncResult<T> {
    fn default() -> Self {
        Self::success(0, None, 0)
    }
}

/// Batch message for efficient multi-item operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMessage<T> {
    /// Batch identifier
    pub batch_id: Uuid,
    /// Items in this batch
    pub items: Vec<T>,
    /// Total number of items across all batches
    pub total_items: usize,
    /// This batch's sequence number (0-indexed)
    pub sequence: usize,
    /// Whether this is the final batch
    pub is_final: bool,
}

impl<T> BatchMessage<T> {
    /// Create a new batch message
    pub fn new(batch_id: Uuid, items: Vec<T>, total_items: usize, sequence: usize, is_final: bool) -> Self {
        Self {
            batch_id,
            items,
            total_items,
            sequence,
            is_final,
        }
    }

    /// Create batch messages from a list of items
    ///
    /// Note: Callers should generate UUIDs via `RandomEffects::random_uuid()` and pass them
    pub fn create_batches(items: Vec<T>, batch_size: usize, batch_uuid: Uuid) -> Vec<BatchMessage<T>>
    where
        T: Clone,
    {
        let total_items = items.len();
        let batch_id = batch_uuid;
        
        items
            .chunks(batch_size)
            .enumerate()
            .map(|(sequence, chunk)| {
                let is_final = (sequence + 1) * batch_size >= total_items;
                BatchMessage::new(
                    batch_id,
                    chunk.to_vec(),
                    total_items,
                    sequence,
                    is_final,
                )
            })
            .collect()
    }
}

/// Progress update for long-running sync operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressMessage {
    /// Operation identifier
    pub operation_id: Uuid,
    /// Current progress (0.0 to 1.0)
    pub progress: f32,
    /// Human-readable status message
    pub status: String,
    /// Optional ETA in seconds
    pub eta_seconds: Option<u64>,
    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl ProgressMessage {
    /// Create a new progress message
    pub fn new(operation_id: Uuid, progress: f32, status: String) -> Self {
        Self {
            operation_id,
            progress: progress.clamp(0.0, 1.0),
            status,
            eta_seconds: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add ETA information
    pub fn with_eta(mut self, eta_seconds: u64) -> Self {
        self.eta_seconds = Some(eta_seconds);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Check if operation is complete
    pub fn is_complete(&self) -> bool {
        (self.progress - 1.0).abs() < f32::EPSILON
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use aura_core::test_utils::test_device_id;

    #[test]
    fn test_session_message() {
        let session_id = SessionId::new();
        let msg = SessionMessage::new(session_id, "test data".to_string());
        
        assert_eq!(msg.session_id, session_id);
        assert_eq!(msg.payload(), "test data");
        
        let mapped = msg.map(|s| s.len());
        assert_eq!(mapped.payload, 9);
    }

    #[test]
    fn test_timestamped_message() {
        let msg = TimestampedMessage::new("test".to_string());
        assert!(msg.age_seconds() < 1); // Should be very recent
        
        let old_msg = TimestampedMessage::with_timestamp(0, "old".to_string());
        assert!(old_msg.age_seconds() > 1000); // Very old
    }

    #[test]
    fn test_request_response_flow() {
        let from = test_device_id(1);
        let to = test_device_id(2);
        
        let request = RequestMessage::new(from, to, "ping".to_string());
        let response = ResponseMessage::success(&request, "pong".to_string());
        
        assert_eq!(response.request_id, request.request_id);
        assert_eq!(response.from, to);
        assert_eq!(response.to, from);
        assert!(response.is_success());
        assert_eq!(response.into_success(), Some("pong".to_string()));
    }

    #[test]
    fn test_sync_result() {
        let result = SyncResult::success(5, Some("data".to_string()), 1000);
        assert!(result.success);
        assert_eq!(result.operations_synced, 5);
        
        let mapped = result.map(|s| s.len());
        assert_eq!(mapped.data, Some(4));
    }

    #[test]
    fn test_batch_creation() {
        let items = vec![1, 2, 3, 4, 5, 6, 7];
        let batches = BatchMessage::create_batches(items, 3);
        
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].items, vec![1, 2, 3]);
        assert_eq!(batches[1].items, vec![4, 5, 6]);
        assert_eq!(batches[2].items, vec![7]);
        assert!(batches[2].is_final);
    }

    #[test]
    fn test_progress_message() {
        let op_id = Uuid::new_v4();
        let progress = ProgressMessage::new(op_id, 0.5, "Processing".to_string())
            .with_eta(300)
            .with_metadata("items", "100");
        
        assert_eq!(progress.progress, 0.5);
        assert!(!progress.is_complete());
        assert_eq!(progress.eta_seconds, Some(300));
        assert_eq!(progress.metadata.get("items"), Some(&"100".to_string()));
    }
}