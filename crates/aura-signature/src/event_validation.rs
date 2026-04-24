//! Identity verification functions for pure authentication
//!
//! This module provides basic signature verification functions for identity proofs.
//! No authorization logic - pure cryptographic verification only.

use crate::authority;
use crate::guardian;
use crate::threshold;
use crate::{Result, ThresholdGroupKey, ThresholdSig};
use aura_core::Ed25519Signature;
use aura_core::{AuthorityId, GuardianId};

/// Identity verification functions
pub struct IdentityValidator;

impl IdentityValidator {
    /// Validate authority signature on an event
    pub fn validate_authority_signature(
        authority_id: AuthorityId,
        signature: &Ed25519Signature,
        event_hash: &[u8],
        authority_public_key: &aura_core::Ed25519VerifyingKey,
    ) -> Result<()> {
        authority::verify_authority_signature(
            authority_id,
            event_hash,
            signature,
            authority_public_key,
        )
    }

    /// Validate guardian signature on an event
    pub fn validate_guardian_signature(
        guardian_id: GuardianId,
        signature: &Ed25519Signature,
        message: &[u8],
        guardian_public_key: &aura_core::Ed25519VerifyingKey,
    ) -> Result<()> {
        guardian::verify_guardian_signature(guardian_id, message, signature, guardian_public_key)
    }

    /// Validate threshold signature on an event
    pub fn validate_threshold_signature(
        threshold_sig: &ThresholdSig,
        event_hash: &[u8],
        group_public_key: &ThresholdGroupKey,
        required_threshold: u16,
    ) -> Result<()> {
        threshold::verify_threshold_signature(
            event_hash,
            threshold_sig,
            group_public_key,
            required_threshold as usize,
        )
    }
}

/// Convenience function for validating authority signatures
pub fn validate_authority_signature(
    authority_id: AuthorityId,
    signature: &Ed25519Signature,
    event_hash: &[u8],
    authority_public_key: &aura_core::Ed25519VerifyingKey,
) -> Result<()> {
    IdentityValidator::validate_authority_signature(
        authority_id,
        signature,
        event_hash,
        authority_public_key,
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
    group_public_key: &ThresholdGroupKey,
    required_threshold: u16,
) -> Result<()> {
    IdentityValidator::validate_threshold_signature(
        threshold_sig,
        event_hash,
        group_public_key,
        required_threshold,
    )
}
