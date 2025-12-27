//! Shared storage helpers for commitment tree operations.
//!
//! These helpers define the canonical storage keys and (de)serialization
//! routines used by both tree and sync handlers.

use aura_core::hash;
use aura_core::tree::AttestedOp;
use aura_core::AuraError;

/// Storage key prefix for tree operations.
pub const TREE_OPS_PREFIX: &str = "tree_ops/";
/// Storage key for the operation index.
pub const TREE_OPS_INDEX_KEY: &str = "tree_ops_index";

/// Compute the storage key for a specific operation hash.
pub fn op_key(op_hash: [u8; 32]) -> String {
    format!("{}{}", TREE_OPS_PREFIX, hex::encode(op_hash))
}

/// Serialize an attested operation for storage using DAG-CBOR.
pub fn serialize_op(op: &AttestedOp) -> Result<Vec<u8>, AuraError> {
    aura_core::util::serialization::to_vec(op)
        .map_err(|e| AuraError::internal(format!("Failed to serialize tree op: {e}")))
}

/// Deserialize an attested operation from storage bytes.
pub fn deserialize_op(bytes: &[u8]) -> Result<AttestedOp, AuraError> {
    aura_core::util::serialization::from_slice(bytes)
        .map_err(|e| AuraError::internal(format!("Failed to deserialize tree op: {e}")))
}

/// Serialize an ordered list of operation hashes for storage using DAG-CBOR.
pub fn serialize_op_index(hashes: &[[u8; 32]]) -> Result<Vec<u8>, AuraError> {
    // Convert slice to Vec for serialization (DAG-CBOR requires Sized)
    let hashes_vec: Vec<[u8; 32]> = hashes.to_vec();
    aura_core::util::serialization::to_vec(&hashes_vec)
        .map_err(|e| AuraError::internal(format!("Failed to serialize ops index: {e}")))
}

/// Deserialize an ordered list of operation hashes from storage bytes.
pub fn deserialize_op_index(bytes: &[u8]) -> Result<Vec<[u8; 32]>, AuraError> {
    aura_core::util::serialization::from_slice(bytes)
        .map_err(|e| AuraError::internal(format!("Failed to deserialize ops index: {e}")))
}

/// Compute hash for an operation (for deduplication and CID).
pub fn op_hash(op: &AttestedOp) -> Result<[u8; 32], AuraError> {
    let bytes = aura_core::util::serialization::to_vec(op)
        .map_err(|e| AuraError::internal(format!("hash serialize attested op: {e}")))?;
    Ok(hash::hash(&bytes))
}
