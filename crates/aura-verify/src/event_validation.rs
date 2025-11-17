//! Identity verification functions for pure authentication
//!
//! This module provides basic signature verification functions for identity proofs.
//! No authorization logic - pure cryptographic verification only.

use crate::{AuthenticationError, Result, ThresholdSig};
use aura_core::Ed25519Signature;
use aura_core::{DeviceId, GuardianId};

/// Identity verification functions
pub struct IdentityValidator;

impl IdentityValidator {
    /// Validate device signature on an event
    pub fn validate_device_signature(
        _device_id: DeviceId,
        signature: &Ed25519Signature,
        event_hash: &[u8],
        device_public_key: &aura_core::Ed25519VerifyingKey,
    ) -> Result<()> {
        // Verify signature
        aura_core::ed25519_verify(device_public_key, event_hash, signature).map_err(|e| {
            AuthenticationError::InvalidDeviceSignature(format!(
                "Device signature verification failed: {}",
                e
            ))
        })?;

        Ok(())
    }

    /// Validate guardian signature on an event
    pub fn validate_guardian_signature(
        guardian_id: GuardianId,
        signature: &Ed25519Signature,
        message: &[u8],
        guardian_public_key: &aura_core::Ed25519VerifyingKey,
    ) -> Result<()> {
        // Verify the actual signature provided with the event
        aura_core::ed25519_verify(guardian_public_key, message, signature).map_err(|e| {
            AuthenticationError::InvalidGuardianSignature(format!(
                "Guardian signature verification failed for {:?}: {}",
                guardian_id, e
            ))
        })?;

        Ok(())
    }

    /// Validate threshold signature on an event
    pub fn validate_threshold_signature(
        threshold_sig: &ThresholdSig,
        event_hash: &[u8],
        group_public_key: &aura_core::Ed25519VerifyingKey,
        required_threshold: u16,
    ) -> Result<()> {
        // Check we have enough signers
        if threshold_sig.signers.len() < required_threshold as usize {
            return Err(AuthenticationError::InvalidThresholdSignature(format!(
                "Threshold not met: current {} < required {}",
                threshold_sig.signers.len(),
                required_threshold
            )));
        }

        // Verify signer indices are valid and unique
        Self::validate_signer_indices(&threshold_sig.signers)?;

        // Verify signature against group public key using FROST verification
        Self::verify_frost_signature(event_hash, threshold_sig, group_public_key)?;

        Ok(())
    }

    /// Validate that signer indices are valid and unique
    fn validate_signer_indices(signers: &[u8]) -> Result<()> {
        // Check for duplicates
        let mut sorted_signers = signers.to_vec();
        sorted_signers.sort_unstable();
        if sorted_signers.windows(2).any(|w| w[0] == w[1]) {
            return Err(AuthenticationError::InvalidThresholdSignature(
                "Duplicate signer indices in threshold signature".to_string(),
            ));
        }

        Ok(())
    }

    /// Verify FROST threshold signature
    fn verify_frost_signature(
        message: &[u8],
        threshold_sig: &ThresholdSig,
        group_public_key: &aura_core::Ed25519VerifyingKey,
    ) -> Result<()> {
        // FROST signatures are compatible with standard Ed25519 verification
        aura_core::ed25519_verify(group_public_key, message, &threshold_sig.signature).map_err(
            |e| {
                AuthenticationError::InvalidThresholdSignature(format!(
                    "FROST threshold signature verification failed: {}",
                    e
                ))
            },
        )?;

        Ok(())
    }
}

/// Convenience function for validating device signatures
pub fn validate_device_signature(
    device_id: DeviceId,
    signature: &Ed25519Signature,
    event_hash: &[u8],
    device_public_key: &aura_core::Ed25519VerifyingKey,
) -> Result<()> {
    IdentityValidator::validate_device_signature(
        device_id,
        signature,
        event_hash,
        device_public_key,
    )
}

/// Convenience function for validating guardian signatures
pub fn validate_guardian_signature(
    guardian_id: GuardianId,
    signature: &Ed25519Signature,
    message: &[u8],
    guardian_public_key: &aura_core::Ed25519VerifyingKey,
) -> Result<()> {
    IdentityValidator::validate_guardian_signature(
        guardian_id,
        signature,
        message,
        guardian_public_key,
    )
}

/// Convenience function for validating threshold signatures
pub fn validate_threshold_signature(
    threshold_sig: &ThresholdSig,
    event_hash: &[u8],
    group_public_key: &aura_core::Ed25519VerifyingKey,
    required_threshold: u16,
) -> Result<()> {
    IdentityValidator::validate_threshold_signature(
        threshold_sig,
        event_hash,
        group_public_key,
        required_threshold,
    )
}
