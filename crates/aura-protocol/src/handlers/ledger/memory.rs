//! Memory-based ledger handler for testing

use crate::effects::{LedgerEffects, LedgerError, LedgerEventStream, DeviceMetadata};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Memory-based ledger handler for testing
pub struct MemoryLedgerHandler {
    // For now, use placeholder data structures
    _events: Vec<Vec<u8>>,
}

impl MemoryLedgerHandler {
    pub fn new() -> Self {
        Self {
            _events: Vec::new(),
        }
    }
}

impl Default for MemoryLedgerHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LedgerEffects for MemoryLedgerHandler {
    // Removed old methods that are no longer part of the trait

    async fn append_event(&self, _event: Vec<u8>) -> Result<(), LedgerError> {
        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, LedgerError> {
        Ok(0)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, LedgerError> {
        Ok(vec![])
    }

    async fn is_device_authorized(&self, _device_id: aura_types::DeviceId, _operation: &str) -> Result<bool, LedgerError> {
        Ok(true)
    }

    async fn get_device_metadata(&self, _device_id: aura_types::DeviceId) -> Result<Option<DeviceMetadata>, LedgerError> {
        Ok(None)
    }

    async fn update_device_activity(&self, _device_id: aura_types::DeviceId) -> Result<(), LedgerError> {
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<LedgerEventStream, LedgerError> {
        Err(LedgerError::NotAvailable)
    }
}