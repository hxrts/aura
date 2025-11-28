//! Foundational Test Infrastructure
//!
//! This module provides testing utilities that comply with the 8-layer architecture
//! by only depending on Layers 1-3 (Foundation, Specification, Implementation).
//!
//! # Architecture Compliance
//!
//! This module only imports from:
//! - Layer 1: aura-core (effect traits, types)
//! - Layer 2: domain crates (aura-journal, aura-transport)
//! - Layer 3: aura-effects (effect implementations)
//!
//! It does NOT import from:
//! - Layer 4+: aura-protocol, aura-agent, etc.
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_testkit::foundation::{TestEffectComposer, SimpleTestContext};
//! use aura_core::effects::{ExecutionMode, CryptoEffects, PhysicalTimeEffects};
//!
//! // Create a test context with specific effect handlers
//! let context = SimpleTestContext::new(ExecutionMode::Testing)
//!     .with_mock_crypto()
//!     .with_mock_time()
//!     .build();
//!
//! // Use the context in tests with trait bounds
//! async fn test_crypto_operation<E: CryptoEffects>(effects: &E) {
//!     let key = effects.generate_signing_key().await.unwrap();
//!     // ... test logic
//! }
//! ```

use aura_core::{
    effects::{
        crypto::{CryptoError, FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext},
        ConsoleEffects, CryptoEffects, ExecutionMode, JournalEffects, NetworkEffects,
        PhysicalTimeEffects, RandomEffects, StorageEffects,
    },
    AuraResult, DeviceId,
};
// Foundation-based test infrastructure - no Arc needed for lightweight handlers

/// Simple test context that provides basic effect handler composition
///
/// This replaces the complex orchestration-layer effect runtime with a simple
/// composition suitable for testing foundational functionality.
#[derive(Clone)]
pub struct SimpleTestContext {
    execution_mode: ExecutionMode,
    device_id: DeviceId,
}

impl SimpleTestContext {
    /// Create a new test context with the specified execution mode
    pub fn new(execution_mode: ExecutionMode) -> Self {
        Self {
            execution_mode,
            device_id: DeviceId::new(),
        }
    }

    /// Create a test context with a specific device ID
    pub fn with_device_id(execution_mode: ExecutionMode, device_id: DeviceId) -> Self {
        Self {
            execution_mode,
            device_id,
        }
    }

    /// Get the execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Trait for composable test effect handlers
///
/// This trait allows tests to work with different effect handler combinations
/// without depending on the orchestration layer.
pub trait TestEffectHandler:
    CryptoEffects
    + NetworkEffects
    + StorageEffects
    + PhysicalTimeEffects
    + RandomEffects
    + ConsoleEffects
    + JournalEffects
    + Send
    + Sync
{
    /// Get the execution mode for this handler
    fn execution_mode(&self) -> ExecutionMode;
}

/// Helper for creating test effect handlers from aura-effects implementations
pub struct TestEffectComposer {
    _execution_mode: ExecutionMode,
    _device_id: DeviceId,
}

impl TestEffectComposer {
    /// Create a new composer for the given execution mode
    pub fn new(execution_mode: ExecutionMode, device_id: DeviceId) -> Self {
        Self {
            _execution_mode: execution_mode,
            _device_id: device_id,
        }
    }

    /// Build a test effect handler using mock implementations
    pub fn build_mock_handler(&self) -> AuraResult<Box<dyn TestEffectHandler>> {
        // Create a composite handler that implements all required effect traits
        let handler = CompositeTestHandler::new_mock(self._execution_mode, self._device_id)?;
        Ok(Box::new(handler))
    }

