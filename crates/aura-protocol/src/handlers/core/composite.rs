//! Composite effect handler
//!
//! Combines multiple effect handlers into a single unified handler that implements all effect traits.

#![allow(clippy::disallowed_methods)] // TODO: Replace direct time/UUID calls with effect system

use super::erased::AuraHandler;
use crate::handlers::{
    context_immutable::AuraContext, tree::DummyTreeHandler, AuraHandlerError, EffectType,
    ExecutionMode,
};
// Use standard effect handlers from aura-effects
use aura_effects::{
    console::{MockConsoleHandler, RealConsoleHandler},
    crypto::{MockCryptoHandler, RealCryptoHandler},
    journal::MockJournalHandler,
    storage::{FilesystemStorageHandler, MemoryStorageHandler},
    time::{RealTimeHandler, SimulatedTimeHandler},
    transport::{InMemoryTransportHandler, TcpTransportHandler as RealNetworkHandler},
};
// Import from local crate for extended effect traits
use crate::effects::{
    AuraError, ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics, ConsoleEffects, ConsoleEvent, CryptoEffects, CryptoError, DeviceMetadata,
    LedgerEffects, LedgerError, LedgerEventStream, NetworkEffects, NetworkError, PeerEventStream,
    RandomEffects, StorageEffects, StorageError, StorageStats, SystemEffects, SystemError,
    TimeEffects, TimeError, TimeoutHandle, TreeEffects, WakeCondition,
};
use async_trait::async_trait;
use aura_core::effects::crypto::{FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext};
use aura_core::effects::JournalEffects;
use aura_core::hash::hash;
use aura_core::{identifiers::{AuthorityId, ContextId, DeviceId}, FlowBudget, LocalSessionType};
use serde_json;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Console handlers now imported from aura-effects (MockConsoleHandler, RealConsoleHandler)

// Time handlers now imported from aura-effects (SimulatedTimeHandler, RealTimeHandler)

/// Composite handler that implements all effect traits
pub struct CompositeHandler {
    device_id: Uuid,
    is_simulation: bool,
    network: Box<dyn NetworkEffects>,
    storage: Box<dyn StorageEffects>,
    crypto: Box<dyn CryptoEffects>,
    time: Box<dyn TimeEffects>,
    console: Box<dyn ConsoleEffects>,
    journal: Box<dyn JournalEffects>,
    tree: Box<dyn TreeEffects>,
    // Note: LedgerEffects and ChoreographicEffects will be added when their handlers are implemented
}

impl CompositeHandler {
    /// Create a composite handler based on execution mode
    pub fn for_mode(mode: ExecutionMode, device_id: Uuid) -> Self {
        match mode {
            ExecutionMode::Testing => Self::for_testing(device_id),
            ExecutionMode::Production => Self::for_production(device_id),
            ExecutionMode::Simulation { seed } => {
                // Create simulation-specific handlers with deterministic behavior
                Self::for_simulation_with_seed(device_id, seed)
            }
        }
    }

