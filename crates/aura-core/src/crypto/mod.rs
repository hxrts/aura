pub mod amp;
pub mod ed25519;
pub mod frost;
pub mod hash;
pub mod hpke;
pub mod key_derivation;
pub mod merkle;
pub mod single_signer;
pub mod tree_signing;

// Merkle helpers
pub use merkle::{
    build_commitment_tree, build_merkle_root, generate_merkle_proof, verify_merkle_proof,
    SimpleMerkleProof,
};

// Deterministic key derivation types
pub use key_derivation::{IdentityKeyContext, KeyDerivationSpec, PermissionKeyContext};

// Ed25519 types and operations
pub use ed25519::{
    ed25519_verify, ed25519_verifying_key, Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey,
};

// HPKE types
pub use hpke::{HpkeKeyPair, HpkePrivateKey, HpkePublicKey};

// Single-signer types (for 1-of-1 scenarios)
pub use single_signer::{
    SigningMode, SingleSignerKeyPackage, SingleSignerPublicKeyPackage,
};

/// Alias for Merkle proof re-export.
pub type MerkleProof = SimpleMerkleProof;

/// Opaque permission key context alias retained for compatibility.
pub type PermissionKeyContextCompat = PermissionKeyContext;