    /// Build a test effect handler using real implementations for integration tests
    pub fn build_real_handler(&self) -> AuraResult<Box<dyn TestEffectHandler>> {
        // Create a composite handler with real implementations for integration testing
        let handler = CompositeTestHandler::new_real(self._execution_mode, self._device_id)?;
        Ok(Box::new(handler))
    }
}

/// Composite test handler that implements all required effect traits
///
/// This handler composes individual effect handlers from stateful_effects into a single
/// object that implements TestEffectHandler trait for convenient testing.
pub struct CompositeTestHandler {
    crypto: crate::stateful_effects::MockCryptoHandler,
    storage: crate::stateful_effects::MemoryStorageHandler,
    time: crate::stateful_effects::SimulatedTimeHandler,
    random: crate::stateful_effects::MockRandomHandler,
    console: crate::stateful_effects::MockConsoleHandler,
    journal: crate::stateful_effects::MockJournalHandler,
    network: crate::stateful_effects::InMemoryTransportHandler,
    execution_mode: ExecutionMode,
}

impl CompositeTestHandler {
    /// Create a new composite handler using mock implementations
    pub fn new_mock(execution_mode: ExecutionMode, _device_id: DeviceId) -> AuraResult<Self> {
        let seed = match execution_mode {
            ExecutionMode::Simulation { seed } => seed,
            _ => 42, // Default seed for deterministic testing
        };

        Ok(Self {
            crypto: crate::stateful_effects::MockCryptoHandler::new(),
            storage: crate::stateful_effects::MemoryStorageHandler::new(),
            time: crate::stateful_effects::SimulatedTimeHandler::new(),
            random: crate::stateful_effects::MockRandomHandler::new_with_seed(seed),
            console: crate::stateful_effects::MockConsoleHandler::new(),
            journal: crate::stateful_effects::MockJournalHandler::new(),
            network: crate::stateful_effects::InMemoryTransportHandler::default(),
            execution_mode,
        })
    }

    /// Create a new composite handler using real implementations for integration tests
    pub fn new_real(execution_mode: ExecutionMode, _device_id: DeviceId) -> AuraResult<Self> {
        // For integration tests, use mock handlers configured for more realistic behavior
        // This provides better determinism while still testing realistic patterns
        let seed = match execution_mode {
            ExecutionMode::Simulation { seed } => seed,
            _ => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        Ok(Self {
            crypto: crate::stateful_effects::MockCryptoHandler::new(),
            storage: crate::stateful_effects::MemoryStorageHandler::new(),
            time: crate::stateful_effects::SimulatedTimeHandler::new(),
            random: crate::stateful_effects::MockRandomHandler::new_with_seed(seed),
            console: crate::stateful_effects::MockConsoleHandler::new(),
            journal: crate::stateful_effects::MockJournalHandler::new(),
            network: crate::stateful_effects::InMemoryTransportHandler::default(),
            execution_mode,
        })
    }
}

impl TestEffectHandler for CompositeTestHandler {
    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

// Delegate implementations for all required effect traits
use async_trait::async_trait;
use aura_core::effects::*;

#[async_trait]
impl PhysicalTimeEffects for CompositeTestHandler {
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        self.time.physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        self.time.sleep_ms(ms).await
    }
}

#[async_trait]
impl CryptoEffects for CompositeTestHandler {
    // Delegate to underlying crypto handler
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.hkdf_derive(ikm, salt, info, output_len).await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.derive_key(master_key, context).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.crypto.ed25519_generate_keypair().await
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.ed25519_sign(message, private_key).await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto
            .ed25519_verify(message, signature, public_key)
            .await
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
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.crypto
            .frost_create_signing_package(message, nonces, participants, public_key_package)
            .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .frost_sign_share(signing_package, key_share, nonces)
            .await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
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

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.ed25519_public_key(private_key).await
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
        aura_core::CryptoEffects::is_simulated(&self.crypto)
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        self.crypto.crypto_capabilities()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.secure_zero(data)
    }
}

#[async_trait]
impl StorageEffects for CompositeTestHandler {
    async fn store(&self, key: &str, data: Vec<u8>) -> Result<(), StorageError> {
        self.storage.store(key, data).await
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
#[async_trait]
impl RandomEffects for CompositeTestHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.random.random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        self.random.random_bytes_32().await
    }

