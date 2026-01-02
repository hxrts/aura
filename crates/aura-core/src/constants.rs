//! Consolidated constants for Aura core
//!
//! This module centralizes size limits and default values that were
//! previously scattered across multiple modules. Constants are organized
//! by domain for easy discovery.
//!
//! # Usage
//!
//! ```rust
//! use aura_core::constants::{MAX_SIGNATURE_BYTES, MAX_FACT_PAYLOAD_BYTES};
//! ```

// =============================================================================
// Tree Operations
// =============================================================================

/// Maximum size for leaf public keys in bytes (Ed25519 FROST keys).
pub const MAX_LEAF_PUBLIC_KEY_BYTES: usize = 64;

/// Maximum size for leaf metadata in bytes.
pub const MAX_LEAF_META_BYTES: usize = 256;

/// Maximum size for aggregate signatures in bytes (FROST aggregate).
pub const MAX_AGG_SIG_BYTES: usize = 128;

/// Maximum size for tree signatures in bytes.
pub const MAX_TREE_SIGNATURE_BYTES: usize = 128;

/// Maximum depth for Merkle trees.
pub const MAX_MERKLE_DEPTH: u32 = 32;

/// Maximum size for tree state serialization.
pub const MAX_TREE_STATE_BYTES: usize = 262_144;

/// Maximum size for tree signature shares.
pub const MAX_TREE_SIGNATURE_SHARE_BYTES: usize = 1024;

/// Maximum size for tree aggregate signatures.
pub const MAX_TREE_AGGREGATE_SIGNATURE_BYTES: usize = 2048;

// =============================================================================
// Cryptographic Operations
// =============================================================================

/// Maximum size for FROST signing shares.
pub const MAX_SHARE_BYTES: usize = 32;

/// Exact size for postcard-serialized FROST commitments (SigningCommitments).
pub const MAX_COMMITMENT_BYTES: usize = 69;

/// Exact size for postcard-serialized FROST nonces (SigningNonces).
pub const MAX_NONCE_BYTES: usize = 138;

/// Maximum size for FROST partial signatures.
pub const MAX_PARTIAL_SIGNATURE_BYTES: usize = 32;

/// Maximum size for Ed25519/FROST signatures.
pub const MAX_SIGNATURE_BYTES: usize = 64;

/// Maximum size for Ed25519/FROST public keys.
pub const MAX_PUBLIC_KEY_BYTES: usize = 32;

/// Maximum size for single signer verifying keys.
pub const MAX_SINGLE_SIGNER_VERIFYING_KEY_BYTES: usize = 32;

/// Maximum size for threshold signatures.
pub const MAX_THRESHOLD_SIGNATURE_BYTES: usize = 512;

/// Maximum size for threshold public key packages.
pub const MAX_THRESHOLD_PUBLIC_KEY_PACKAGE_BYTES: usize = 65_536;

/// Maximum size for key packages.
pub const MAX_KEY_PACKAGE_BYTES: usize = 65_536;

/// Maximum size for public key packages.
pub const MAX_PUBLIC_KEY_PACKAGE_BYTES: usize = 65_536;

/// Maximum size for signing messages.
pub const MAX_SIGNING_MESSAGE_BYTES: usize = 65_536;

/// Maximum size for signing packages.
pub const MAX_SIGNING_PACKAGE_BYTES: usize = 65_536;

// =============================================================================
// Transport & Network
// =============================================================================

/// Maximum size for transport signatures.
pub const MAX_TRANSPORT_SIGNATURE_BYTES: usize = 512;

/// Maximum size for transport payloads.
pub const MAX_TRANSPORT_PAYLOAD_BYTES: usize = 65_536;

/// Maximum size for rendezvous payloads.
pub const MAX_RENDEZVOUS_PAYLOAD_BYTES: usize = 4096;

/// Default TTL for flood messages.
pub const DEFAULT_FLOOD_TTL: u8 = 3;

/// Default originate limit for flood budget.
pub const DEFAULT_FLOOD_ORIGINATE_LIMIT: u32 = 100;

/// Default forward limit for flood budget.
pub const DEFAULT_FLOOD_FORWARD_LIMIT: u32 = 1000;

// =============================================================================
// AMP (Authenticated Message Protocol)
// =============================================================================

/// Maximum size for AMP ciphertext.
pub const MAX_AMP_CIPHERTEXT_BYTES: usize = 65_536;

/// Maximum size for AMP plaintext.
pub const MAX_AMP_PLAINTEXT_BYTES: usize = 65_536;

// =============================================================================
// Journal & Facts
// =============================================================================

/// Maximum size for fact payloads.
pub const MAX_FACT_PAYLOAD_BYTES: usize = 65_536;

/// Maximum size for temporal fact content.
pub const MAX_TEMPORAL_FACT_CONTENT_BYTES: usize = 65_536;

/// Maximum entries in LWW map.
pub const MAX_LWW_MAP_ENTRIES_COUNT: u32 = 65_536;

/// Maximum fact operations in a journal.
pub const MAX_FACT_OPERATIONS: usize = 131_072;

/// Maximum size for relational binding data.
pub const MAX_RELATIONAL_BINDING_DATA_BYTES: usize = 65_536;

// =============================================================================
// Agent & Bloom
// =============================================================================

/// Maximum size for encrypted credentials.
pub const MAX_ENCRYPTED_CREDENTIALS_BYTES: usize = 4096;

/// Maximum size for agent payloads.
pub const MAX_AGENT_PAYLOAD_BYTES: usize = 65_536;

/// Maximum size for bloom filter bits.
pub const MAX_BLOOM_BITS_BYTES: usize = 1_048_576;

// =============================================================================
// Protocol Versioning
// =============================================================================

/// Minimum supported protocol version.
pub const MIN_SUPPORTED_PROTOCOL_VERSION: (u16, u16, u16) = (1, 0, 0);

/// Current protocol version.
pub const CURRENT_PROTOCOL_VERSION: (u16, u16, u16) = (1, 0, 0);
