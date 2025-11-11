//! Memory-based ledger handler for testing

use crate::effects::{DeviceMetadata, LedgerEffects, LedgerError, LedgerEventStream};
use async_trait::async_trait;
use rand::RngCore;

/// Memory-based ledger handler for testing
pub struct MemoryLedgerHandler {
    // TODO fix - For now, use placeholder data structures
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

    async fn is_device_authorized(
        &self,
        _device_id: aura_core::DeviceId,
        _operation: &str,
    ) -> Result<bool, LedgerError> {
        Ok(true)
    }

    async fn get_device_metadata(
        &self,
        _device_id: aura_core::DeviceId,
    ) -> Result<Option<DeviceMetadata>, LedgerError> {
        Ok(None)
    }

    async fn update_device_activity(
        &self,
        _device_id: aura_core::DeviceId,
    ) -> Result<(), LedgerError> {
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<LedgerEventStream, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn would_create_cycle(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, LedgerError> {
        Ok(false) // Memory implementation assumes no cycles
    }

    async fn find_connected_components(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, LedgerError> {
        Ok(vec![]) // Memory implementation returns empty components
    }

    async fn topological_sort(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, LedgerError> {
        Ok(vec![]) // Memory implementation returns empty sort
    }

    async fn shortest_path(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _start: Vec<u8>,
        _end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, LedgerError> {
        Ok(None) // Memory implementation returns no path
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, LedgerError> {
        let mut secret = vec![0u8; length];
        rand::thread_rng().fill_bytes(&mut secret);
        Ok(secret)
    }

    async fn hash_blake3(&self, data: &[u8]) -> Result<[u8; 32], LedgerError> {
        Ok(blake3::hash(data).into())
    }

    async fn current_timestamp(&self) -> Result<u64, LedgerError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| LedgerError::Corrupted { reason: "Failed to get current time".to_string() })?;
        Ok(duration.as_secs())
    }

    async fn ledger_device_id(&self) -> Result<aura_core::DeviceId, LedgerError> {
        Ok(aura_core::DeviceId::new()) // Memory implementation returns a new device ID
    }

    async fn new_uuid(&self) -> Result<uuid::Uuid, LedgerError> {
        Ok(uuid::Uuid::new_v4())
    }
}
