//! Thin adapter over core FROST primitives for consensus.
//!
//! Keeps consensus decoupled from `frost_ed25519` and any future
//! choreography crates. All crypto lives in `aura-core::crypto::tree_signing`.

use aura_core::crypto::tree_signing::frost_verify_aggregate;
use aura_core::frost::PublicKeyPackage;
use aura_core::{AuraError, Result};

/// Interface for verifying aggregated threshold signatures.
pub trait ConsensusFrost {
    /// Verify an aggregated threshold signature over `message` using the group key.
    fn verify_aggregate(
        &self,
        group_public_key: &PublicKeyPackage,
        message: &[u8],
        signature: &[u8],
    ) -> Result<()>;
}

/// Default adapter backed by core tree-signing helpers.
pub struct CoreFrostAdapter;

impl ConsensusFrost for CoreFrostAdapter {
    fn verify_aggregate(
        &self,
        group_public_key: &PublicKeyPackage,
        message: &[u8],
        signature: &[u8],
    ) -> Result<()> {
        let frost_pkg: frost_ed25519::keys::PublicKeyPackage = group_public_key
            .clone()
            .try_into()
            .map_err(|e| AuraError::crypto(format!("invalid group public key: {e}")))?;
        let verifying_key = frost_pkg.verifying_key();
        frost_verify_aggregate(verifying_key, message, signature)
            .map_err(|e| AuraError::crypto(format!("threshold verify failed: {e}")))
    }
}

/// Utility: guard signature length before calling crypto.
pub fn validate_signature_bytes(sig: &[u8]) -> Result<()> {
    if sig.len() != 64 {
        return Err(AuraError::crypto(format!(
            "invalid signature length: {} (expected 64)",
            sig.len()
        )));
    }
    Ok(())
}
