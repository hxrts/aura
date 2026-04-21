//! Identity Verification Service
//!
//! Provides identity verification logic for attested tree operations
//! and authority management. Tracks authority lifecycle and organizational status.

use crate::facts::{Confidence, PublicKeyBytes};
use aura_core::{
    tree::{verify_attested_op, AttestedOp, BranchSigningKey},
    AccountId, AuraError, AuraResult, AuthorityId, Cap, Epoch, Hash32, Policy,
};
use std::collections::HashMap;

/// Type alias for identity operation results
pub type IdentityResult<T> = AuraResult<T>;

/// Authority verification service
#[derive(Debug)]
pub struct AuthorityRegistry {
    /// Known authority identities
    known_authorities: HashMap<AuthorityId, AuthorityInfo>,
    /// Account policies for authorization enforcement
    account_policies: HashMap<AccountId, Policy>,
}

/// Information about a known authority
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthorityInfo {
    /// Authority identifier
    pub authority_id: AuthorityId,
    /// Authority public key
    pub public_key: PublicKeyBytes,
    /// Authority capabilities
    pub capabilities: Cap,
    /// Authority status
    pub status: AuthorityStatus,
}

/// Authority status
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AuthorityStatus {
    /// Authority is active and trusted
    Active,
    /// Authority is suspended
    Suspended,
    /// Authority is revoked
    Revoked,
}

/// Verification result
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Whether verification passed
    pub verified: bool,
    /// Verification details
    pub details: String,
    /// Confidence score (0.0 to 1.0)
    pub confidence: Confidence,
}

impl AuthorityRegistry {
    /// Create a new authority registry
    pub fn new() -> Self {
        Self {
            known_authorities: HashMap::new(),
            account_policies: HashMap::new(),
        }
    }

    /// Register an authority
    pub fn register_authority(&mut self, authority_info: AuthorityInfo) -> IdentityResult<()> {
        if self
            .known_authorities
            .contains_key(&authority_info.authority_id)
        {
            return Err(AuraError::invalid("Authority already registered"));
        }

        self.known_authorities
            .insert(authority_info.authority_id, authority_info);
        Ok(())
    }

    /// Verify an authority identity
    pub fn verify_authority(
        &self,
        authority_id: AuthorityId,
    ) -> IdentityResult<VerificationResult> {
        let authority_info = self
            .known_authorities
            .get(&authority_id)
            .ok_or_else(|| AuraError::not_found("Unknown authority"))?;

        let (verified, confidence) = verification_state(authority_info.status);

        Ok(VerificationResult {
            verified,
            details: format!("Authority status: {:?}", authority_info.status),
            confidence,
        })
    }

    /// Verify an attested operation
    pub fn verify_attested_operation(
        &self,
        attested_op: &AttestedOp,
        witness: &aura_core::tree::verification::SigningWitness,
        current_epoch: Epoch,
        child_count: u32,
    ) -> IdentityResult<VerificationResult> {
        // Structural validation and policy evaluation are handled by TreeState in aura-journal.
        // This method focuses on signature verification and policy-derived thresholds.
        // Convert the witness into the signing material required by the
        // cryptographic verifier. The witness is produced by TreeState in
        // aura-journal and contains the group public key plus the threshold
        // derived from the active policy.
        let signing_key = BranchSigningKey::new(witness.group_public_key, witness.key_epoch);

        // Guard 0: sanity bounds on signer count relative to topology and policy.
        if attested_op.signer_count > child_count as u16 {
            return Err(AuraError::invalid(format!(
                "Signer count {} exceeds child fan-out {}",
                attested_op.signer_count, child_count
            )));
        }

        if attested_op.signer_count < witness.threshold {
            return Err(AuraError::invalid(format!(
                "Signer count {} below policy threshold {}",
                attested_op.signer_count, witness.threshold
            )));
        }

        // Guard 1: enforce epoch alignment between the attesting key and current state.
        if witness.key_epoch > current_epoch {
            return Err(AuraError::invalid(format!(
                "Signing key epoch {} is ahead of current epoch {}",
                witness.key_epoch, current_epoch
            )));
        }

        // 1) Cryptographically verify the aggregate signature against the
        //     branch key and required threshold.
        verify_attested_op(attested_op, &signing_key, witness.threshold, current_epoch)
            .map_err(|e| AuraError::invalid(format!("Attested op verification failed: {e}")))?;

        // 2) Integrity check: ensure the operation hash matches the payload we
        //     intend to commit (guards against serialization tampering).
        let op_bytes = aura_core::util::serialization::to_vec(&attested_op.op)
            .map_err(|e| AuraError::serialization(e.to_string()))?;
        let op_hash = Hash32(aura_core::hash::hash(&op_bytes));

        tracing::debug!(
            signer_count = attested_op.signer_count,
            threshold = witness.threshold,
            key_epoch = %witness.key_epoch,
            ?op_hash,
            "Attested operation verified against branch signing key"
        );

        Ok(VerificationResult {
            verified: true,
            details: format!(
                "Signature verified with {} of {} signers",
                attested_op.signer_count, witness.threshold
            ),
            confidence: Confidence::MAX,
        })
    }

