//! Composite effect handler
//!
//! Combines multiple effect handlers into a single unified handler that implements all effect traits.

use crate::effects::*;
use crate::handlers::{
    console::{SilentConsoleHandler, StdoutConsoleHandler},
    crypto::{MockCryptoHandler, RealCryptoHandler},
    network::{MemoryNetworkHandler, RealNetworkHandler, SimulatedNetworkHandler},
    storage::{MemoryStorageHandler, FilesystemStorageHandler},
    time::{RealTimeHandler, SimulatedTimeHandler},
};
use async_trait::async_trait;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use std::sync::Arc;
use uuid::Uuid;

/// Composite handler that implements all effect traits
pub struct CompositeHandler {
    device_id: Uuid,
    is_simulation: bool,
    network: Box<dyn NetworkEffects>,
    storage: Box<dyn StorageEffects>,
    crypto: Box<dyn CryptoEffects>,
    time: Box<dyn TimeEffects>,
    console: Box<dyn ConsoleEffects>,
    // Note: LedgerEffects and ChoreographicEffects will be added when their handlers are implemented
}

impl CompositeHandler {
    /// Create a composite handler for testing with all mock/memory implementations
    pub fn for_testing(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(MemoryNetworkHandler::new(device_id)),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::new(42)),
            time: Box::new(SimulatedTimeHandler::new()),
            console: Box::new(SilentConsoleHandler::new()),
        }
    }

    /// Create a composite handler for production with real implementations
    pub fn for_production(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: false,
            network: Box::new(RealNetworkHandler::new(device_id, "tcp://0.0.0.0:0".to_string())),
            storage: Box::new(FilesystemStorageHandler::new("/tmp/aura".into()).unwrap()),
            crypto: Box::new(RealCryptoHandler::new()),
            time: Box::new(RealTimeHandler::new()),
            console: Box::new(StdoutConsoleHandler::new()),
        }
    }

    /// Create a composite handler for simulation/deterministic testing
    pub fn for_simulation(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(SimulatedNetworkHandler::new(device_id)),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::new(device_id.as_u128() as u64)),
            time: Box::new(SimulatedTimeHandler::new()),
            console: Box::new(SilentConsoleHandler::new()),
        }
    }

    /// Builder for custom composite handlers
    pub fn builder(device_id: Uuid) -> CompositeHandlerBuilder {
        CompositeHandlerBuilder::new(device_id)
    }
}

// Implement all effect traits by delegating to the individual handlers

#[async_trait]
impl NetworkEffects for CompositeHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        self.network.send_to_peer(peer_id, message).await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        self.network.broadcast(message).await
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        self.network.receive().await
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        self.network.receive_from(peer_id).await
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        self.network.connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        self.network.is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        self.network.subscribe_to_peer_events().await
    }
}

#[async_trait]
impl StorageEffects for CompositeHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.storage.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.storage.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.storage.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.storage.list_keys(prefix).await
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.storage.exists(key).await
    }

    async fn store_batch(&self, pairs: std::collections::HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        self.storage.store_batch(pairs).await
    }

    async fn retrieve_batch(&self, keys: &[String]) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        self.storage.retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        self.storage.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.storage.stats().await
    }
}

#[async_trait]
impl CryptoEffects for CompositeHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.crypto.random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        self.crypto.random_bytes_32().await
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        self.crypto.random_range(range).await
    }

    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        self.crypto.blake3_hash(data).await
    }

    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        self.crypto.sha256_hash(data).await
    }

    async fn ed25519_sign(&self, data: &[u8], key: &SigningKey) -> Result<Signature, CryptoError> {
        self.crypto.ed25519_sign(data, key).await
    }

    async fn ed25519_verify(&self, data: &[u8], signature: &Signature, public_key: &VerifyingKey) -> Result<bool, CryptoError> {
        self.crypto.ed25519_verify(data, signature, public_key).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(SigningKey, VerifyingKey), CryptoError> {
        self.crypto.ed25519_generate_keypair().await
    }

    async fn ed25519_public_key(&self, private_key: &SigningKey) -> VerifyingKey {
        self.crypto.ed25519_public_key(private_key).await
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.secure_zero(data)
    }
}

#[async_trait]
impl TimeEffects for CompositeHandler {
    async fn current_epoch(&self) -> u64 {
        self.time.current_epoch().await
    }

    async fn sleep_ms(&self, ms: u64) {
        self.time.sleep_ms(ms).await
    }

    async fn sleep_until(&self, epoch: u64) {
        self.time.sleep_until(epoch).await
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        self.time.yield_until(condition).await
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        self.time.set_timeout(timeout_ms).await
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        self.time.cancel_timeout(handle).await
    }

    fn is_simulated(&self) -> bool {
        self.time.is_simulated()
    }

    fn register_context(&self, context_id: Uuid) {
        self.time.register_context(context_id)
    }

    fn unregister_context(&self, context_id: Uuid) {
        self.time.unregister_context(context_id)
    }

    async fn notify_events_available(&self) {
        self.time.notify_events_available().await
    }

    fn resolution_ms(&self) -> u64 {
        self.time.resolution_ms()
    }
}

#[async_trait]
impl ConsoleEffects for CompositeHandler {
    async fn emit_choreo_event(&self, event: ConsoleEffect) {
        self.console.emit_choreo_event(event).await
    }

    async fn protocol_started(&self, protocol_id: Uuid, protocol_type: &str) {
        self.console.protocol_started(protocol_id, protocol_type).await
    }

    async fn protocol_completed(&self, protocol_id: Uuid, duration_ms: u64) {
        self.console.protocol_completed(protocol_id, duration_ms).await
    }

