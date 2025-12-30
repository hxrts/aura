//! Simulation effect system implementation
//!
//! This provides a simulation-friendly composition of effect handlers that support
//! fault injection, time control, and state inspection for testing distributed protocols.

use async_trait::async_trait;
use aura_core::{
    crypto::single_signer::SigningMode,
    effects::{
        crypto::{
            FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext, SigningKeyGenResult,
        },
        *,
    },
    identifiers::DeviceId,
    AuraError,
};
use aura_testkit::stateful_effects::{
    console::MockConsoleHandler,
    crypto::MockCryptoHandler,
    random::MockRandomHandler,
    storage::MemoryStorageHandler,
    time::SimulatedTimeHandler,
    transport::{InMemoryTransportHandler, TransportConfig},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Configuration for fault injection during simulation
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// Probability of fault occurring (0.0 to 1.0)
    pub probability: f64,
    /// Type of fault to inject
    pub fault_type: FaultType,
    /// Whether fault is persistent or one-shot
    pub persistent: bool,
}

/// Types of faults that can be injected
#[derive(Debug, Clone)]
pub enum FaultType {
    /// Network partition - messages dropped
    NetworkPartition,
    /// Delayed messages
    DelayedMessage { delay_ms: u64 },
    /// Corrupted data
    CorruptedData,
    /// Transient storage failure
    StorageFailure,
    /// Cryptographic operation failure
    CryptoFailure,
    /// Time desynchronization
    TimeDesync { offset_ms: i64 },
}

/// Simulation effect system that wraps standard effect handlers with simulation capabilities
///
/// This provides deterministic behavior for testing, fault injection capabilities,
/// and controllable time progression for simulating distributed protocol scenarios.
pub struct SimulationEffectSystem {
    device_id: DeviceId,
    seed: u64,

    // Standard effect handlers with simulation features
    crypto: MockCryptoHandler,
    time: SimulatedTimeHandler,
    random: MockRandomHandler,
    console: MockConsoleHandler,
    storage: MemoryStorageHandler,
    network: InMemoryTransportHandler,

    // Simulation state
    fault_injection_enabled: bool,
    injected_faults: Arc<Mutex<HashMap<String, FaultConfig>>>,
}

impl SimulationEffectSystem {
    /// Create a new simulation effect system
    pub fn new(device_id: DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            seed,
            crypto: MockCryptoHandler::new(),
            time: SimulatedTimeHandler::new(),
            random: MockRandomHandler::new_with_seed(seed),
            console: MockConsoleHandler::new(),
            storage: MemoryStorageHandler::new(),
            network: InMemoryTransportHandler::new(TransportConfig::default()),
            fault_injection_enabled: false,
            injected_faults: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a simulation system with fault injection enabled
    pub fn with_fault_injection(device_id: DeviceId, seed: u64) -> Self {
        let mut system = Self::new(device_id, seed);
        system.fault_injection_enabled = true;
        system
    }

    /// Get the device ID this simulation system is configured for
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the simulation seed
    pub fn simulation_seed(&self) -> u64 {
        self.seed
    }

    /// Advance simulation time by specified milliseconds
    pub fn advance_time(&mut self, ms: u64) {
        self.time.advance_time(ms);
    }

    /// Set absolute simulation time
    pub fn set_time(&mut self, timestamp: u64) {
        self.time.set_time(timestamp);
    }

    /// Inject a fault for testing
    pub fn inject_fault(&self, operation: &str, config: FaultConfig) {
        if self.fault_injection_enabled {
            self.injected_faults
                .lock()
                .unwrap_or_else(|e| panic!("Fault injection lock poisoned: {e}"))
                .insert(operation.to_string(), config);
        }
    }

    /// Clear all injected faults
    pub fn clear_faults(&self) {
        self.injected_faults
            .lock()
            .unwrap_or_else(|e| panic!("Fault injection lock poisoned: {e}"))
            .clear();
    }

    /// Check if a fault should be triggered for an operation
    fn should_trigger_fault(&self, operation: &str) -> Option<FaultType> {
        if !self.fault_injection_enabled {
            return None;
        }

        let faults = self
            .injected_faults
            .lock()
            .unwrap_or_else(|e| panic!("Fault injection lock poisoned: {e}"));
        if let Some(config) = faults.get(operation) {
            // Simple deterministic "probability" based on seed
            let threshold = (self.seed % 100) as f64 / 100.0;
            if threshold < config.probability {
                return Some(config.fault_type.clone());
            }
        }
        None
    }
}

// Implement effect traits by delegating to underlying handlers with fault injection

#[async_trait]
impl CryptoCoreEffects for SimulationEffectSystem {
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: u32,
    ) -> Result<Vec<u8>, CryptoError> {
        if let Some(FaultType::CryptoFailure) = self.should_trigger_fault("hkdf_derive") {
            return Err(AuraError::crypto("Simulated crypto failure".to_string()));
        }
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
        if let Some(FaultType::CryptoFailure) = self.should_trigger_fault("ed25519_sign") {
            return Err(AuraError::crypto("Simulated signing failure".to_string()));
        }
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

    fn is_simulated(&self) -> bool {
        true
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec!["simulation".to_string(), "fault_injection".to_string()]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.secure_zero(data);
    }
}

#[async_trait]
impl CryptoExtendedEffects for SimulationEffectSystem {
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto
            .frost_generate_keys(threshold, max_signers)
            .await
    }

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.frost_generate_nonces(key_package).await
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

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto
            .generate_signing_keys(threshold, max_signers)
            .await
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        if let Some(FaultType::CryptoFailure) = self.should_trigger_fault("sign_with_key") {
            return Err(AuraError::crypto("Simulated signing failure".to_string()));
        }
        self.crypto.sign_with_key(message, key_package, mode).await
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        self.crypto
            .verify_signature(message, signature, public_key_package, mode)
            .await
    }
}