    async fn random_u64(&self) -> u64 {
        self.random.random_u64().await
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        self.random.random_range(min, max).await
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        self.random.random_uuid().await
    }
}

#[async_trait]
impl ConsoleEffects for CompositeTestHandler {
    async fn log_info(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_info(message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_warn(message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_error(message).await
    }

    async fn log_debug(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_debug(message).await
    }
}

#[async_trait]
impl JournalEffects for CompositeTestHandler {
    async fn merge_facts(
        &self,
        target: &aura_core::Journal,
        delta: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        self.journal.merge_facts(target, delta).await
    }

    async fn refine_caps(
        &self,
        target: &aura_core::Journal,
        refinement: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        self.journal.refine_caps(target, refinement).await
    }

    async fn get_journal(&self) -> Result<aura_core::Journal, aura_core::AuraError> {
        self.journal.get_journal().await
    }

    async fn persist_journal(
        &self,
        journal: &aura_core::Journal,
    ) -> Result<(), aura_core::AuraError> {
        self.journal.persist_journal(journal).await
    }

    async fn get_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &aura_core::AuthorityId,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        self.journal.get_flow_budget(context, peer).await
    }

    async fn update_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &aura_core::AuthorityId,
        budget: &aura_core::FlowBudget,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        self.journal.update_flow_budget(context, peer, budget).await
    }

    async fn charge_flow_budget(
        &self,
        context: &aura_core::identifiers::ContextId,
        peer: &aura_core::AuthorityId,
        cost: u32,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        self.journal.charge_flow_budget(context, peer, cost).await
    }
}

#[async_trait]
impl NetworkEffects for CompositeTestHandler {
    async fn send_to_peer(
        &self,
        peer_id: uuid::Uuid,
        message: Vec<u8>,
    ) -> Result<(), aura_core::effects::NetworkError> {
        // Delegate to the NetworkEffects trait implementation on InMemoryTransportHandler
        NetworkEffects::send_to_peer(&self.network, peer_id, message).await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), aura_core::effects::NetworkError> {
        // Delegate to the NetworkEffects trait implementation on InMemoryTransportHandler
        NetworkEffects::broadcast(&self.network, message).await
    }

    async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), aura_core::effects::NetworkError> {
        self.network.receive().await
    }

    async fn receive_from(
        &self,
        peer_id: uuid::Uuid,
    ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
        self.network.receive_from(peer_id).await
    }

    async fn connected_peers(&self) -> Vec<uuid::Uuid> {
        self.network.connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: uuid::Uuid) -> bool {
        self.network.is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(
        &self,
    ) -> Result<aura_core::effects::PeerEventStream, aura_core::effects::NetworkError> {
        self.network.subscribe_to_peer_events().await
    }
}

/// Convenience functions for common test scenarios
/// Create a simple mock effect context for unit tests
pub fn create_mock_test_context() -> AuraResult<SimpleTestContext> {
    Ok(SimpleTestContext::new(ExecutionMode::Testing))
}

/// Create a simulation context with deterministic behavior
pub fn create_simulation_context(seed: u64) -> AuraResult<SimpleTestContext> {
    Ok(SimpleTestContext::new(ExecutionMode::Simulation { seed }))
}

/// Create a production-like context for integration tests
pub fn create_integration_context() -> AuraResult<SimpleTestContext> {
    Ok(SimpleTestContext::new(ExecutionMode::Production))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_context_creation() {
        let context = SimpleTestContext::new(ExecutionMode::Testing);
        assert_eq!(context.execution_mode(), ExecutionMode::Testing);
        assert_ne!(context.device_id(), DeviceId::new()); // Should have unique ID
    }

    #[test]
    fn test_context_with_device_id() {
        let device_id = DeviceId::new();
        let context = SimpleTestContext::with_device_id(ExecutionMode::Testing, device_id);
        assert_eq!(context.execution_mode(), ExecutionMode::Testing);
        assert_eq!(context.device_id(), device_id);
    }

    #[test]
    fn test_convenience_functions() {
        let mock_context = create_mock_test_context().unwrap();
        assert_eq!(mock_context.execution_mode(), ExecutionMode::Testing);

        let sim_context = create_simulation_context(42).unwrap();
        assert_eq!(
            sim_context.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );

        let integration_context = create_integration_context().unwrap();
        assert_eq!(
            integration_context.execution_mode(),
            ExecutionMode::Production
        );
    }
}