    async fn protocol_failed(&self, protocol_id: Uuid, error: &str) {
        self.console.protocol_failed(protocol_id, error).await
    }

    async fn log_info(&self, message: &str) {
        self.console.log_info(message).await
    }

    async fn log_warning(&self, message: &str) {
        self.console.log_warning(message).await
    }

    async fn log_error(&self, message: &str) {
        self.console.log_error(message).await
    }

    async fn flush(&self) {
        self.console.flush().await
    }
}

impl ProtocolEffects for CompositeHandler {
    fn device_id(&self) -> Uuid {
        self.device_id
    }

    fn is_simulation(&self) -> bool {
        self.is_simulation
    }
}

impl MinimalEffects for CompositeHandler {
    fn device_id(&self) -> Uuid {
        self.device_id
    }
}

/// Builder for creating custom composite handlers
pub struct CompositeHandlerBuilder {
    device_id: Uuid,
    is_simulation: bool,
    network: Option<Box<dyn NetworkEffects>>,
    storage: Option<Box<dyn StorageEffects>>,
    crypto: Option<Box<dyn CryptoEffects>>,
    time: Option<Box<dyn TimeEffects>>,
    console: Option<Box<dyn ConsoleEffects>>,
}

impl CompositeHandlerBuilder {
    fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            is_simulation: false,
            network: None,
            storage: None,
            crypto: None,
            time: None,
            console: None,
        }
    }

    pub fn simulation(mut self) -> Self {
        self.is_simulation = true;
        self
    }

    pub fn with_network(mut self, network: Box<dyn NetworkEffects>) -> Self {
        self.network = Some(network);
        self
    }

    pub fn with_storage(mut self, storage: Box<dyn StorageEffects>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn with_crypto(mut self, crypto: Box<dyn CryptoEffects>) -> Self {
        self.crypto = Some(crypto);
        self
    }

    pub fn with_time(mut self, time: Box<dyn TimeEffects>) -> Self {
        self.time = Some(time);
        self
    }

    pub fn with_console(mut self, console: Box<dyn ConsoleEffects>) -> Self {
        self.console = Some(console);
        self
    }

    pub fn build(self) -> CompositeHandler {
        let device_id = self.device_id;
        let is_simulation = self.is_simulation;

        CompositeHandler {
            device_id,
            is_simulation,
            network: self.network.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MemoryNetworkHandler::new(device_id))
                } else {
                    Box::new(RealNetworkHandler::new(device_id, "tcp://0.0.0.0:0".to_string()))
                }
            }),
            storage: self.storage.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MemoryStorageHandler::new())
                } else {
                    Box::new(FilesystemStorageHandler::new("/tmp/aura".into()).unwrap())
                }
            }),
            crypto: self.crypto.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MockCryptoHandler::new(device_id.as_u128() as u64))
                } else {
                    Box::new(RealCryptoHandler::new())
                }
            }),
            time: self.time.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(SimulatedTimeHandler::new())
                } else {
                    Box::new(RealTimeHandler::new())
                }
            }),
            console: self.console.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(SilentConsoleHandler::new())
                } else {
                    Box::new(StdoutConsoleHandler::new())
                }
            }),
        }
    }
}

// Implement LedgerEffects with placeholder delegation
#[async_trait]
impl LedgerEffects for CompositeHandler {
    async fn read_ledger(&self) -> Result<Arc<tokio::sync::RwLock<aura_journal::AccountState>>, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn write_ledger(&self) -> Result<Arc<tokio::sync::RwLock<aura_journal::AccountState>>, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn get_account_state(&self) -> Result<aura_journal::AccountState, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn append_event(&self, _event: Vec<u8>) -> Result<(), LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn current_epoch(&self) -> Result<u64, LedgerError> {
        Ok(self.time.current_epoch().await)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn is_device_authorized(&self, _device_id: aura_types::DeviceId, _operation: &str) -> Result<bool, LedgerError> {
        Ok(true) // Placeholder - always authorized
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

// Implement ChoreographicEffects with placeholder delegation
#[async_trait]
impl ChoreographicEffects for CompositeHandler {
    async fn send_to_role_bytes(&self, role: ChoreographicRole, message: Vec<u8>) -> Result<(), ChoreographyError> {
        self.network.send_to_peer(role.device_id, message).await
            .map_err(|e| ChoreographyError::Transport { source: Box::new(e) })
    }

    async fn receive_from_role_bytes(&self, role: ChoreographicRole) -> Result<Vec<u8>, ChoreographyError> {
        self.network.receive_from(role.device_id).await
            .map_err(|e| ChoreographyError::Transport { source: Box::new(e) })
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        self.network.broadcast(message).await
            .map_err(|e| ChoreographyError::Transport { source: Box::new(e) })
    }

    fn current_role(&self) -> ChoreographicRole {
        ChoreographicRole {
            device_id: self.device_id,
            role_index: 0,
        }
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        vec![self.current_role()]
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        self.network.is_peer_connected(role.device_id).await
    }

    async fn start_session(&self, _session_id: Uuid, _roles: Vec<ChoreographicRole>) -> Result<(), ChoreographyError> {
        Ok(())
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        Ok(())
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        // Convert to console event
        let console_event = ConsoleEffect::ChoreoEvent(serde_json::to_value(event).unwrap_or_default());
        self.console.emit_choreo_event(console_event).await;
        Ok(())
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        let _ = self.time.set_timeout(timeout_ms).await;
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        ChoreographyMetrics {
            messages_sent: 0,
            messages_received: 0,
            avg_latency_ms: 0.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 0,
        }
    }
}