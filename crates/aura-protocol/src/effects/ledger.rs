//! Ledger effects interface
//!
//! Pure trait definitions for ledger operations used by protocols.
//!
//! Note: This trait works with serialized event data to avoid circular dependencies
//! with aura-journal. Implementations can internally use richer types like AccountState.

use async_trait::async_trait;
use aura_core::DeviceId;

/// Ledger effects for account state management
///
/// This trait combines traditional ledger operations (events, devices, authorization)
/// with journal graph operations (nodes, edges, CRDT merging) needed for the journal's
/// graph-based threshold identity structure.
#[async_trait]
pub trait LedgerEffects: Send + Sync {
    // Traditional Ledger Operations

    /// Append an event to the ledger
    async fn append_event(&self, event: Vec<u8>) -> Result<(), LedgerError>;

    /// Get the current epoch/sequence number
    async fn current_epoch(&self) -> Result<u64, LedgerError>;

    /// Get events since a specific epoch
    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, LedgerError>;

    /// Check if a device is authorized for an operation
    async fn is_device_authorized(
        &self,
        device_id: DeviceId,
        operation: &str,
    ) -> Result<bool, LedgerError>;

    /// Get device metadata
    async fn get_device_metadata(
        &self,
        device_id: DeviceId,
    ) -> Result<Option<DeviceMetadata>, LedgerError>;

    /// Update device last seen timestamp
    async fn update_device_activity(&self, device_id: DeviceId) -> Result<(), LedgerError>;

    /// Subscribe to ledger events
    async fn subscribe_to_events(&self) -> Result<LedgerEventStream, LedgerError>;

    // Journal Graph Operations
    // Operations for managing the journal's graph-based threshold identity structure

    /// Check if adding an edge would create a cycle in the journal graph
    async fn would_create_cycle(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, LedgerError>;

    /// Find strongly connected components in the journal graph
    async fn find_connected_components(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, LedgerError>;

    /// Find topological ordering of nodes in the journal graph
    async fn topological_sort(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, LedgerError>;

    /// Calculate shortest path between two nodes in the journal graph
    async fn shortest_path(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        start: Vec<u8>,
        end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, LedgerError>;

    /// Generate a random secret for cryptographic operations
    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, LedgerError>;

    /// Hash data with Blake3
    async fn hash_blake3(&self, data: &[u8]) -> Result<[u8; 32], LedgerError>;

    /// Get current timestamp (seconds since Unix epoch)
    async fn current_timestamp(&self) -> Result<u64, LedgerError>;

    /// Get device ID for this ledger instance
    async fn ledger_device_id(&self) -> Result<DeviceId, LedgerError>;

    /// Generate a new UUID
    async fn new_uuid(&self) -> Result<uuid::Uuid, LedgerError>;
}

/// Ledger-related errors
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// Ledger is not available
    #[error("Ledger not available")]
    NotAvailable,

    /// Access denied for the requested operation
    #[error("Access denied for operation: {operation}")]
    AccessDenied {
        /// The operation that was denied
        operation: String,
    },

    /// Device not found in ledger
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

    /// Ledger data is corrupted
    #[error("Ledger corrupted: {reason}")]
    Corrupted {
        /// Reason for corruption
        reason: String,
    },

    /// Concurrent access conflict detected
    #[error("Concurrent access conflict")]
    ConcurrentAccess,

    /// Backend storage error
    #[error("Backend error: {source}")]
    Backend {
        /// The underlying backend error
        source: Box<dyn std::error::Error + Send + Sync>,
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

/// Device metadata from ledger
#[derive(Debug, Clone)]
pub struct DeviceMetadata {
    /// Unique device identifier
    pub device_id: DeviceId,
    /// Human-readable device name
    pub name: String,
    /// Last activity timestamp (epoch seconds)
    pub last_seen: u64,
    /// Whether the device is currently active
    pub is_active: bool,
    /// List of granted permissions
    pub permissions: Vec<String>,
}

/// Stream of ledger events
pub type LedgerEventStream = Box<dyn futures::Stream<Item = LedgerEvent> + Send + Unpin>;

/// Ledger events
#[derive(Debug, Clone)]
pub enum LedgerEvent {
    /// New event appended to ledger
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
