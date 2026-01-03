//! Common message framework for aura-sync protocols
//!
//! This module provides unified message patterns and building blocks used across
//! all sync protocols. It eliminates duplication and provides consistent interfaces.
//!
//! **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.

use aura_core::time::PhysicalTime;
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
        Self {
            session_id,
            payload,
        }
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
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampedMessage<T> {
    /// Timestamp when message was created (unified time system)
    pub timestamp: PhysicalTime,
    /// The actual message payload
    pub payload: T,
}

impl<T> TimestampedMessage<T> {
    /// Create a new timestamped message with provided timestamp
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn new(timestamp: PhysicalTime, payload: T) -> Self {
        Self { timestamp, payload }
    }

    /// Create a timestamped message from milliseconds timestamp
    ///
    /// Convenience constructor for backward compatibility.
    pub fn new_from_ms(timestamp_ms: u64, payload: T) -> Self {
        Self::new(
            PhysicalTime {
                ts_ms: timestamp_ms,
                uncertainty: None,
            },
            payload,
        )
    }

    /// Extract the payload
    pub fn into_payload(self) -> T {
        self.payload
    }

    /// Get timestamp in milliseconds for backward compatibility
    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp.ts_ms
    }

    /// Get age of this message in milliseconds
    pub fn age_ms(&self, current_time: &PhysicalTime) -> u64 {
        current_time.ts_ms.saturating_sub(self.timestamp.ts_ms)
    }

    /// Get age of this message in seconds
    pub fn age_seconds(&self, current_timestamp_secs: u64) -> u64 {
        let current_ms = current_timestamp_secs * 1000;
        current_ms.saturating_sub(self.timestamp.ts_ms) / 1000
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
    pub operations_synced: u64,
    /// Optional protocol-specific data
    pub data: Option<T>,
    /// Error message if sync failed
    pub error: Option<String>,
    /// Duration of the sync operation in milliseconds
    pub duration_ms: u64,
}

impl<T> SyncResult<T> {
    /// Create a successful sync result
    pub fn success(operations_synced: u64, data: Option<T>, duration_ms: u64) -> Self {
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
    pub fn partial(operations_synced: u64, error: String, duration_ms: u64) -> Self {
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
    pub total_items: u64,
    /// This batch's sequence number (0-indexed)
    pub sequence: u64,
    /// Whether this is the final batch
    pub is_final: bool,
}

impl<T> BatchMessage<T> {
    /// Create a new batch message
    pub fn new(
        batch_id: Uuid,
        items: Vec<T>,
        total_items: u64,
        sequence: u64,
        is_final: bool,
    ) -> Self {
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
    pub fn create_batches(items: Vec<T>, batch_size: u32, batch_uuid: Uuid) -> Vec<BatchMessage<T>>
    where
        T: Clone,
    {
        if batch_size == 0 {
            return Vec::new();
        }

        let total_items = items.len() as u64;
        let batch_id = batch_uuid;
        let batch_size_usize = batch_size as usize;

        items
            .chunks(batch_size_usize)
            .enumerate()
            .map(|(sequence, chunk)| {
                let sequence = sequence as u64;
                let is_final = (sequence + 1) * batch_size as u64 >= total_items;
                BatchMessage::new(batch_id, chunk.to_vec(), total_items, sequence, is_final)
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
    use aura_testkit::builders::test_device_id;

    fn test_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_session_message() {
        let session_id = SessionId::new_from_entropy([7u8; 32]);
        let msg = SessionMessage::new(session_id, "test data".to_string());

        assert_eq!(msg.session_id, session_id);
        assert_eq!(msg.payload(), "test data");

        let mapped = msg.map(|s| s.len());
        assert_eq!(mapped.payload, 9);
    }

    #[test]
    fn test_timestamped_message() {
        let msg = TimestampedMessage::new(test_time(0), "test".to_string());
        assert_eq!(msg.age_ms(&test_time(1000)), 1000); // Age should be 1000ms

        let old_msg = TimestampedMessage::new_from_ms(0, "old".to_string());
        // Use deterministic timestamp
        let current_time = test_time(2_000_000); // 2000 seconds in ms
        assert!(old_msg.age_ms(&current_time) > 1_000_000); // Very old
    }

    #[test]
    fn test_request_response_flow() {
        let from = test_device_id(1);
        let to = test_device_id(2);
        let request_id = uuid::Uuid::from_bytes(1u128.to_be_bytes());

        let request = RequestMessage::new(from, to, "ping".to_string(), request_id);
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
        let batch_id = uuid::Uuid::from_bytes(2u128.to_be_bytes());
        let batches = BatchMessage::create_batches(items, 3, batch_id);

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].items, vec![1, 2, 3]);
        assert_eq!(batches[1].items, vec![4, 5, 6]);
        assert_eq!(batches[2].items, vec![7]);
        assert!(batches[2].is_final);
    }

    #[test]
    fn test_progress_message() {
        let op_id = Uuid::from_bytes(3u128.to_be_bytes());
        let progress = ProgressMessage::new(op_id, 0.5, "Processing".to_string())
            .with_eta(300)
            .with_metadata("items", "100");

        assert_eq!(progress.progress, 0.5);
        assert!(!progress.is_complete());
        assert_eq!(progress.eta_seconds, Some(300));
        assert_eq!(progress.metadata.get("items"), Some(&"100".to_string()));
    }
}