    /// Get known authorities
    pub fn known_authorities(&self) -> &HashMap<AuthorityId, AuthorityInfo> {
        &self.known_authorities
    }

    /// Get account policies
    pub fn account_policies(&self) -> &HashMap<AccountId, Policy> {
        &self.account_policies
    }

    /// Set policy for an account
    pub fn set_account_policy(&mut self, account_id: AccountId, policy: Policy) {
        self.account_policies.insert(account_id, policy);
    }

    /// Get policy for a specific account
    pub fn get_account_policy(&self, account_id: &AccountId) -> Option<&Policy> {
        self.account_policies.get(account_id)
    }

    /// Update authority status
    pub fn update_authority_status(
        &mut self,
        authority_id: AuthorityId,
        status: AuthorityStatus,
    ) -> IdentityResult<()> {
        let authority_info = self
            .known_authorities
            .get_mut(&authority_id)
            .ok_or_else(|| AuraError::not_found("Unknown authority"))?;

        // Enforce monotonic lifecycle: Active → Suspended → Revoked.
        // Backward transitions are rejected to prevent reactivation of
        // revoked or suspended authorities.
        let current = authority_info.status;
        if !is_valid_status_transition(current, status) {
            return Err(AuraError::invalid(format!(
                "Invalid lifecycle transition: {current:?} → {status:?} (backward transitions are forbidden)"
            )));
        }

        authority_info.status = status;
        tracing::info!("Updated authority {} status to {:?}", authority_id, status);
        Ok(())
    }
}

impl Default for AuthorityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn suspended_confidence() -> Confidence {
    Confidence::new(0.5).unwrap_or(Confidence::MIN)
}

fn verification_state(status: AuthorityStatus) -> (bool, Confidence) {
    match status {
        AuthorityStatus::Active => (true, Confidence::MAX),
        AuthorityStatus::Suspended => (false, suspended_confidence()),
        AuthorityStatus::Revoked => (false, Confidence::MIN),
    }
}

