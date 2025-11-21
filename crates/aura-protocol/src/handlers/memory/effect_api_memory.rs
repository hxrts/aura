//! Memory-based effect_api handler for testing

use crate::effects::{EffectApiEffects, EffectApiError, EffectApiEventStream};
use async_trait::async_trait;
use aura_core::effects::{RandomEffects, TimeEffects};
use std::sync::Arc;

/// Memory-based effect_api handler for testing
pub struct MemoryLedgerHandler {
    // TODO fix - For now, use placeholder data structures
    _events: Vec<Vec<u8>>,
    random: Arc<dyn RandomEffects>,
    time: Arc<dyn TimeEffects>,
}

impl MemoryLedgerHandler {
    /// Create a new memory effect_api handler with explicit effect dependencies.
    ///
    /// # Parameters
    /// - `random`: RandomEffects implementation for UUID generation and secrets
    /// - `time`: TimeEffects implementation for timestamp operations
    ///
    /// This follows Layer 4 orchestration pattern where handlers store effect dependencies
    /// for coordinated multi-effect operations.
    pub fn new(random: Arc<dyn RandomEffects>, time: Arc<dyn TimeEffects>) -> Self {
        Self {
            _events: Vec::new(),
            random,
            time,
        }
    }
}

impl Default for MemoryLedgerHandler {
    fn default() -> Self {
        // Default uses mock handlers for testing
        use aura_effects::{MockRandomHandler, SimulatedTimeHandler};
        Self::new(
            Arc::new(MockRandomHandler::new()),
            Arc::new(SimulatedTimeHandler::new()),
        )
    }
}

#[async_trait]
impl EffectApiEffects for MemoryLedgerHandler {
    // Removed old methods that are no longer part of the trait

    async fn append_event(&self, _event: Vec<u8>) -> Result<(), EffectApiError> {
        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, EffectApiError> {
        Ok(0)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, EffectApiError> {
        Ok(vec![])
    }

    async fn is_device_authorized(
        &self,
        _device_id: aura_core::identifiers::DeviceId,
        _operation: &str,
    ) -> Result<bool, EffectApiError> {
        Ok(true)
    }

    async fn update_device_activity(
        &self,
        _device_id: aura_core::identifiers::DeviceId,
    ) -> Result<(), EffectApiError> {
        Ok(())
    }

    async fn subscribe_to_events(&self) -> Result<EffectApiEventStream, EffectApiError> {
        Err(EffectApiError::NotAvailable)
    }

    async fn would_create_cycle(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, EffectApiError> {
        Ok(false) // Memory implementation assumes no cycles
    }

    async fn find_connected_components(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, EffectApiError> {
        Ok(vec![]) // Memory implementation returns empty components
    }

    async fn topological_sort(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, EffectApiError> {
        Ok(vec![]) // Memory implementation returns empty sort
    }

    async fn shortest_path(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _start: Vec<u8>,
        _end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, EffectApiError> {
        Ok(None) // Memory implementation returns no path
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, EffectApiError> {
        let secret = self.random.random_bytes(length).await;
        Ok(secret)
    }

    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], EffectApiError> {
        Ok(aura_core::hash::hash(data))
    }

    async fn current_timestamp(&self) -> Result<u64, EffectApiError> {
        // Use TimeEffects for testable timestamp operations
        let timestamp = self.time.current_timestamp().await;
        Ok(timestamp)
    }

    async fn effect_api_device_id(
        &self,
    ) -> Result<aura_core::identifiers::DeviceId, EffectApiError> {
        Ok(aura_core::identifiers::DeviceId::new()) // Memory implementation returns a new device ID
    }

    async fn new_uuid(&self) -> Result<uuid::Uuid, EffectApiError> {
        // Use RandomEffects for testable UUID generation
        let bytes = self.random.random_bytes(16).await;
        let uuid_bytes: [u8; 16] =
            bytes
                .try_into()
                .map_err(|_| EffectApiError::CryptoOperationFailed {
                    message: "Failed to generate UUID bytes".to_string(),
                })?;
        Ok(uuid::Uuid::from_bytes(uuid_bytes))
    }
}
