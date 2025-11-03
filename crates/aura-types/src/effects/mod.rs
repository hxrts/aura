//! Universal Effects System for Aura
//!
//! This module provides the foundational effects system that enables
//! deterministic testing and clean architecture across all Aura components.
//!
//! ## Design Principles
//!
//! 1. **Algebraic Effects**: Pure functions accept effects as parameters
//! 2. **Effect Isolation**: All side effects are contained within effect traits
//! 3. **Testability**: All effects can be mocked/injected for deterministic testing
//! 4. **Universal Usage**: Same effects system used by all layers (protocol, journal, storage, etc.)
//! 5. **Composition**: Effects can be combined and layered
//!
//! ## Core Effect Categories
//!
//! - **Cryptographic Effects**: Signing, verification, key operations
//! - **Time Effects**: Scheduling, timeouts, cooperative yielding
//! - **Storage Effects**: File I/O, persistence operations
//! - **Network Effects**: Communication, transport operations
//! - **Random Effects**: Deterministic randomness for testing
//! - **Console Effects**: Logging, debugging, tracing
//!
//! ## Usage Pattern
//!
//! ```rust
//! use aura_types::effects::{AuraEffects, TimeEffects, CryptoEffects};
//!
//! // Pure function that accepts effects
//! async fn execute_operation(
//!     state: ComponentState,
//!     effects: &impl AuraEffects,
//! ) -> Result<ComponentState, AuraError> {
//!     // Use effects for side-effect operations
//!     let timestamp = effects.current_timestamp();
//!     let signature = effects.sign_data(&data, &key).await?;
//!
//!     // Pure logic using the effect results
//!     Ok(state.with_timestamp(timestamp).with_signature(signature))
//! }
//! ```

pub mod console;
pub mod crypto;
pub mod network;
pub mod random;
pub mod storage;
pub mod time;

// Re-export core effect types
pub use console::{ConsoleEffects, ConsoleEvent, LogLevel};
pub use crypto::{
    CryptoEffects, Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey, SigningError,
    VerificationError,
};
pub use network::{NetworkAddress, NetworkEffects, NetworkError};
pub use random::{ProductionRandomEffects, RandomEffects, TestRandomEffects};
pub use storage::{StorageEffects, StorageError, StorageLocation};
pub use time::{ProductionTimeEffects, TestTimeEffects, TimeEffects, WakeCondition};

// Note: Effects struct is defined below, no need to re-export

// Re-export standard Duration
pub use std::time::Duration;

/// Timeout error for time effects
#[derive(Debug, thiserror::Error)]
pub enum TimeoutError {
    /// Operation exceeded the specified timeout duration
    #[error("Operation timed out")]
    Elapsed,
}

use crate::identifiers::DeviceId;
use ed25519_dalek::Signer;
use std::future::Future;

/// Main trait that combines all effect categories for convenience
pub trait AuraEffects:
    CryptoEffects
    + TimeEffects
    + StorageEffects
    + NetworkEffects
    + RandomEffects
    + ConsoleEffects
    + Send
    + Sync
{
    /// Get the device ID for this effects instance
    fn device_id(&self) -> DeviceId;

    /// Check if this is running in simulation mode
    fn is_simulation(&self) -> bool;

    /// Create an isolated effects instance for a specific component
    fn isolate(&self, component_id: &str) -> DefaultEffects;
}

/// Builder for creating effects instances
pub struct EffectsBuilder {
    device_id: Option<DeviceId>,
    is_simulation: bool,
}

impl EffectsBuilder {
    /// Create a new effects builder
    pub fn new() -> Self {
        Self {
            device_id: None,
            is_simulation: false,
        }
    }

    /// Set the device ID
    pub fn with_device_id(mut self, device_id: DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Enable simulation mode
    pub fn with_simulation(mut self) -> Self {
        self.is_simulation = true;
        self
    }

    /// Build a default effects instance
    pub fn build(self) -> DefaultEffects {
        DefaultEffects {
            device_id: self.device_id.unwrap_or_else(|| DeviceId::from("default")),
            is_simulation: self.is_simulation,
        }
    }
}

impl Default for EffectsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Default implementation of AuraEffects for production use
pub struct DefaultEffects {
    device_id: DeviceId,
    is_simulation: bool,
}

impl DefaultEffects {
    /// Create new default effects
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            is_simulation: false,
        }
    }

    /// Create simulation effects
    pub fn simulation(device_id: DeviceId) -> Self {
        Self {
            device_id,
            is_simulation: true,
        }
    }
}

impl AuraEffects for DefaultEffects {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    fn is_simulation(&self) -> bool {
        self.is_simulation
    }

    fn isolate(&self, _component_id: &str) -> DefaultEffects {
        DefaultEffects {
            device_id: self.device_id,
            is_simulation: self.is_simulation,
        }
    }
}