    /// Create a composite handler for testing with all mock/memory implementations
    pub fn for_testing(device_id: Uuid) -> Self {
        let journal = MockJournalHandler::new();
        // Note: journal doesn't need to be cloned since MockJournalHandler is used directly
        use aura_effects::transport::TransportConfig;
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(InMemoryTransportHandler::new(TransportConfig::default())),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::with_seed(0)),
            time: Box::new(SimulatedTimeHandler::new()),
            console: Box::new(MockConsoleHandler::new()),
            journal: Box::new(journal),
            // TODO fix - Tree handler needs choreographic setup with runtime
            // For now, use a simple no-op handler
            tree: Box::new(DummyTreeHandler::new()),
        }
    }

    /// Create a composite handler for production with real implementations
    /// Uses default storage path, prefer `for_production_with_config` for custom configuration
    pub fn for_production(device_id: Uuid) -> Self {
        Self::for_production_with_storage_path(device_id, "/tmp/aura".into())
    }

    /// Create a composite handler for production with configurable storage path
    pub fn for_production_with_storage_path(
        device_id: Uuid,
        _storage_path: std::path::PathBuf, // Unused due to macro-generated handlers
    ) -> Self {
        let journal = MockJournalHandler::new();
        // Note: journal doesn't need to be cloned since MockJournalHandler is used directly
        use aura_effects::transport::TransportConfig;
        Self {
            device_id,
            is_simulation: false,
            network: Box::new(RealNetworkHandler::new(TransportConfig::default())),
            storage: Box::new(FilesystemStorageHandler::new()),
            crypto: Box::new(RealCryptoHandler::new()),
            time: Box::new(RealTimeHandler::new()),
            console: Box::new(RealConsoleHandler::new()),
            journal: Box::new(journal),
            tree: Box::new(DummyTreeHandler::new()),
        }
    }

    /// Create a composite handler for simulation/deterministic testing
    pub fn for_simulation(device_id: Uuid) -> Self {
        Self::for_simulation_with_seed(device_id, 0)
    }

    /// Create a composite handler for simulation with a specific seed for deterministic behavior
    pub fn for_simulation_with_seed(device_id: Uuid, _seed: u64) -> Self {
        let journal = MockJournalHandler::new();
        // Note: journal doesn't need to be cloned since MockJournalHandler is used directly
        use aura_effects::transport::TransportConfig;
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(InMemoryTransportHandler::new(TransportConfig::default())),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::with_seed(0)), // Macro-generated handlers don't take seed
            time: Box::new(SimulatedTimeHandler::new()), // Macro-generated handlers don't take seed
            console: Box::new(MockConsoleHandler::new()),
            journal: Box::new(journal),
            tree: Box::new(DummyTreeHandler::new()),
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

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        self.storage.store_batch(pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
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
impl aura_core::effects::RandomEffects for CompositeHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.crypto.random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        self.crypto.random_bytes_32().await
    }

    async fn random_u64(&self) -> u64 {
        self.crypto.random_u64().await
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        self.crypto.random_range(min, max).await
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        let bytes = self.random_bytes(16).await;
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes);
        uuid::Uuid::from_bytes(uuid_bytes)
    }
}

#[async_trait]
impl CryptoEffects for CompositeHandler {
    // Note: hash is NOT an algebraic effect - use aura_core::hash::hash() instead

    async fn ed25519_sign(&self, data: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.ed25519_sign(data, private_key).await
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto
            .ed25519_verify(data, signature, public_key)
            .await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.crypto.ed25519_generate_keypair().await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.ed25519_public_key(private_key).await
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.secure_zero(data)
    }

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.hkdf_derive(ikm, salt, info, output_len).await
    }

    // Note: hmac is NOT an algebraic effect - use aura_core::hash::hash() instead

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &aura_core::effects::crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.derive_key(master_key, context).await
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto
            .frost_generate_keys(threshold, max_signers)
            .await
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        self.crypto.frost_generate_nonces().await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<aura_core::effects::crypto::FrostSigningPackage, CryptoError> {
        self.crypto
            .frost_create_signing_package(message, nonces, participants, public_key_package)
            .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .frost_sign_share(signing_package, key_share, nonces)
            .await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .frost_aggregate_signatures(signing_package, signature_shares)
            .await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto
            .frost_verify(message, signature, group_public_key)
            .await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.chacha20_encrypt(plaintext, key, nonce).await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.chacha20_decrypt(ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.aes_gcm_encrypt(plaintext, key, nonce).await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.aes_gcm_decrypt(ciphertext, key, nonce).await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto
            .frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers)
            .await
    }

    fn is_simulated(&self) -> bool {
        self.crypto.is_simulated()
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        self.crypto.crypto_capabilities()
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

    async fn current_timestamp(&self) -> u64 {
        self.time.current_timestamp().await
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.time.current_timestamp_millis().await
    }

    async fn delay(&self, duration: std::time::Duration) {
        self.time.delay(duration).await
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), aura_core::AuraError> {
        self.time.sleep(duration_ms).await
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), aura_core::AuraError> {
        self.time.wait_until(condition).await
    }

    async fn now_instant(&self) -> std::time::Instant {
        self.time.now_instant().await
    }

    // timeout method removed to make TimeEffects dyn-compatible
    // Use tokio::time::timeout directly where needed
}

