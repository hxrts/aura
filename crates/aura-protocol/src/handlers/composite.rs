//! Composite effect handler
//!
//! Combines multiple effect handlers into a single unified handler that implements all effect traits.

use super::{
    console::{SilentConsoleHandler, StdoutConsoleHandler},
    context::AuraContext,
    crypto::{MockCryptoHandler, RealCryptoHandler},
    // time::{RealTimeHandler, SimulatedTimeHandler}, // Temporarily commented out
    erased::AuraHandler,
    network::{MemoryNetworkHandler, RealNetworkHandler, SimulatedNetworkHandler},
    storage::{FilesystemStorageHandler, MemoryStorageHandler},
    {AuraHandlerError, EffectType, ExecutionMode},
};
use crate::effects::*;
use async_trait::async_trait;
use aura_core::{identifiers::DeviceId, relationships::ContextId, FlowBudget, LocalSessionType};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde_json;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Dummy time handler for stub implementation
/// Dummy time handler for testing and simulation
pub struct DummyTimeHandler;

impl DummyTimeHandler {
    /// Create a new dummy time handler
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TimeEffects for DummyTimeHandler {
    async fn current_epoch(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    async fn current_timestamp(&self) -> u64 {
        self.current_epoch().await / 1000
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.current_epoch().await
    }

    async fn sleep_ms(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    async fn sleep_until(&self, _epoch: u64) {
        // Stub implementation
    }

    async fn delay(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), aura_core::AuraError> {
        self.sleep_ms(duration_ms).await;
        Ok(())
    }

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        Ok(()) // Stub - immediately returns
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), aura_core::AuraError> {
        self.yield_until(condition)
            .await
            .map_err(|e| aura_core::AuraError::internal(&format!("Wait until failed: {}", e)))
    }

    async fn set_timeout(&self, _timeout_ms: u64) -> TimeoutHandle {
        Uuid::new_v4()
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        Ok(()) // Stub
    }

    // timeout method removed to make TimeEffects dyn-compatible
    // Use tokio::time::timeout directly where needed

    fn is_simulated(&self) -> bool {
        false // Not a simulation
    }

    fn register_context(&self, _context_id: Uuid) {
        // Stub
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // Stub
    }

    async fn notify_events_available(&self) {
        // Stub
    }

    fn resolution_ms(&self) -> u64 {
        1 // 1ms resolution
    }
}

/// Composite handler that implements all effect traits
pub struct CompositeHandler {
    device_id: Uuid,
    is_simulation: bool,
    network: Box<dyn NetworkEffects>,
    storage: Box<dyn StorageEffects>,
    crypto: Box<dyn CryptoEffects>,
    time: DummyTimeHandler,
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
            ExecutionMode::Simulation { seed: _seed } => {
                // TODO fix - For now, simulation mode uses the same as testing
                // TODO: Create simulation-specific handlers with the seed
                Self::for_testing(device_id)
            }
        }
    }

