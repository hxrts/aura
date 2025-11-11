//! Cryptographic Effect Handlers
//!
//! Provides context-free implementations of cryptographic operations.

mod mock;
mod real;

pub use mock::MockCryptoHandler;
pub use real::RealCryptoHandler;