#[async_trait]
impl ConsoleEffects for CompositeHandler {
    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_debug(message).await
    }

    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_info(message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_warn(message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_error(message).await
    }
}

// Removed legacy ProtocolEffects and MinimalEffects traits
// Device ID and simulation state are available through the struct fields directly

/// Builder for creating custom composite handlers
pub struct CompositeHandlerBuilder {
    device_id: Uuid,
    is_simulation: bool,
    network: Option<Box<dyn NetworkEffects>>,
    storage: Option<Box<dyn StorageEffects>>,
    crypto: Option<Box<dyn CryptoEffects>>,
    time: Option<Box<dyn TimeEffects>>,
    console: Option<Box<dyn ConsoleEffects>>,
    journal: Option<Box<dyn JournalEffects>>,
    tree: Option<Box<dyn TreeEffects>>,
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
            journal: None,
            tree: None,
        }
    }

    /// Configure handler for simulation mode
    pub fn simulation(mut self) -> Self {
        self.is_simulation = true;
        self
    }

    /// Set the network effects handler
    pub fn with_network(mut self, network: Box<dyn NetworkEffects>) -> Self {
        self.network = Some(network);
        self
    }

    /// Set the storage effects handler
    pub fn with_storage(mut self, storage: Box<dyn StorageEffects>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Set the crypto effects handler
    pub fn with_crypto(mut self, crypto: Box<dyn CryptoEffects>) -> Self {
        self.crypto = Some(crypto);
        self
    }

    /// Set the time effects handler
    pub fn with_time(mut self, time: Box<dyn TimeEffects>) -> Self {
        self.time = Some(time);
        self
    }

    /// Set the console effects handler
    pub fn with_console(mut self, console: Box<dyn ConsoleEffects>) -> Self {
        self.console = Some(console);
        self
    }

    /// Build the final composite handler
    pub fn build(self) -> CompositeHandler {
        let device_id = self.device_id;
        let is_simulation = self.is_simulation;

        CompositeHandler {
            device_id,
            is_simulation,
            network: self.network.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(InMemoryTransportHandler::new(
                        aura_effects::transport::TransportConfig::default(),
                    ))
                } else {
                    // RealNetworkHandler now requires TransportConfig parameter
                    use aura_effects::transport::TransportConfig;
                    let config = TransportConfig::default();
                    Box::new(RealNetworkHandler::new(config))
                }
            }),
            storage: self.storage.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MemoryStorageHandler::new())
                } else {
                    // FilesystemStorageHandler now uses macro-generated new() method
                    Box::new(FilesystemStorageHandler::new())
                }
            }),
            crypto: self.crypto.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MockCryptoHandler::with_seed(0))
                } else {
                    Box::new(RealCryptoHandler::new())
                }
            }),
            time: self
                .time
                .unwrap_or_else(|| Box::new(SimulatedTimeHandler::new())),
            console: self.console.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(MockConsoleHandler::new())
                } else {
                    Box::new(RealConsoleHandler::new())
                }
            }),
            journal: self
                .journal
                .unwrap_or_else(|| Box::new(MockJournalHandler::new())),
            tree: self
                .tree
                .unwrap_or_else(|| Box::new(DummyTreeHandler::new())),
        }
    }
}

// Implement LedgerEffects with placeholder delegation
#[async_trait]
impl LedgerEffects for CompositeHandler {
    // Removed old methods that are no longer part of the trait

