//! Cryptographic Primitives and Key Management
//!
//! This module provides encryption, key derivation, and cryptographic primitives
//! for the rendezvous layer.

mod derivation;
pub mod encryption;
pub mod keys;
mod primitives;

pub use derivation::derive_test_root_key;
pub use encryption::{EncryptedEnvelope, EnvelopeEncryption, PaddingStrategy};
pub use keys::{RelationshipContext, RelationshipKey, RelationshipKeyManager};
pub use primitives::{BlindSignature, SecretBrand, UnlinkableCredential};