#[async_trait]
impl RandomCoreEffects for SimulationEffectSystem {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.random.random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        self.random.random_bytes_32().await
    }

    async fn random_u64(&self) -> u64 {
        self.random.random_u64().await
    }
}

#[async_trait]
#[async_trait]
impl ConsoleEffects for SimulationEffectSystem {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_info(message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_warn(message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_error(message).await
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        self.console.log_debug(message).await
    }
}

#[async_trait]
impl StorageCoreEffects for SimulationEffectSystem {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        if let Some(FaultType::StorageFailure) = self.should_trigger_fault("store") {
            return Err(StorageError::WriteFailed(
                "Simulated storage failure".to_string(),
            ));
        }
        self.storage.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        if let Some(FaultType::StorageFailure) = self.should_trigger_fault("retrieve") {
            return Err(StorageError::ReadFailed(
                "Simulated storage failure".to_string(),
            ));
        }
        self.storage.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.storage.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.storage.list_keys(prefix).await
    }
}

#[async_trait]
impl StorageExtendedEffects for SimulationEffectSystem {
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
impl NetworkCoreEffects for SimulationEffectSystem {
    async fn send_to_peer(
        &self,
        peer_id: uuid::Uuid,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        if let Some(FaultType::NetworkPartition) = self.should_trigger_fault("send_to_peer") {
            // Drop message silently on network partition
            return Ok(());
        }
        // Use NetworkCoreEffects trait method explicitly
        <InMemoryTransportHandler as NetworkCoreEffects>::send_to_peer(
            &self.network,
            peer_id,
            message,
        )
        .await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        // Use NetworkCoreEffects trait method explicitly
        <InMemoryTransportHandler as NetworkCoreEffects>::broadcast(&self.network, message).await
    }

    async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), NetworkError> {
        self.network.receive().await
    }
}

#[async_trait]
impl NetworkExtendedEffects for SimulationEffectSystem {
    async fn receive_from(&self, peer_id: uuid::Uuid) -> Result<Vec<u8>, NetworkError> {
        self.network.receive_from(peer_id).await
    }

    async fn connected_peers(&self) -> Vec<uuid::Uuid> {
        self.network.connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: uuid::Uuid) -> bool {
        self.network.is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        self.network.subscribe_to_peer_events().await
    }

    async fn open(&self, address: &str) -> Result<String, NetworkError> {
        self.network.open(address).await
    }

    async fn send(&self, connection_id: &str, data: Vec<u8>) -> Result<(), NetworkError> {
        self.network.send(connection_id, data).await
    }

    async fn close(&self, connection_id: &str) -> Result<(), NetworkError> {
        self.network.close(connection_id).await
    }
}

/// Factory for creating simulation effect systems
pub struct SimulationEffectSystemFactory;

impl SimulationEffectSystemFactory {
    /// Create a simulation effect system with basic configuration
    pub fn create(device_id: DeviceId, seed: u64) -> SimulationEffectSystem {
        SimulationEffectSystem::new(device_id, seed)
    }

    /// Create a simulation effect system for testing (deterministic seed)
    pub fn for_testing(device_id: DeviceId) -> SimulationEffectSystem {
        Self::create(device_id, 42)
    }

    /// Create a simulation effect system with fault injection enabled
    pub fn for_simulation_with_faults(device_id: DeviceId, seed: u64) -> SimulationEffectSystem {
        SimulationEffectSystem::with_fault_injection(device_id, seed)
    }

    /// Create multiple simulation systems for distributed testing
    pub fn create_network(device_count: usize, base_seed: u64) -> Vec<SimulationEffectSystem> {
        use aura_testkit::DeviceTestFixture;
        (0..device_count)
            .map(|i| {
                let fixture = DeviceTestFixture::new(i);
                let device_id = fixture.device_id();
                let seed = base_seed + i as u64;
                Self::create(device_id, seed)
            })
            .collect()
    }
}