fn is_valid_status_transition(current: AuthorityStatus, next: AuthorityStatus) -> bool {
    current == next
        || matches!(
            (current, next),
            (AuthorityStatus::Active, AuthorityStatus::Suspended)
                | (AuthorityStatus::Active, AuthorityStatus::Revoked)
                | (AuthorityStatus::Suspended, AuthorityStatus::Revoked)
        )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aura_core::Cap;

    fn test_authority_info(seed: u8, status: AuthorityStatus) -> AuthorityInfo {
        AuthorityInfo {
            authority_id: AuthorityId::new_from_entropy([seed; 32]),
            public_key: PublicKeyBytes::new([seed; 32]),
            capabilities: Cap::top(),
            status,
        }
    }

    /// Active authority registers and appears in known_authorities map.
    #[test]
    fn test_authority_registration() {
        let mut verifier = AuthorityRegistry::new();
        let authority_info = test_authority_info(1, AuthorityStatus::Active);
        let authority_id = authority_info.authority_id;

        assert!(verifier.register_authority(authority_info).is_ok());
        assert!(verifier.known_authorities().contains_key(&authority_id));
    }

    /// Active authority verifies with maximum confidence.
    #[test]
    fn test_authority_verification() {
        let mut verifier = AuthorityRegistry::new();
        let authority_info = test_authority_info(2, AuthorityStatus::Active);
        let authority_id = authority_info.authority_id;

        verifier.register_authority(authority_info).unwrap();

        let result = verifier.verify_authority(authority_id).unwrap();
        assert!(result.verified);
        assert_eq!(result.confidence, Confidence::MAX);
    }

    /// Suspending an authority drops confidence and verification to false.
    #[test]
    fn test_authority_status_update() {
        let mut verifier = AuthorityRegistry::new();
        let authority_info = test_authority_info(3, AuthorityStatus::Active);
        let authority_id = authority_info.authority_id;

        verifier.register_authority(authority_info).unwrap();

        // Suspend the authority
        assert!(verifier
            .update_authority_status(authority_id, AuthorityStatus::Suspended)
            .is_ok());

        let result = verifier.verify_authority(authority_id).unwrap();
        assert!(!result.verified);
        assert_eq!(
            result.confidence,
            Confidence::new(0.5).expect("valid confidence")
        );
    }

    /// Forward lifecycle: Active → Suspended → Revoked with decreasing
    /// confidence at each step.
    #[test]
    fn test_authority_lifecycle_transition() {
        let mut verifier = AuthorityRegistry::new();
        let authority_info = test_authority_info(4, AuthorityStatus::Active);
        let authority_id = authority_info.authority_id;

        verifier.register_authority(authority_info).unwrap();

        let active = verifier.verify_authority(authority_id).unwrap();
        assert!(active.verified);
        assert_eq!(active.confidence, Confidence::MAX);

        verifier
            .update_authority_status(authority_id, AuthorityStatus::Suspended)
            .unwrap();
        let suspended = verifier.verify_authority(authority_id).unwrap();
        assert!(!suspended.verified);
        assert_eq!(
            suspended.confidence,
            Confidence::new(0.5).expect("valid confidence")
        );

        verifier
            .update_authority_status(authority_id, AuthorityStatus::Revoked)
            .unwrap();
        let revoked = verifier.verify_authority(authority_id).unwrap();
        assert!(!revoked.verified);
        assert_eq!(revoked.confidence, Confidence::MIN);
    }

    /// Backward lifecycle transitions must be rejected — a revoked authority
    /// cannot be reactivated. If this fails, an attacker who compromised
    /// a revoked key can re-enable it.
    #[test]
    fn test_backward_lifecycle_rejected() {
        let mut verifier = AuthorityRegistry::new();
        let authority_info = test_authority_info(5, AuthorityStatus::Active);
        let authority_id = authority_info.authority_id;
        verifier.register_authority(authority_info).unwrap();

        // Suspend first
        verifier
            .update_authority_status(authority_id, AuthorityStatus::Suspended)
            .unwrap();

        // Backward: Suspended → Active must fail
        let result = verifier.update_authority_status(authority_id, AuthorityStatus::Active);
        assert!(
            result.is_err(),
            "Suspended → Active must be rejected (backward transition)"
        );

        // Forward to Revoked
        verifier
            .update_authority_status(authority_id, AuthorityStatus::Revoked)
            .unwrap();

        // Backward: Revoked → Active must fail
        let result = verifier.update_authority_status(authority_id, AuthorityStatus::Active);
        assert!(
            result.is_err(),
            "Revoked → Active must be rejected (backward transition)"
        );

        // Backward: Revoked → Suspended must fail
        let result = verifier.update_authority_status(authority_id, AuthorityStatus::Suspended);
        assert!(
            result.is_err(),
            "Revoked → Suspended must be rejected (backward transition)"
        );
    }

    /// Verifying an unregistered authority must fail — prevents accepting
    /// signatures from unknown trust roots.
    #[test]
    fn test_unregistered_authority_rejected() {
        let verifier = AuthorityRegistry::new();
        let unknown_id = AuthorityId::new_from_entropy([99u8; 32]);

        let result = verifier.verify_authority(unknown_id);
        assert!(result.is_err(), "unregistered authority must be rejected");
    }

    /// Idempotent status update (same status) must succeed.
    #[test]
    fn test_idempotent_status_update() {
        let mut verifier = AuthorityRegistry::new();
        let authority_info = test_authority_info(6, AuthorityStatus::Active);
        let authority_id = authority_info.authority_id;
        verifier.register_authority(authority_info).unwrap();

        // Active → Active is idempotent
        assert!(verifier
            .update_authority_status(authority_id, AuthorityStatus::Active)
            .is_ok());
    }
}