    async fn append_event(&self, _event: Vec<u8>) -> Result<(), LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn current_epoch(&self) -> Result<u64, LedgerError> {
        Ok(self.time.current_epoch().await)
    }

    async fn events_since(&self, _epoch: u64) -> Result<Vec<Vec<u8>>, LedgerError> {
        Err(LedgerError::NotAvailable)
    }

    async fn is_device_authorized(
        &self,
        _device_id: aura_core::DeviceId,
        _operation: &str,
    ) -> Result<bool, LedgerError> {
        Ok(true) // Placeholder - always authorized
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
        Ok(false) // Stub implementation
    }

    async fn find_connected_components(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, LedgerError> {
        Ok(vec![]) // Stub implementation
    }

    async fn topological_sort(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, LedgerError> {
        Ok(vec![]) // Stub implementation
    }

    async fn shortest_path(
        &self,
        _edges: &[(Vec<u8>, Vec<u8>)],
        _start: Vec<u8>,
        _end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, LedgerError> {
        Ok(None) // Stub implementation
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, LedgerError> {
        let random_bytes = self.crypto.random_bytes(length).await;
        Ok(random_bytes)
    }

    async fn hash_data(&self, _data: &[u8]) -> Result<[u8; 32], LedgerError> {
        let hash_result = hash(_data);
        Ok(hash_result)
    }

    async fn current_timestamp(&self) -> Result<u64, LedgerError> {
        let timestamp = self.time.current_timestamp().await;
        Ok(timestamp)
    }

    async fn ledger_device_id(&self) -> Result<aura_core::DeviceId, LedgerError> {
        Ok(self.device_id.into())
    }

    async fn new_uuid(&self) -> Result<Uuid, LedgerError> {
        Ok(Uuid::new_v4())
    }
}

// Implement ChoreographicEffects with placeholder delegation
#[async_trait]
impl ChoreographicEffects for CompositeHandler {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        self.network
            .send_to_peer(role.device_id, message)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        self.network
            .receive_from(role.device_id)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        self.network
            .broadcast(message)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })
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

    async fn start_session(
        &self,
        _session_id: Uuid,
        _roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        Ok(())
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        Ok(())
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        // Convert to console event
        let _console_event = ConsoleEvent::Custom {
            event_type: "choreography".to_string(),
            data: serde_json::to_value(event).unwrap_or_default(),
        };
        // Note: emit_event removed from ConsoleEffects trait
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

// REMOVED: ProtocolJournalEffects implementation - violates crate boundaries
// All the deprecated methods from local JournalEffects trait have been removed
// Core JournalEffects is implemented below

// Implement aura-core's JournalEffects for flow budget operations
#[async_trait]
impl JournalEffects for CompositeHandler {
    async fn merge_facts(
        &self,
        target: &aura_core::Journal,
        delta: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        // Delegate to journal handler for CRDT operations
        self.journal
            .merge_facts(target, delta)
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("merge_facts failed: {}", e)))
    }

    async fn refine_caps(
        &self,
        target: &aura_core::Journal,
        refinement: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        self.journal
            .refine_caps(target, refinement)
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("refine_caps failed: {}", e)))
    }

    async fn get_journal(&self) -> Result<aura_core::Journal, aura_core::AuraError> {
        self.journal
            .get_journal()
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("get_journal failed: {}", e)))
    }

    async fn persist_journal(
        &self,
        journal: &aura_core::Journal,
    ) -> Result<(), aura_core::AuraError> {
        self.journal
            .persist_journal(journal)
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("persist_journal failed: {}", e)))
    }

    async fn get_flow_budget(
        &self,
        context: &ContextId,
        peer: &aura_core::identifiers::AuthorityId,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        self.journal
            .get_flow_budget(context, peer)
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("get_flow_budget failed: {}", e)))
    }

    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &aura_core::identifiers::AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        self.journal
            .update_flow_budget(context, peer, budget)
            .await
            .map_err(|e| {
                aura_core::AuraError::internal(format!("update_flow_budget failed: {}", e))
            })
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &aura_core::identifiers::AuthorityId,
        cost: u32,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        self.journal
            .charge_flow_budget(context, peer, cost)
            .await
            .map_err(|e| {
                aura_core::AuraError::internal(format!("charge_flow_budget failed: {}", e))
            })
    }
}

