//! Effect API effects interface
//!
//! Pure trait definitions for effect_api operations used by protocols.
//!
//! Note: This trait works with serialized event data to avoid circular dependencies
//! with aura-journal. Implementations can internally use richer types like AccountState.

use async_trait::async_trait;
use aura_core::identifiers::DeviceId;

/// Effect API effects for account state management
///
/// This trait combines traditional effect_api operations (events, devices, authorization)
/// with journal graph operations (nodes, edges, CRDT merging) needed for the journal's
/// graph-based threshold identity structure.
#[async_trait]
pub trait EffectApiEffects: Send + Sync {
    // Traditional Effect API Operations

    /// Append an event to the effect_api
    async fn append_event(&self, event: Vec<u8>) -> Result<(), EffectApiError>;

    /// Get the current epoch/sequence number
    async fn current_epoch(&self) -> Result<u64, EffectApiError>;

    /// Get events since a specific epoch
    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, EffectApiError>;

    /// Check if a device is authorized for an operation
    async fn is_device_authorized(
        &self,
        device_id: DeviceId,
        operation: &str,
    ) -> Result<bool, EffectApiError>;

    /// Update device last seen timestamp
    async fn update_device_activity(&self, device_id: DeviceId) -> Result<(), EffectApiError>;

    /// Subscribe to effect_api events
    async fn subscribe_to_events(&self) -> Result<EffectApiEventStream, EffectApiError>;

    // Journal Graph Operations
    // Operations for managing the journal's graph-based threshold identity structure

    /// Check if adding an edge would create a cycle in the journal graph
    async fn would_create_cycle(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, EffectApiError>;

    /// Find strongly connected components in the journal graph
    async fn find_connected_components(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, EffectApiError>;

    /// Find topological ordering of nodes in the journal graph
    async fn topological_sort(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, EffectApiError>;

    /// Calculate shortest path between two nodes in the journal graph
    async fn shortest_path(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        start: Vec<u8>,
        end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, EffectApiError>;

    /// Generate a random secret for cryptographic operations
    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, EffectApiError>;

    /// Hash data with cryptographic hash function
    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], EffectApiError>;

    /// Get current timestamp (seconds since Unix epoch)
    async fn current_timestamp(&self) -> Result<u64, EffectApiError>;

    /// Get device ID for this effect_api instance
    async fn effect_api_device_id(&self) -> Result<DeviceId, EffectApiError>;

    /// Generate a new UUID
    async fn new_uuid(&self) -> Result<uuid::Uuid, EffectApiError>;
}

/// Effect API-related errors
#[derive(Debug, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum EffectApiError {
    /// Effect API is not available
    #[error("Effect API not available")]
    NotAvailable,

    /// Access denied for the requested operation
    #[error("Access denied for operation: {operation}")]
    AccessDenied {
        /// The operation that was denied
        operation: String,
    },

    /// Device not found in effect_api
    #[error("Device not found: {device_id}")]
    DeviceNotFound {
        /// The device ID that was not found
        device_id: DeviceId,
    },

    /// Event has invalid format
    #[error("Invalid event format")]
    InvalidEvent,

    /// Requested epoch is out of range
    #[error("Epoch out of range: {epoch}")]
    EpochOutOfRange {
        /// The invalid epoch number
        epoch: u64,
    },

    /// Effect API data is corrupted
    #[error("Effect API corrupted: {reason}")]
    Corrupted {
        /// Reason for corruption
        reason: String,
    },

    /// Concurrent access conflict detected
    #[error("Concurrent access conflict")]
    ConcurrentAccess,

    /// Backend storage error
    #[error("Backend error: {error}")]
    Backend {
        /// Backend error message
        error: String,
    },

    /// Journal graph operation failed
    #[error("Graph operation failed: {message}")]
    GraphOperationFailed {
        /// Description of the failure
        message: String,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic operation failed: {message}")]
    CryptoOperationFailed {
        /// Description of the failure
        message: String,
    },

    /// Invalid node or edge data
    #[error("Invalid graph data: {message}")]
    InvalidGraphData {
        /// Description of invalid data
        message: String,
    },
}

impl aura_core::ProtocolErrorCode for EffectApiError {
    fn code(&self) -> &'static str {
        match self {
            EffectApiError::NotAvailable => "effect_api_not_available",
            EffectApiError::AccessDenied { .. } => "effect_api_access_denied",
            EffectApiError::DeviceNotFound { .. } => "effect_api_device_not_found",
            EffectApiError::InvalidEvent => "effect_api_invalid_event",
            EffectApiError::EpochOutOfRange { .. } => "effect_api_epoch_out_of_range",
            EffectApiError::Corrupted { .. } => "effect_api_corrupted",
            EffectApiError::ConcurrentAccess => "effect_api_concurrent_access",
            EffectApiError::Backend { .. } => "effect_api_backend",
            EffectApiError::GraphOperationFailed { .. } => "effect_api_graph_operation",
            EffectApiError::CryptoOperationFailed { .. } => "effect_api_crypto_operation",
            EffectApiError::InvalidGraphData { .. } => "effect_api_invalid_graph_data",
        }
    }
}

/// Effect API events
#[derive(Debug, Clone)]
pub enum EffectApiEvent {
    /// New event appended to effect_api
    EventAppended {
        /// Epoch number of the new event
        epoch: u64,
        /// The event data
        event: Vec<u8>,
    },
    /// Device activity timestamp updated
    DeviceActivity {
        /// The device that was active
        device_id: DeviceId,
        /// Updated last seen timestamp
        last_seen: u64,
    },
    /// Account state has changed
    StateChanged,
}

/// Stream of effect_api events
pub type EffectApiEventStream = Box<dyn futures::Stream<Item = EffectApiEvent> + Send + Unpin>;
