//! Cryptographic effect handlers
//!
//! Provides different implementations of CryptoEffects for various execution contexts.

pub mod mock;
pub mod real;

pub use mock::MockCryptoHandler;
pub use real::RealCryptoHandler;
