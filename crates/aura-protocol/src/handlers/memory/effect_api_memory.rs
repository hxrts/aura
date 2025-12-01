//! Memory-based effect_api handler for testing

use crate::effects::{EffectApiEffects, EffectApiError, EffectApiEventStream};
use async_trait::async_trait;
use aura_core::effects::{PhysicalTimeEffects, RandomEffects};
use rand::{rngs::StdRng, Rng, RngCore, SeedableRng};
use std::sync::Arc;
use std::sync::Mutex;

/// Memory-based effect_api handler for testing
pub struct MemoryLedgerHandler {
    /// In-memory event log with monotonic epochs (epoch = index + 1)
    events: Mutex<Vec<(u64, Vec<u8>)>>,
    random: Arc<dyn RandomEffects>,
    time: Arc<dyn PhysicalTimeEffects>,
    /// Stable device ID for this handler
    device_id: aura_core::identifiers::DeviceId,
}

impl MemoryLedgerHandler {
    /// Create a new memory effect_api handler with explicit effect dependencies.
    ///
    /// # Parameters
    /// - `random`: RandomEffects implementation for UUID generation and secrets
    /// - `time`: PhysicalTimeEffects implementation for timestamp operations
    /// - `device_id`: Stable device ID for this handler
    ///
    /// This follows Layer 4 orchestration pattern where handlers store effect dependencies
    /// for coordinated multi-effect operations.
    pub fn new(
        random: Arc<dyn RandomEffects>,
        time: Arc<dyn PhysicalTimeEffects>,
        device_id: aura_core::identifiers::DeviceId,
    ) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            random,
            time,
            device_id,
        }
    }
}

impl Default for MemoryLedgerHandler {
    fn default() -> Self {
        // Default uses mock handlers for testing with deterministic device ID
        use aura_effects::time::PhysicalTimeHandler;
        Self::new(
            Arc::new(DeterministicRandom::new([1u8; 32])),
            Arc::new(PhysicalTimeHandler),
            aura_core::identifiers::DeviceId::deterministic_test_id(),
        )
    }
}

/// Deterministic random handler for memory tests
struct DeterministicRandom {
    rng: Mutex<StdRng>,
}

impl DeterministicRandom {
    fn new(seed: [u8; 32]) -> Self {
        Self {
            rng: Mutex::new(StdRng::from_seed(seed)),
        }
    }
}

#[async_trait]
impl RandomEffects for DeterministicRandom {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut rng = self.rng.lock().unwrap();
        let mut bytes = vec![0u8; len];
        rng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut rng = self.rng.lock().unwrap();
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_u64(&self) -> u64 {
        let mut rng = self.rng.lock().unwrap();
        rng.next_u64()
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        let mut rng = self.rng.lock().unwrap();
        rng.gen_range(min..=max)
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        let mut rng = self.rng.lock().unwrap();
        let mut bytes = [0u8; 16];
        rng.fill_bytes(&mut bytes);
        uuid::Uuid::from_bytes(bytes)
    }
}

#[async_trait]
impl EffectApiEffects for MemoryLedgerHandler {
    // Removed old methods that are no longer part of the trait

    async fn append_event(&self, event: Vec<u8>) -> Result<(), EffectApiError> {
        let mut events = self.events.lock().unwrap();
        let next_epoch = (events.len() as u64) + 1;
        events.push((next_epoch, event));
        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, EffectApiError> {
        let events = self.events.lock().unwrap();
        Ok(events.len() as u64)
    }

    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, EffectApiError> {
        let events = self.events.lock().unwrap();
        Ok(events
            .iter()
            .filter(|(e, _)| *e > epoch)
            .map(|(_, bytes)| bytes.clone())
            .collect())
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
        let ts = self
            .time
            .physical_time()
            .await
            .map_err(|err| EffectApiError::Backend {
                error: format!("time unavailable: {err}"),
            })?
            .ts_ms;
        Ok(ts / 1000)
    }

    async fn effect_api_device_id(
        &self,
    ) -> Result<aura_core::identifiers::DeviceId, EffectApiError> {
        Ok(self.device_id)
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