// Implement TreeEffects by delegating to the tree handler
#[async_trait]
impl TreeEffects for CompositeHandler {
    async fn get_current_state(
        &self,
    ) -> Result<aura_journal::ratchet_tree::TreeState, aura_core::AuraError> {
        self.tree.get_current_state().await
    }

    async fn get_current_commitment(&self) -> Result<aura_core::Hash32, aura_core::AuraError> {
        self.tree.get_current_commitment().await
    }

    async fn get_current_epoch(&self) -> Result<u64, aura_core::AuraError> {
        self.tree.get_current_epoch().await
    }

    async fn apply_attested_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, aura_core::AuraError> {
        self.tree.apply_attested_op(op).await
    }

    async fn verify_aggregate_sig(
        &self,
        op: &aura_core::AttestedOp,
        state: &aura_journal::ratchet_tree::TreeState,
    ) -> Result<bool, aura_core::AuraError> {
        self.tree.verify_aggregate_sig(op, state).await
    }

    async fn add_leaf(
        &self,
        leaf: aura_core::LeafNode,
        under: aura_core::NodeIndex,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        self.tree.add_leaf(leaf, under).await
    }

    async fn remove_leaf(
        &self,
        leaf_id: aura_core::LeafId,
        reason: u8,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        self.tree.remove_leaf(leaf_id, reason).await
    }

    async fn change_policy(
        &self,
        node: aura_core::NodeIndex,
        new_policy: aura_core::Policy,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        self.tree.change_policy(node, new_policy).await
    }

    async fn rotate_epoch(
        &self,
        affected: Vec<aura_core::NodeIndex>,
    ) -> Result<aura_core::TreeOpKind, aura_core::AuraError> {
        self.tree.rotate_epoch(affected).await
    }

    async fn propose_snapshot(
        &self,
        cut: crate::effects::tree::Cut,
    ) -> Result<crate::effects::tree::ProposalId, aura_core::AuraError> {
        self.tree.propose_snapshot(cut).await
    }

    async fn approve_snapshot(
        &self,
        proposal_id: crate::effects::tree::ProposalId,
    ) -> Result<crate::effects::tree::Partial, aura_core::AuraError> {
        self.tree.approve_snapshot(proposal_id).await
    }

    async fn finalize_snapshot(
        &self,
        proposal_id: crate::effects::tree::ProposalId,
    ) -> Result<crate::effects::tree::Snapshot, aura_core::AuraError> {
        self.tree.finalize_snapshot(proposal_id).await
    }

    async fn apply_snapshot(
        &self,
        snapshot: &crate::effects::tree::Snapshot,
    ) -> Result<(), aura_core::AuraError> {
        self.tree.apply_snapshot(snapshot).await
    }
}

