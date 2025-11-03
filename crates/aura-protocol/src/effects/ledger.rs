//! Ledger effects interface
//!
//! Pure trait definitions for ledger operations used by protocols.

use async_trait::async_trait;
use aura_journal::AccountState;
use aura_types::DeviceId;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Ledger effects for account state management
#[async_trait]
pub trait LedgerEffects: Send + Sync {
    /// Get read access to the account ledger
    async fn read_ledger(&self) -> Result<Arc<RwLock<AccountState>>, LedgerError>;
    
    /// Get write access to the account ledger
    async fn write_ledger(&self) -> Result<Arc<RwLock<AccountState>>, LedgerError>;
    
    /// Get the current account state
    async fn get_account_state(&self) -> Result<AccountState, LedgerError>;
    
    /// Append an event to the ledger
    async fn append_event(&self, event: Vec<u8>) -> Result<(), LedgerError>;
    
    /// Get the current epoch/sequence number
    async fn current_epoch(&self) -> Result<u64, LedgerError>;
    
    /// Get events since a specific epoch
    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, LedgerError>;
    
    /// Check if a device is authorized for an operation
    async fn is_device_authorized(&self, device_id: DeviceId, operation: &str) -> Result<bool, LedgerError>;
    
    /// Get device metadata
    async fn get_device_metadata(&self, device_id: DeviceId) -> Result<Option<DeviceMetadata>, LedgerError>;
    
    /// Update device last seen timestamp
    async fn update_device_activity(&self, device_id: DeviceId) -> Result<(), LedgerError>;
    
    /// Subscribe to ledger events
    async fn subscribe_to_events(&self) -> Result<LedgerEventStream, LedgerError>;
}

/// Ledger-related errors
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("Ledger not available")]
    NotAvailable,
    
    #[error("Access denied for operation: {operation}")]
    AccessDenied { operation: String },
    
    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: DeviceId },
    
    #[error("Invalid event format")]
    InvalidEvent,
    
    #[error("Epoch out of range: {epoch}")]
    EpochOutOfRange { epoch: u64 },
    
    #[error("Ledger corrupted: {reason}")]
    Corrupted { reason: String },
    
    #[error("Concurrent access conflict")]
    ConcurrentAccess,
    
    #[error("Backend error: {source}")]
    Backend { source: Box<dyn std::error::Error + Send + Sync> },
}

/// Device metadata from ledger
#[derive(Debug, Clone)]
pub struct DeviceMetadata {
    pub device_id: DeviceId,
    pub name: String,
    pub last_seen: u64,
    pub is_active: bool,
    pub permissions: Vec<String>,
}

/// Stream of ledger events
pub type LedgerEventStream = Box<dyn futures::Stream<Item = LedgerEvent> + Send + Unpin>;

/// Ledger events
#[derive(Debug, Clone)]
pub enum LedgerEvent {
    /// New event appended
    EventAppended { epoch: u64, event: Vec<u8> },
    /// Device activity updated
    DeviceActivity { device_id: DeviceId, last_seen: u64 },
    /// Account state changed
    StateChanged,
}