// Implement all effect traits for DefaultEffects with simple implementations
impl CryptoEffects for DefaultEffects {
    fn sign_data(
        &self,
        _data: &[u8],
        _key: &crypto::Ed25519SigningKey,
    ) -> Result<crypto::Ed25519Signature, SigningError> {
        // TODO: Implement actual signing when aura-crypto is available
        Ok(crypto::Ed25519Signature::default())
    }

    fn verify_signature(
        &self,
        _data: &[u8],
        _signature: &crypto::Ed25519Signature,
        _public_key: &crypto::Ed25519VerifyingKey,
    ) -> Result<bool, VerificationError> {
        // TODO: Implement actual verification when aura-crypto is available
        Ok(true)
    }

    fn generate_signing_key(&self) -> crypto::Ed25519SigningKey {
        // TODO: Implement actual key generation when aura-crypto is available
        crypto::Ed25519SigningKey::default()
    }

    fn derive_key(
        &self,
        _seed: &[u8],
        _context: &str,
    ) -> Result<crypto::Ed25519SigningKey, crate::AuraError> {
        // TODO: Implement actual key derivation when aura-crypto is available
        Ok(crypto::Ed25519SigningKey::default())
    }
}

impl TimeEffects for DefaultEffects {
    fn current_timestamp(&self) -> u64 {
        if self.is_simulation {
            // Return deterministic timestamp for testing
            1234567890
        } else {
            #[allow(clippy::disallowed_methods)]
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        }
    }

    fn delay(&self, duration: Duration) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if !is_simulation {
                std::thread::sleep(duration); // Placeholder
            }
            // In simulation, sleep returns immediately
        })
    }

    fn yield_until(
        &self,
        _condition: WakeCondition,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), crate::AuraError>> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // In simulation, conditions are met immediately
                Ok(())
            } else {
                // Placeholder implementation
                Ok(())
            }
        })
    }
}

impl StorageEffects for DefaultEffects {
    fn read_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, StorageError>> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // Return empty data in simulation
                Ok(Vec::new())
            } else {
                std::fs::read(location.path()).map_err(|e| StorageError::ReadFailed(e.to_string()))
            }
        })
    }

    fn write_file(
        &self,
        location: StorageLocation,
        data: &[u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>> {
        let data = data.to_vec(); // Clone for move
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // No-op in simulation
                Ok(())
            } else {
                std::fs::write(location.path(), data)
                    .map_err(|e| StorageError::WriteFailed(e.to_string()))
            }
        })
    }

    fn delete_file(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // No-op in simulation
                Ok(())
            } else {
                std::fs::remove_file(location.path())
                    .map_err(|e| StorageError::DeleteFailed(e.to_string()))
            }
        })
    }

    fn list_files(
        &self,
        location: StorageLocation,
    ) -> std::pin::Pin<
        Box<dyn Future<Output = Result<Vec<StorageLocation>, StorageError>> + Send + '_>,
    > {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // Return empty list in simulation
                Ok(Vec::new())
            } else {
                match std::fs::read_dir(location.path()) {
                    Ok(entries) => {
                        let mut files = Vec::new();
                        for entry in entries {
                            match entry {
                                Ok(entry) => files.push(StorageLocation::from_path(entry.path())),
                                Err(e) => return Err(StorageError::ListFailed(e.to_string())),
                            }
                        }
                        Ok(files)
                    }
                    Err(e) => Err(StorageError::ListFailed(e.to_string())),
                }
            }
        })
    }
}

impl NetworkEffects for DefaultEffects {
    fn send_message(
        &self,
        _address: NetworkAddress,
        _data: &[u8],
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), NetworkError>> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // Simulate successful send
                Ok(())
            } else {
                // TODO: Implement actual network sending
                Err(NetworkError::NotImplemented)
            }
        })
    }

    fn receive_message(
        &self,
        _address: NetworkAddress,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<Vec<u8>, NetworkError>> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // Return empty message in simulation
                Ok(Vec::new())
            } else {
                // TODO: Implement actual network receiving
                Err(NetworkError::NotImplemented)
            }
        })
    }

    fn connect(
        &self,
        _address: NetworkAddress,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), NetworkError>> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // Simulate successful connection
                Ok(())
            } else {
                // TODO: Implement actual connection
                Err(NetworkError::NotImplemented)
            }
        })
    }

    fn disconnect(
        &self,
        _address: NetworkAddress,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), NetworkError>> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if is_simulation {
                // Simulate successful disconnection
                Ok(())
            } else {
                // TODO: Implement actual disconnection
                Err(NetworkError::NotImplemented)
            }
        })
    }
}

impl RandomEffects for DefaultEffects {
    fn random_bytes(&self, len: usize) -> Vec<u8> {
        if self.is_simulation {
            // Use deterministic test effects
            random::TestRandomEffects::new(42).random_bytes(len)
        } else {
            // Use production effects
            random::ProductionRandomEffects::new().random_bytes(len)
        }
    }

    fn random_u64(&self) -> u64 {
        if self.is_simulation {
            // Use deterministic test effects
            random::TestRandomEffects::new(42).random_u64()
        } else {
            // Use production effects
            random::ProductionRandomEffects::new().random_u64()
        }
    }