// Implement SystemEffects - delegating to console for logging
#[async_trait]
impl SystemEffects for CompositeHandler {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        let log_message = format!("[{}] {}: {}", component, level, message);
        match level {
            "ERROR" => {
                self.console.log_error(&log_message).await?;
            }
            "WARN" => {
                self.console.log_warn(&log_message).await?;
            }
            "INFO" => {
                self.console.log_info(&log_message).await?;
            }
            "DEBUG" => {
                self.console.log_debug(&log_message).await?;
            }
            "TRACE" => {
                let _ = self.console.log_debug(&log_message).await;
            }
            _ => {
                self.console.log_info(&log_message).await?;
            }
        }
        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: std::collections::HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let _fields: Vec<(&str, &str)> = context
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        // Format context fields into the message since ConsoleEffects doesn't support fields
        let context_str = if !context.is_empty() {
            format!(
                " [{}]",
                context
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            String::new()
        };

        let log_message = format!("[{}] {}: {}{}", component, level, message, context_str);
        match level {
            "ERROR" => {
                self.console.log_error(&log_message).await?;
            }
            "WARN" => {
                self.console.log_warn(&log_message).await?;
            }
            "INFO" => {
                self.console.log_info(&log_message).await?;
            }
            "DEBUG" => {
                self.console.log_debug(&log_message).await?;
            }
            "TRACE" => {
                let _ = self.console.log_debug(&log_message).await;
            }
            _ => {
                self.console.log_info(&log_message).await?;
            }
        }
        Ok(())
    }

    async fn get_system_info(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, SystemError> {
        let mut info = std::collections::HashMap::new();
        info.insert("device_id".to_string(), self.device_id.to_string());
        info.insert("is_simulation".to_string(), self.is_simulation.to_string());
        info.insert(
            "crypto_capabilities".to_string(),
            self.crypto.crypto_capabilities().join(","),
        );
        info.insert(
            "execution_mode".to_string(),
            format!("{:?}", self.execution_mode()),
        );
        Ok(info)
    }

    async fn set_config(&self, key: &str, _value: &str) -> Result<(), SystemError> {
        // For now, composite handler doesn't support dynamic configuration
        Err(SystemError::PermissionDenied {
            operation: format!("set_config: {}", key),
        })
    }

    async fn get_config(&self, key: &str) -> Result<String, SystemError> {
        match key {
            "device_id" => Ok(self.device_id.to_string()),
            "is_simulation" => Ok(self.is_simulation.to_string()),
            "execution_mode" => Ok(format!("{:?}", self.execution_mode())),
            _ => Err(SystemError::ResourceNotFound {
                resource: format!("config key: {}", key),
            }),
        }
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        // Basic health check - always healthy for composite handler
        Ok(true)
    }

    async fn get_metrics(&self) -> Result<std::collections::HashMap<String, f64>, SystemError> {
        let mut metrics = std::collections::HashMap::new();

        // Get storage stats if available
        if let Ok(stats) = self.storage.stats().await {
            metrics.insert("storage_items".to_string(), stats.key_count as f64);
            metrics.insert("storage_size_bytes".to_string(), stats.total_size as f64);
        }

        // Get current epoch from time
        let current_epoch = self.time.current_epoch().await;
        metrics.insert("current_epoch".to_string(), current_epoch as f64);

        // Get number of connected peers
        let connected_peers = self.network.connected_peers().await.len();
        metrics.insert("connected_peers".to_string(), connected_peers as f64);

        Ok(metrics)
    }

    async fn restart_component(&self, component: &str) -> Result<(), SystemError> {
        // Composite handler doesn't support restarting components
        Err(SystemError::PermissionDenied {
            operation: format!("restart_component: {}", component),
        })
    }

    async fn shutdown(&self) -> Result<(), SystemError> {
        // Composite handler doesn't support shutdown (would need to be handled at higher level)
        Err(SystemError::PermissionDenied {
            operation: "shutdown".to_string(),
        })
    }
}

// Blanket implementation for AuraEffects umbrella trait
impl crate::effects::AuraEffects for CompositeHandler {
    fn execution_mode(&self) -> aura_core::effects::ExecutionMode {
        // For now, return Testing mode since we're using test handlers
        // TODO: Track actual execution mode in CompositeHandler
        aura_core::effects::ExecutionMode::Testing
    }
}

// Implementation of unified AuraHandler trait
#[async_trait]
impl AuraHandler for CompositeHandler {
    /// Execute an effect with serialized parameters and return serialized result
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match effect_type {
            EffectType::Network => self.execute_network_effect(operation, parameters).await,
            EffectType::Storage => self.execute_storage_effect(operation, parameters).await,
            EffectType::Crypto => self.execute_crypto_effect(operation, parameters).await,
            EffectType::Time => self.execute_time_effect(operation, parameters).await,
            EffectType::Console => self.execute_console_effect(operation, parameters).await,
            EffectType::Ledger => self.execute_ledger_effect(operation, parameters).await,
            EffectType::Choreographic => {
                self.execute_choreographic_effect(operation, parameters)
                    .await
            }
            EffectType::System => self.execute_system_effect(operation, parameters).await,
            EffectType::Journal => self.execute_journal_effect(operation, parameters).await,
            EffectType::Random => self.execute_random_effect(operation, parameters).await,
            _ => Err(AuraHandlerError::UnsupportedEffect { effect_type }),
        }
    }