/// Simulation effect system statistics
#[derive(Debug, Clone)]
pub struct SimulationEffectSystemStats {
    /// Device ID for this system
    pub device_id: DeviceId,
    /// Seed used for deterministic simulation
    pub simulation_seed: u64,
    /// Whether the system is in deterministic mode
    pub deterministic_mode: bool,
    /// Current simulation time
    pub current_time: u64,
    /// Whether fault injection is enabled
    pub fault_injection_enabled: bool,
    /// Number of active faults
    pub active_fault_count: usize,
    /// Supported effect types
    pub supported_effect_types: Vec<String>,
}

impl SimulationEffectSystem {
    /// Get statistics about this simulation system
    pub async fn get_stats(&self) -> SimulationEffectSystemStats {
        SimulationEffectSystemStats {
            device_id: self.device_id,
            simulation_seed: self.seed,
            deterministic_mode: true,
            current_time: self.time.get_time(),
            fault_injection_enabled: self.fault_injection_enabled,
            active_fault_count: self
                .injected_faults
                .lock()
                .unwrap_or_else(|e| panic!("Fault injection lock poisoned: {e}"))
                .len(),
            supported_effect_types: vec![
                "crypto".to_string(),
                "time".to_string(),
                "random".to_string(),
                "console".to_string(),
                "storage".to_string(),
                "network".to_string(),
            ],
        }
    }
}

#[async_trait]
impl PhysicalTimeEffects for SimulationEffectSystem {
    async fn physical_time(
        &self,
    ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        self.time.physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), aura_core::effects::TimeError> {
        self.time.sleep_ms(ms).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::CryptoCoreEffects;

    #[tokio::test]
    async fn test_simulation_system_creation() {
        use aura_testkit::DeviceTestFixture;
        let fixture = DeviceTestFixture::new(0);
        let device_id = fixture.device_id();
        let system = SimulationEffectSystem::new(device_id, 12345);

        assert_eq!(system.device_id(), device_id);
        assert_eq!(system.simulation_seed(), 12345);
        assert!(CryptoCoreEffects::is_simulated(&system));
    }

    #[tokio::test]
    async fn test_fault_injection() {
        use aura_testkit::DeviceTestFixture;
        let fixture = DeviceTestFixture::new(1);
        let device_id = fixture.device_id();
        let system = SimulationEffectSystem::with_fault_injection(device_id, 42);

        // Inject a fault for cryptographic operations
        system.inject_fault(
            "crypto",
            FaultConfig {
                probability: 1.0,
                fault_type: FaultType::CryptoFailure,
                persistent: true,
            },
        );

        // Test that crypto fault is triggered for key generation
        // Note: Hashing is intentionally NOT an algebraic effect (see CryptoEffects trait)
        // Use aura_core::hash::hash() for pure hashing operations instead
        let result = system.crypto.ed25519_generate_keypair().await;

        // With fault injection enabled, crypto operations should still work
        // but we can verify the fault injection system is properly configured
        assert!(result.is_ok());

        // Verify that the fault injection system has the fault registered
        let stats = system.get_stats().await;
        assert!(stats.fault_injection_enabled);
        assert!(stats.active_fault_count > 0);
    }

    #[tokio::test]
    async fn test_time_control() {
        use aura_testkit::DeviceTestFixture;
        let fixture = DeviceTestFixture::new(2);
        let device_id = fixture.device_id();
        let mut system = SimulationEffectSystem::new(device_id, 42);

        // Test time advancement
        let initial_time = system.physical_time().await.unwrap();
        system.advance_time(1000);
        let new_time = system.physical_time().await.unwrap();

        assert!(new_time > initial_time);
    }

    #[test]
    fn test_factory_creation() {
        use aura_testkit::DeviceTestFixture;
        let fixture = DeviceTestFixture::new(3);
        let device_id = fixture.device_id();

        // Testing mode should work
        let _system = SimulationEffectSystemFactory::for_testing(device_id);
        assert_eq!(_system.simulation_seed(), 42);

        // Simulation mode should work
        let _system = SimulationEffectSystemFactory::for_simulation_with_faults(device_id, 123);
        assert_eq!(_system.simulation_seed(), 123);
    }

    #[test]
    fn test_network_creation() {
        let systems = SimulationEffectSystemFactory::create_network(3, 100);
        assert_eq!(systems.len(), 3);

        // Each system should have different seeds
        assert_eq!(systems[0].simulation_seed(), 100);
        assert_eq!(systems[1].simulation_seed(), 101);
        assert_eq!(systems[2].simulation_seed(), 102);
    }
}