    fn random_range(&self, min: u64, max: u64) -> u64 {
        if self.is_simulation {
            // Use deterministic test effects
            random::TestRandomEffects::new(42).random_range(min, max)
        } else {
            // Use production effects
            random::ProductionRandomEffects::new().random_range(min, max)
        }
    }

    fn rng(&self) -> Box<dyn random::CryptoRngCore> {
        if self.is_simulation {
            // Use deterministic test effects
            random::TestRandomEffects::new(42).rng()
        } else {
            // Use production effects
            random::ProductionRandomEffects::new().rng()
        }
    }
}

impl ConsoleEffects for DefaultEffects {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        if !self.is_simulation {
            println!("[TRACE] {}: {:?}", message, fields);
        }
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        if !self.is_simulation {
            println!("[DEBUG] {}: {:?}", message, fields);
        }
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        if !self.is_simulation {
            println!("[INFO] {}: {:?}", message, fields);
        }
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        if !self.is_simulation {
            println!("[WARN] {}: {:?}", message, fields);
        }
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        if !self.is_simulation {
            println!("[ERROR] {}: {:?}", message, fields);
        }
    }

    fn emit_event(
        &self,
        event: ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let is_simulation = self.is_simulation;
        Box::pin(async move {
            if !is_simulation {
                println!("[EVENT] {:?}", event);
            }
        })
    }
}

/// Convenient Effects bundle that provides commonly needed effects
/// This is a concrete implementation that can be used by components that need effects
#[derive(Clone)]
pub struct Effects {
    random: Arc<dyn RandomEffects + Send + Sync>,
    time: Arc<dyn TimeEffects + Send + Sync>,
}

impl Effects {
    /// Create production effects (real time + OS randomness)
    pub fn production() -> Self {
        Self {
            random: Arc::new(random::ProductionRandomEffects::new()),
            time: Arc::new(time::ProductionTimeEffects::new()),
        }
    }

    /// Create deterministic test effects (simulated time + seeded RNG)
    pub fn deterministic(seed: u64, initial_time: u64) -> Self {
        Self {
            random: Arc::new(random::TestRandomEffects::new(seed)),
            time: Arc::new(time::TestTimeEffects::new(initial_time)),
        }
    }

    /// Create test effects with default seed and recent time
    pub fn test() -> Self {
        Self::deterministic(0, 1735689600) // 2025-01-01
    }

    /// Create test effects isolated by test name
    pub fn for_test(test_name: &str) -> Self {
        Self {
            random: Arc::new(random::TestRandomEffects::from_test_name(test_name)),
            time: Arc::new(time::TestTimeEffects::new(1735689600)), // 2025-01-01
        }
    }

    /// Get an RNG that implements the rand crate traits
    pub fn rng(&self) -> Box<dyn random::CryptoRngCore> {
        self.random.rng()
    }

    /// Generate random bytes (const generic version)
    pub fn random_bytes_array<const N: usize>(&self) -> [u8; N] {
        let bytes = self.random.random_bytes(N);
        let mut array = [0u8; N];
        array.copy_from_slice(&bytes[..N.min(bytes.len())]);
        array
    }

    /// Get current timestamp
    pub fn now(&self) -> u64 {
        self.time.current_timestamp()
    }

    /// Generate a UUID (deterministic in tests)
    pub fn gen_uuid(&self) -> uuid::Uuid {
        let mut bytes = [0u8; 16];
        let random_bytes = self.random.random_bytes(16);
        bytes.copy_from_slice(&random_bytes);
        uuid::Uuid::from_bytes(bytes)
    }

    /// Generate random bytes (non-const version)
    pub fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.random.random_bytes(len)
    }

    /// Blake3 hash function
    pub fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        blake3::hash(data).into()
    }

    /// Check if this is a simulated environment
    pub fn is_simulated(&self) -> bool {
        // We can determine this based on the type of random effects
        // For now, return false - this should be properly implemented when needed
        false
    }

    /// Generate a signing key
    pub fn generate_signing_key(&self) -> ed25519_dalek::SigningKey {
        // Use the random bytes directly to create a signing key
        let key_bytes = self.random_bytes_array::<32>();
        ed25519_dalek::SigningKey::from_bytes(&key_bytes)
    }

    /// Sign data with a signing key
    pub fn sign_data(
        &self,
        data: &[u8],
        key: &ed25519_dalek::SigningKey,
    ) -> ed25519_dalek::Signature {
        key.sign(data)
    }

    /// Verify a signature
    pub fn verify_signature(
        &self,
        data: &[u8],
        signature: &ed25519_dalek::Signature,
        public_key: &ed25519_dalek::VerifyingKey,
    ) -> bool {
        public_key.verify_strict(data, signature).is_ok()
    }

    /// Delay/sleep for a duration
    pub async fn delay(&self, duration: Duration) {
        self.time.delay(duration).await
    }
}

use std::sync::Arc;