    /// Execute a session type
    async fn execute_session(
        &self,
        _session: LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Placeholder implementation - session type execution will be implemented
        // when the session type algebra is fully defined
        Err(AuraHandlerError::UnsupportedEffect {
            effect_type: EffectType::SessionManagement,
        })
    }

    /// Check if this handler supports a specific effect type
    fn supports_effect(&self, effect_type: EffectType) -> bool {
        matches!(
            effect_type,
            EffectType::Network
                | EffectType::Storage
                | EffectType::Crypto
                | EffectType::Time
                | EffectType::Console
                | EffectType::Ledger
                | EffectType::Choreographic
                | EffectType::System
                | EffectType::Journal
                | EffectType::Random
        )
    }

    /// Get the execution mode of this handler
    fn execution_mode(&self) -> ExecutionMode {
        if self.is_simulation {
            ExecutionMode::Simulation { seed: 0 }
        } else {
            ExecutionMode::Production
        }
    }
}

impl CompositeHandler {
    /// Execute network effects through serialized interface
    async fn execute_network_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "send_to_peer" => {
                let params: serde_json::Value =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;

                let peer_id = params
                    .get("peer_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AuraHandlerError::ContextError {
                        message: "Missing peer_id field".to_string(),
                    })?;

                let peer_uuid =
                    Uuid::parse_str(peer_id).map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Invalid peer_id format: {}", e),
                    })?;

                let data = params
                    .get("data")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| AuraHandlerError::ContextError {
                        message: "Missing data field".to_string(),
                    })?;

                let message: Vec<u8> = data
                    .iter()
                    .filter_map(|v| v.as_u64())
                    .map(|n| n as u8)
                    .collect();

                self.network
                    .send_to_peer(peer_uuid, message)
                    .await
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Network effect {} failed: {}", operation, e),
                    })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            "broadcast" => {
                let message: Vec<u8> = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                self.network.broadcast(message).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Network effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            "receive" => {
                let result =
                    self.network
                        .receive()
                        .await
                        .map_err(|e| AuraHandlerError::ContextError {
                            message: format!("Network effect {} failed: {}", operation, e),
                        })?;
                Ok(serde_json::to_vec(&serde_json::json!({
                    "peer_id": result.0.to_string(),
                    "data": result.1
                }))
                .unwrap_or_default())
            }
            "receive_from" => {
                let peer_id: Uuid = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.network.receive_from(peer_id).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Network effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&serde_json::json!({
                    "data": result
                }))
                .unwrap_or_default())
            }
            "connected_peers" => {
                let result = self.network.connected_peers().await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "is_peer_connected" => {
                let peer_id: Uuid = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.network.is_peer_connected(peer_id).await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Network,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute storage effects through serialized interface
    async fn execute_storage_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "store" => {
                let params: serde_json::Value =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;

                let key = params
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AuraHandlerError::ContextError {
                        message: "Missing or invalid 'key' parameter".to_string(),
                    })?
                    .to_string();

                let value = params
                    .get("value")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| AuraHandlerError::ContextError {
                        message: "Missing or invalid 'value' parameter".to_string(),
                    })?
                    .iter()
                    .map(|v| v.as_u64().unwrap_or(0) as u8)
                    .collect::<Vec<u8>>();
                self.storage.store(&key, value).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            "retrieve" => {
                let key: String = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.storage.retrieve(&key).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "remove" => {
                let key: String = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.storage.remove(&key).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "exists" => {
                let key: String = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.storage.exists(&key).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "list_keys" => {
                let prefix: Option<String> = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self
                    .storage
                    .list_keys(prefix.as_deref())
                    .await
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "store_batch" => {
                let pairs: std::collections::HashMap<String, Vec<u8>> =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
                self.storage.store_batch(pairs).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            "retrieve_batch" => {
                let keys: Vec<String> = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.storage.retrieve_batch(&keys).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "clear_all" => {
                self.storage
                    .clear_all()
                    .await
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Storage effect {} failed: {}", operation, e),
                    })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Storage,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute crypto effects through serialized interface
    async fn execute_crypto_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "random_bytes" => {
                let len: usize = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.crypto.random_bytes(len).await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "hash_data" => {
                let data: Vec<u8> = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = hash(&data);
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Crypto,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute time effects through serialized interface
    async fn execute_time_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "current_epoch" => {
                let result = self.time.current_epoch().await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "sleep_ms" => {
                let ms: u64 = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                self.time.sleep_ms(ms).await;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Time,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute console effects through serialized interface
    async fn execute_console_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "log_info" => {
                #[derive(serde::Deserialize)]
                #[serde(untagged)]
                enum LogInfoParams {
                    MessageOnly(String),
                    MessageWithFields { message: String },
                }

                let params: LogInfoParams = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;

                let message = match params {
                    LogInfoParams::MessageOnly(msg) => msg,
                    LogInfoParams::MessageWithFields { message } => message,
                };

                self.console.log_info(&message).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Console effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Console,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute ledger effects through serialized interface
    async fn execute_ledger_effect(
        &self,
        operation: &str,
        _parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "current_epoch" => {
                let result = self.time.current_epoch().await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Ledger,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute choreographic effects through serialized interface
    async fn execute_choreographic_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "send_to_role_bytes" => {
                let (role, message): (ChoreographicRole, Vec<u8>) =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
                self.network
                    .send_to_peer(role.device_id, message)
                    .await
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Choreographic effect {} failed: {}", operation, e),
                    })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Choreographic,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute system effects through serialized interface
    async fn execute_system_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "log" => {
                let (level, component, message): (String, String, String) =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
                SystemEffects::log(self, &level, &component, &message)
                    .await
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("System effect {} failed: {}", operation, e),
                    })?;
                Ok(serde_json::to_vec(&()).unwrap_or_default())
            }
            "get_system_info" => {
                let result = SystemEffects::get_system_info(self).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("System effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "health_check" => {
                let result = SystemEffects::health_check(self).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("System effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "get_metrics" => {
                let result = SystemEffects::get_metrics(self).await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("System effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::System,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute journal effects through serialized interface
    async fn execute_journal_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "get_journal_state" | "get_journal" => {
                let result = self.journal.get_journal().await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Journal effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            // Legacy operations removed - use TreeEffects for tree operations
            // "get_current_tree" and "get_latest_epoch" are tree-specific, not journal operations
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Journal,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute random effects through serialized interface
    async fn execute_random_effect(
        &self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "random_bytes" => {
                let len: usize = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.random_bytes(len).await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "random_bytes_32" => {
                let result = self.random_bytes_32().await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "random_u64" => {
                let result = self.random_u64().await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "random_range" => {
                let (min, max): (u64, u64) = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.random_range(min, max).await;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type: EffectType::Random,
                operation: operation.to_string(),
            }),
        }
    }
}