    /// Create a composite handler for testing with all mock/memory implementations
    pub fn for_testing(device_id: Uuid) -> Self {
        let journal = super::journal::MemoryJournalHandler::new();
        let tree_journal: Arc<dyn JournalEffects> = std::sync::Arc::new(journal.clone());
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(MemoryNetworkHandler::new(device_id)),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::new(42)),
            time: DummyTimeHandler::new(),
            console: Box::new(SilentConsoleHandler::new()),
            journal: Box::new(journal),
            tree: Box::new(super::tree::MemoryTreeHandler::new(tree_journal)),
        }
    }

    /// Create a composite handler for production with real implementations
    pub fn for_production(device_id: Uuid) -> Self {
        let journal = super::journal::MemoryJournalHandler::new();
        let tree_journal: Arc<dyn JournalEffects> = std::sync::Arc::new(journal.clone());
        Self {
            device_id,
            is_simulation: false,
            network: Box::new(RealNetworkHandler::new(
                device_id,
                "tcp://0.0.0.0:0".to_string(),
            )),
            storage: Box::new(FilesystemStorageHandler::new("/tmp/aura".into()).unwrap()),
            crypto: Box::new(RealCryptoHandler::new()),
            time: DummyTimeHandler::new(),
            console: Box::new(StdoutConsoleHandler::new()),
            journal: Box::new(journal),
            tree: Box::new(super::tree::MemoryTreeHandler::new(tree_journal)),
        }
    }

    /// Create a composite handler for simulation/deterministic testing
    pub fn for_simulation(device_id: Uuid) -> Self {
        let journal = super::journal::MemoryJournalHandler::new();
        let tree_journal: Arc<dyn JournalEffects> = std::sync::Arc::new(journal.clone());
        Self {
            device_id,
            is_simulation: true,
            network: Box::new(SimulatedNetworkHandler::new(device_id)),
            storage: Box::new(MemoryStorageHandler::new()),
            crypto: Box::new(MockCryptoHandler::new(device_id.as_u128() as u64)),
            time: DummyTimeHandler::new(),
            console: Box::new(SilentConsoleHandler::new()),
            journal: Box::new(journal),
            tree: Box::new(super::tree::MemoryTreeHandler::new(tree_journal)),
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
}

#[async_trait]
impl CryptoEffects for CompositeHandler {
    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        self.crypto.hash(data).await
    }

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

    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        self.crypto.hmac(key, data).await
    }

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
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
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
    ) -> Result<aura_core::effects::crypto::FrostSigningPackage, CryptoError> {
        self.crypto
            .frost_create_signing_package(message, nonces, participants)
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
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
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

    // timeout method removed to make TimeEffects dyn-compatible
    // Use tokio::time::timeout directly where needed
}

