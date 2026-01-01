//! Crypto Subsystem
//!
//! Groups cryptographic-related fields from AuraEffectSystem:
//! - `crypto_handler`: Core cryptographic operations (signing, verification, hashing)
//! - `random_rng`: Cryptographically secure RNG for key generation
//! - `secure_storage_handler`: Secure storage for key material (FROST keys, device keys)
//!
//! ## Lock Usage
//!
//! Uses a thread-local RNG in production to avoid contention, and a
//! `parking_lot::Mutex`-backed deterministic RNG in tests/simulation.
//! RNG operations are synchronous and never held across async boundaries.

#![allow(clippy::disallowed_types)]

use aura_effects::{crypto::RealCryptoHandler, secure::RealSecureStorageHandler};
use parking_lot::Mutex;
use rand::rngs::StdRng;
use rand::{RngCore, SeedableRng};
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static THREAD_RNG: RefCell<StdRng> = RefCell::new(StdRng::from_entropy());
}

#[derive(Debug)]
pub(crate) enum CryptoRng {
    Deterministic(Box<Mutex<StdRng>>),
    ThreadLocal,
}

impl CryptoRng {
    pub(crate) fn deterministic(rng: StdRng) -> Self {
        CryptoRng::Deterministic(Box::new(Mutex::new(rng)))
    }

    pub(crate) fn thread_local() -> Self {
        CryptoRng::ThreadLocal
    }

    fn fill_bytes(&self, bytes: &mut [u8]) {
        match self {
            CryptoRng::Deterministic(rng) => {
                let mut rng = rng.lock();
                rng.fill_bytes(bytes);
            }
            CryptoRng::ThreadLocal => {
                THREAD_RNG.with(|cell| {
                    let mut rng = cell.borrow_mut();
                    rng.fill_bytes(bytes);
                });
            }
        }
    }

    fn next_u64(&self) -> u64 {
        match self {
            CryptoRng::Deterministic(rng) => {
                let mut rng = rng.lock();
                rng.next_u64()
            }
            CryptoRng::ThreadLocal => THREAD_RNG.with(|cell| {
                let mut rng = cell.borrow_mut();
                rng.next_u64()
            }),
        }
    }
}

impl Clone for CryptoRng {
    fn clone(&self) -> Self {
        match self {
            CryptoRng::Deterministic(rng) => {
                let rng = rng.lock();
                CryptoRng::Deterministic(Box::new(Mutex::new(rng.clone())))
            }
            CryptoRng::ThreadLocal => CryptoRng::ThreadLocal,
        }
    }
}

/// Crypto subsystem grouping cryptographic operations and key management.
///
/// This subsystem encapsulates:
/// - Cryptographic primitives (signing, verification, key generation)
/// - Secure random number generation
/// - Secure storage for cryptographic key material
pub struct CryptoSubsystem {
    /// Core cryptographic handler for signing, verification, and key operations
    handler: RealCryptoHandler,

    /// Cryptographically secure RNG for key generation and nonces.
    ///
    /// Production uses a thread-local RNG. Deterministic modes use a mutex-backed RNG.
    rng: CryptoRng,

    /// Secure storage for key material (FROST keys, device keys)
    ///
    /// Uses platform-specific secure storage (Keychain, TPM, Keystore)
    secure_storage: Arc<RealSecureStorageHandler>,
}

impl CryptoSubsystem {
    /// Create a new crypto subsystem with production random source
    #[allow(dead_code)]
    pub fn new(base_path: std::path::PathBuf) -> Self {
        Self {
            handler: RealCryptoHandler::new(),
            rng: CryptoRng::thread_local(),
            secure_storage: Arc::new(RealSecureStorageHandler::with_base_path(base_path)),
        }
    }

    /// Create a crypto subsystem with deterministic seed for testing
    #[allow(dead_code)]
    pub fn seeded(seed: [u8; 32], base_path: std::path::PathBuf) -> Self {
        Self {
            handler: RealCryptoHandler::seeded(seed),
            rng: CryptoRng::deterministic(StdRng::from_seed(seed)),
            secure_storage: Arc::new(RealSecureStorageHandler::with_base_path(base_path)),
        }
    }

    /// Create from existing components
    pub fn from_parts(
        handler: RealCryptoHandler,
        rng: CryptoRng,
        secure_storage: Arc<RealSecureStorageHandler>,
    ) -> Self {
        Self {
            handler,
            rng,
            secure_storage,
        }
    }

    /// Get reference to the crypto handler
    pub fn handler(&self) -> &RealCryptoHandler {
        &self.handler
    }

    /// Get clone of the crypto handler (for effect trait delegation)
    #[allow(dead_code)]
    pub fn handler_clone(&self) -> RealCryptoHandler {
        self.handler.clone()
    }

    /// Get shared secure storage handler
    pub fn secure_storage(&self) -> Arc<RealSecureStorageHandler> {
        self.secure_storage.clone()
    }

    /// Generate random bytes using the subsystem's RNG
    ///
    /// This is the single point for random byte generation in the crypto subsystem.
    pub fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        self.rng.fill_bytes(&mut bytes);
        bytes
    }

    /// Generate a random u64
    pub fn random_u64(&self) -> u64 {
        self.rng.next_u64()
    }

    /// Generate a random [u8; 32] array
    pub fn random_32_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        self.rng.fill_bytes(&mut bytes);
        bytes
    }
}

impl Clone for CryptoSubsystem {
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            rng: self.rng.clone(),
            secure_storage: self.secure_storage.clone(),
        }
    }
}

impl std::fmt::Debug for CryptoSubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoSubsystem")
            .field("handler", &"<RealCryptoHandler>")
            .field("rng", &format_args!("{:?}", self.rng))
            .field("secure_storage", &"<Arc<RealSecureStorageHandler>>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_subsystem_creation() {
        let temp_dir = std::env::temp_dir().join("crypto_subsystem_test");
        let subsystem = CryptoSubsystem::new(temp_dir);
        assert!(subsystem.random_bytes(32).len() == 32);
    }

    #[test]
    fn test_seeded_crypto_subsystem() {
        let temp_dir = std::env::temp_dir().join("crypto_subsystem_seeded_test");
        let seed = [42u8; 32];
        let subsystem1 = CryptoSubsystem::seeded(seed, temp_dir.clone());
        let subsystem2 = CryptoSubsystem::seeded(seed, temp_dir);

        // Seeded subsystems should produce same random values
        let bytes1 = subsystem1.random_bytes(16);
        let bytes2 = subsystem2.random_bytes(16);
        assert_eq!(bytes1, bytes2);
    }
}
