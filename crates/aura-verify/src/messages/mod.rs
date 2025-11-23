//! Layer 2: Verification Message Types
//!
//! Message types supporting cryptographic verification operations:
//! threshold signatures (FROST), key resharing/rotation protocols.
//!
//! **Organization**:
//! - `crypto`: Cryptographic protocol messages (resharing, FROST, future DKD)
//!
//! All messages use aura-core message envelope (Layer 1) for versioning and serialization safety.

pub mod crypto;

// Re-export main crypto types
pub use crypto::*;