impl ConsoleEffects for CompositeHandler {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_trace(message, fields);
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_debug(message, fields);
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_info(message, fields);
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_warn(message, fields);
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        self.console.log_error(message, fields);
    }

    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.console.emit_event(event)
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
    time: Option<DummyTimeHandler>,
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
    pub fn with_time(mut self, time: DummyTimeHandler) -> Self {
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
                    Box::new(MemoryNetworkHandler::new(device_id))
                } else {
                    Box::new(RealNetworkHandler::new(
                        device_id,
                        "tcp://0.0.0.0:0".to_string(),
                    ))
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
            time: self.time.unwrap_or_else(|| DummyTimeHandler::new()),
            console: self.console.unwrap_or_else(|| {
                if is_simulation {
                    Box::new(SilentConsoleHandler::new())
                } else {
                    Box::new(StdoutConsoleHandler::new())
                }
            }),
            journal: self
                .journal
                .unwrap_or_else(|| Box::new(super::journal::MemoryJournalHandler::new())),
            tree: self.tree.unwrap_or_else(|| {
                let journal_arc = std::sync::Arc::new(super::journal::MemoryJournalHandler::new());
                Box::new(super::tree::MemoryTreeHandler::new(journal_arc))
            }),
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

    async fn hash_blake3(&self, _data: &[u8]) -> Result<[u8; 32], LedgerError> {
        let hash = self.crypto.hash(_data).await;
        Ok(hash)
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
        let console_event = ConsoleEvent::Custom {
            event_type: "choreography".to_string(),
            data: serde_json::to_value(event).unwrap_or_default(),
        };
        self.console.emit_event(console_event).await;
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

// Implement JournalEffects by delegating to the journal handler
#[async_trait]
impl JournalEffects for CompositeHandler {
    async fn append_attested_tree_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, aura_core::AuraError> {
        self.journal.append_attested_tree_op(op).await
    }

    async fn get_tree_state(
        &self,
    ) -> Result<aura_journal::ratchet_tree::TreeState, aura_core::AuraError> {
        self.journal.get_tree_state().await
    }

    async fn get_op_log(&self) -> Result<aura_journal::semilattice::OpLog, aura_core::AuraError> {
        self.journal.get_op_log().await
    }

    async fn merge_op_log(
        &self,
        remote: aura_journal::semilattice::OpLog,
    ) -> Result<(), aura_core::AuraError> {
        self.journal.merge_op_log(remote).await
    }

    async fn get_attested_op(
        &self,
        cid: &aura_core::Hash32,
    ) -> Result<Option<aura_core::AttestedOp>, aura_core::AuraError> {
        self.journal.get_attested_op(cid).await
    }

    async fn list_attested_ops(&self) -> Result<Vec<aura_core::AttestedOp>, aura_core::AuraError> {
        self.journal.list_attested_ops().await
    }

    // Delegate other JournalEffects methods to the journal handler
    async fn get_journal_state(
        &self,
    ) -> Result<crate::effects::journal::JournalMap, aura_core::AuraError> {
        self.journal.get_journal_state().await
    }

    async fn get_current_tree(
        &self,
    ) -> Result<crate::effects::journal::RatchetTree, aura_core::AuraError> {
        self.journal.get_current_tree().await
    }

    async fn get_tree_at_epoch(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<crate::effects::journal::RatchetTree, aura_core::AuraError> {
        self.journal.get_tree_at_epoch(epoch).await
    }

    async fn get_current_commitment(
        &self,
    ) -> Result<crate::effects::journal::Commitment, aura_core::AuraError> {
        self.journal.get_current_commitment().await
    }

    async fn get_latest_epoch(
        &self,
    ) -> Result<Option<crate::effects::journal::Epoch>, aura_core::AuraError> {
        self.journal.get_latest_epoch().await
    }

    async fn append_tree_op(
        &self,
        op: crate::effects::journal::TreeOpRecord,
    ) -> Result<(), aura_core::AuraError> {
        self.journal.append_tree_op(op).await
    }

    async fn get_tree_op(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<Option<crate::effects::journal::TreeOpRecord>, aura_core::AuraError> {
        self.journal.get_tree_op(epoch).await
    }

    async fn list_tree_ops(
        &self,
    ) -> Result<Vec<crate::effects::journal::TreeOpRecord>, aura_core::AuraError> {
        self.journal.list_tree_ops().await
    }

    async fn submit_intent(
        &self,
        intent: crate::effects::journal::Intent,
    ) -> Result<crate::effects::journal::IntentId, aura_core::AuraError> {
        self.journal.submit_intent(intent).await
    }

    async fn get_intent(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<Option<crate::effects::journal::Intent>, aura_core::AuraError> {
        self.journal.get_intent(intent_id).await
    }

    async fn get_intent_status(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<crate::effects::journal::IntentStatus, aura_core::AuraError> {
        self.journal.get_intent_status(intent_id).await
    }

    async fn list_pending_intents(
        &self,
    ) -> Result<Vec<crate::effects::journal::Intent>, aura_core::AuraError> {
        self.journal.list_pending_intents().await
    }

    async fn tombstone_intent(
        &self,
        intent_id: crate::effects::journal::IntentId,
    ) -> Result<(), aura_core::AuraError> {
        self.journal.tombstone_intent(intent_id).await
    }

    async fn prune_stale_intents(
        &self,
        current_commitment: crate::effects::journal::Commitment,
    ) -> Result<usize, aura_core::AuraError> {
        self.journal.prune_stale_intents(current_commitment).await
    }

    async fn validate_capability(
        &self,
        capability: &crate::effects::journal::CapabilityRef,
    ) -> Result<bool, aura_core::AuraError> {
        self.journal.validate_capability(capability).await
    }

    async fn is_capability_revoked(
        &self,
        capability_id: &crate::effects::journal::CapabilityId,
    ) -> Result<bool, aura_core::AuraError> {
        self.journal.is_capability_revoked(capability_id).await
    }

    async fn list_capabilities_in_op(
        &self,
        epoch: crate::effects::journal::Epoch,
    ) -> Result<Vec<crate::effects::journal::CapabilityRef>, aura_core::AuraError> {
        self.journal.list_capabilities_in_op(epoch).await
    }

    async fn merge_journal_state(
        &self,
        other: crate::effects::journal::JournalMap,
    ) -> Result<(), aura_core::AuraError> {
        self.journal.merge_journal_state(other).await
    }

    async fn get_journal_stats(
        &self,
    ) -> Result<crate::effects::journal::JournalStats, aura_core::AuraError> {
        self.journal.get_journal_stats().await
    }

    async fn is_device_member(
        &self,
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<bool, aura_core::AuraError> {
        self.journal.is_device_member(device_id).await
    }

    async fn get_device_leaf_index(
        &self,
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<Option<crate::effects::journal::LeafIndex>, aura_core::AuraError> {
        self.journal.get_device_leaf_index(device_id).await
    }

    async fn list_devices(
        &self,
    ) -> Result<Vec<aura_core::identifiers::DeviceId>, aura_core::AuraError> {
        self.journal.list_devices().await
    }

    async fn list_guardians(
        &self,
    ) -> Result<Vec<aura_core::identifiers::GuardianId>, aura_core::AuraError> {
        self.journal.list_guardians().await
    }

    async fn get_flow_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        self.journal.get_flow_budget(context, peer).await
    }

    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        self.journal.update_flow_budget(context, peer, budget).await
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        self.journal.charge_flow_budget(context, peer, cost).await
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
            "ERROR" => self.console.log_error(&log_message, &[]),
            "WARN" => self.console.log_warn(&log_message, &[]),
            "INFO" => self.console.log_info(&log_message, &[]),
            "DEBUG" => self.console.log_debug(&log_message, &[]),
            "TRACE" => self.console.log_trace(&log_message, &[]),
            _ => self.console.log_info(&log_message, &[]),
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
        let fields: Vec<(&str, &str)> = context
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        
        let log_message = format!("[{}] {}: {}", component, level, message);
        match level {
            "ERROR" => self.console.log_error(&log_message, &fields),
            "WARN" => self.console.log_warn(&log_message, &fields),
            "INFO" => self.console.log_info(&log_message, &fields),
            "DEBUG" => self.console.log_debug(&log_message, &fields),
            "TRACE" => self.console.log_trace(&log_message, &fields),
            _ => self.console.log_info(&log_message, &fields),
        }
        Ok(())
    }

    async fn get_system_info(&self) -> Result<std::collections::HashMap<String, String>, SystemError> {
        let mut info = std::collections::HashMap::new();
        info.insert("device_id".to_string(), self.device_id.to_string());
        info.insert("is_simulation".to_string(), self.is_simulation.to_string());
        info.insert("crypto_capabilities".to_string(), 
                   self.crypto.crypto_capabilities().join(","));
        info.insert("execution_mode".to_string(), 
                   format!("{:?}", self.execution_mode()));
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
            metrics.insert("storage_items".to_string(), stats.total_items as f64);
            metrics.insert("storage_size_bytes".to_string(), stats.total_size_bytes as f64);
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
impl AuraEffects for CompositeHandler {}

// Implementation of unified AuraHandler trait
#[async_trait]
impl AuraHandler for CompositeHandler {
    /// Execute an effect with serialized parameters and return serialized result
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &mut AuraContext,
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
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
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
                let (peer_id, message): (Uuid, Vec<u8>) = serde_json::from_slice(parameters)
                    .map_err(|e| AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    })?;
                self.network
                    .send_to_peer(peer_id, message)
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
                let (key, value): (String, Vec<u8>) =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
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
            "blake3_hash" => {
                let data: Vec<u8> = serde_json::from_slice(parameters).map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Failed to deserialize {} parameters: {}", operation, e),
                    }
                })?;
                let result = self.crypto.hash(&data).await;
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
                let (message, fields): (String, Vec<(String, String)>) =
                    serde_json::from_slice(parameters).map_err(|e| {
                        AuraHandlerError::ContextError {
                            message: format!(
                                "Failed to deserialize {} parameters: {}",
                                operation, e
                            ),
                        }
                    })?;
                let field_refs: Vec<(&str, &str)> = fields
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                self.console.log_info(&message, &field_refs);
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
            "get_journal_state" => {
                let result = self.journal.get_journal_state().await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Journal effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "get_current_tree" => {
                let result = self.journal.get_current_tree().await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Journal effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
            "get_latest_epoch" => {
                let result = self.journal.get_latest_epoch().await.map_err(|e| {
                    AuraHandlerError::ContextError {
                        message: format!("Journal effect {} failed: {}", operation, e),
                    }
                })?;
                Ok(serde_json::to_vec(&result).unwrap_or_default())
            }
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
