//! Memory-based effect_api handler for testing

use async_lock::Mutex;
use async_trait::async_trait;
use aura_core::effects::{EffectApiEffects, EffectApiError, EffectApiEvent, EffectApiEventStream};
use aura_core::effects::{PhysicalTimeEffects, RandomCoreEffects, RandomEffects};
use futures::channel::mpsc;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use std::sync::Arc;

type EventLog = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;

/// Memory-based effect_api handler for testing
pub struct MemoryLedgerHandler {
    events: EventLog,
    epoch: Arc<Mutex<u64>>,
    subscribers: Arc<Mutex<Vec<mpsc::UnboundedSender<EffectApiEvent>>>>,
    random: Arc<dyn RandomEffects>,
    time: Arc<dyn PhysicalTimeEffects>,
}

impl MemoryLedgerHandler {
    /// Create a new memory effect_api handler with explicit effect dependencies.
    ///
    /// # Parameters
    /// - `random`: RandomEffects implementation for UUID generation and secrets
    /// - `time`: PhysicalTimeEffects implementation for timestamp operations
    ///
    /// This follows Layer 4 orchestration pattern where handlers store effect dependencies
    /// for coordinated multi-effect operations.
    pub fn new(random: Arc<dyn RandomEffects>, time: Arc<dyn PhysicalTimeEffects>) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            epoch: Arc::new(Mutex::new(0)),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            random,
            time,
        }
    }
}

impl Default for MemoryLedgerHandler {
    fn default() -> Self {
        // Default uses mock handlers for testing
        use aura_effects::time::PhysicalTimeHandler;
        Self::new(
            Arc::new(DeterministicRandom::new([1u8; 32])),
            Arc::new(PhysicalTimeHandler),
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
impl RandomCoreEffects for DeterministicRandom {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut rng = self.rng.lock().await;
        let mut bytes = vec![0u8; len];
        rng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut rng = self.rng.lock().await;
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_u64(&self) -> u64 {
        let mut rng = self.rng.lock().await;
        rng.next_u64()
    }
}

// RandomExtendedEffects is provided by blanket impl in aura_core

#[async_trait]
impl EffectApiEffects for MemoryLedgerHandler {
    async fn append_event(&self, event: Vec<u8>) -> Result<(), EffectApiError> {
        let timestamp = self.current_timestamp().await?;
        let mut events = self.events.lock().await;
        events.push((timestamp, event.clone()));
        // advance epoch monotonically
        let mut epoch = self.epoch.lock().await;
        *epoch = epoch.saturating_add(1);
        let mut subs = self.subscribers.lock().await;
        subs.retain(|tx| {
            tx.unbounded_send(EffectApiEvent::EventAppended {
                epoch: *epoch,
                event: event.clone(),
            })
            .is_ok()
        });
        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, EffectApiError> {
        let epoch = self.epoch.lock().await;
        Ok(*epoch)
    }

    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, EffectApiError> {
        let events = self.events.lock().await;
        Ok(events
            .iter()
            .filter(|(e, _)| *e > epoch)
            .map(|(_, ev)| ev.clone())
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
        let (tx, rx) = mpsc::unbounded();
        {
            let mut subs = self.subscribers.lock().await;
            subs.push(tx);
        }
        Ok(Box::new(Box::pin(rx)))
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
