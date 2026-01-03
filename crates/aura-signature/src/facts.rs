//! Verification Domain Facts
//!
//! Pure fact types for identity verification state changes.
//! These facts are defined here (Layer 2) and committed by higher layers.
//!
//! **Authority Model**: Facts reference authorities using the
//! authority-centric model where authorities hide internal device structure.

use aura_core::time::PhysicalTime;
use aura_core::types::facts::{FactDelta, FactDeltaReducer, FactError, FactTypeId};
use aura_core::types::Epoch;
use aura_core::AuthorityId;
use aura_core::{AccountId, Cap};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique type identifier for verification facts
pub static VERIFY_FACT_TYPE_ID: FactTypeId = FactTypeId::new("verify/v1");
/// Schema version for verification fact encoding
pub const VERIFY_FACT_SCHEMA_VERSION: u16 = 2;

/// Get the typed fact ID for verification facts
pub fn verify_fact_type_id() -> &'static FactTypeId {
    &VERIFY_FACT_TYPE_ID
}

/// Validated Ed25519 public key bytes (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PublicKeyBytes([u8; 32]);

impl PublicKeyBytes {
    /// Length in bytes for Ed25519 public keys.
    pub const LENGTH: usize = 32;

    /// Create from a fixed-size array.
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Attempt to create from a byte slice.
    pub fn try_from_slice(bytes: &[u8]) -> Result<Self, PublicKeyBytesError> {
        if bytes.len() != Self::LENGTH {
            return Err(PublicKeyBytesError {
                actual: bytes.len() as u64,
            });
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(Self(arr))
    }

    /// Access the underlying bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for PublicKeyBytes {
    type Error = PublicKeyBytesError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from_slice(&value)
    }
}

impl From<PublicKeyBytes> for Vec<u8> {
    fn from(value: PublicKeyBytes) -> Self {
        value.0.to_vec()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKeyBytesError {
    actual: u64,
}

impl fmt::Display for PublicKeyBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "public key must be {} bytes, got {}",
            PublicKeyBytes::LENGTH,
            self.actual
        )
    }
}

impl std::error::Error for PublicKeyBytesError {}

/// Confidence score between 0.0 and 1.0 (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Confidence(f64);

impl Confidence {
    /// Maximum confidence (1.0).
    pub const MAX: Confidence = Confidence(1.0);
    /// Minimum confidence (0.0).
    pub const MIN: Confidence = Confidence(0.0);

    /// Create a validated confidence score.
    pub fn new(value: f64) -> Result<Self, ConfidenceError> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ConfidenceError { value })
        }
    }

    /// Access the underlying value.
    pub fn value(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Confidence {
    type Error = ConfidenceError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Confidence::new(value)
    }
}

impl From<Confidence> for f64 {
    fn from(value: Confidence) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfidenceError {
    value: f64,
}

impl fmt::Display for ConfidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "confidence must be between 0.0 and 1.0, got {}",
            self.value
        )
    }
}

impl std::error::Error for ConfidenceError {}

/// Verification domain facts for identity state changes.
///
/// These facts capture authority lifecycle events and are used by the
/// journal system to derive authority registry state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum VerifyFact {
    /// Authority registered
    AuthorityRegistered {
        /// Authority being registered
        authority_id: AuthorityId,
        /// Authority public key (serialized)
        public_key: PublicKeyBytes,
        /// Initial capabilities granted
        capabilities: Cap,
        /// Epoch when authority was registered
        registered_epoch: Epoch,
        /// Timestamp when authority was registered (uses unified time system)
        registered_at: PhysicalTime,
    },

    /// Authority suspended (temporarily disabled)
    AuthoritySuspended {
        /// Authority being suspended
        authority_id: AuthorityId,
        /// Reason for suspension
        reason: RevocationReason,
        /// Epoch when authority was suspended
        suspended_epoch: Epoch,
        /// Timestamp when authority was suspended (uses unified time system)
        suspended_at: PhysicalTime,
    },

    /// Authority revoked (permanently disabled)
    AuthorityRevoked {
        /// Authority being revoked
        authority_id: AuthorityId,
        /// Reason for revocation
        reason: RevocationReason,
        /// Epoch when authority was revoked
        revoked_epoch: Epoch,
        /// Timestamp when authority was revoked (uses unified time system)
        revoked_at: PhysicalTime,
    },

    /// Authority reactivated after suspension
    AuthorityReactivated {
        /// Authority being reactivated
        authority_id: AuthorityId,
        /// Epoch when authority was reactivated
        reactivated_epoch: Epoch,
        /// Timestamp when authority was reactivated (uses unified time system)
        reactivated_at: PhysicalTime,
    },

    /// Authority capabilities updated
    AuthorityCapabilitiesUpdated {
        /// Authority being updated
        authority_id: AuthorityId,
        /// New capabilities
        new_capabilities: Cap,
        /// Epoch when authority capabilities were updated
        updated_epoch: Epoch,
        /// Timestamp when authority capabilities were updated (uses unified time system)
        updated_at: PhysicalTime,
    },

    /// Account policy set or updated
    AccountPolicySet {
        /// Account ID
        account_id: AccountId,
        /// Policy threshold (minimum signers)
        threshold: u16,
        /// Epoch when policy was set
        set_epoch: Epoch,
        /// Timestamp when policy was set (uses unified time system)
        set_at: PhysicalTime,
    },

    /// Identity verification performed
    IdentityVerified {
        /// Authority that was verified
        authority_id: AuthorityId,
        /// Type of verification performed
        verification_type: VerificationType,
        /// Whether verification succeeded
        success: bool,
        /// Confidence score (0.0 to 1.0)
        confidence: Confidence,
        /// Timestamp when verification occurred (uses unified time system)
        verified_at: PhysicalTime,
    },

    /// Threshold signature verified
    ThresholdSignatureVerified {
        /// Account whose threshold signature was verified
        account_id: AccountId,
        /// Number of signers that participated
        signer_count: u16,
        /// Required threshold
        threshold: u16,
        /// Hash of the message that was signed
        message_hash: [u8; 32],
        /// Whether verification succeeded
        success: bool,
        /// Timestamp when verification occurred (uses unified time system)
        verified_at: PhysicalTime,
    },
}

/// Type of identity verification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationType {
    /// Guardian signature verification
    Guardian,
    /// Authority signature verification
    Authority,
    /// Threshold signature verification
    Threshold,
}

/// Closed set of lifecycle revocation/suspension reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevocationReason {
    /// Policy or terms violation.
    PolicyViolation,
    /// Cryptographic key compromise detected.
    CompromiseDetected,
    /// User-initiated revocation.
    UserRequested,
    /// Administrative action.
    Administrative,
    /// Credential or delegation expired.
    Expired,
    /// Reason could not be determined.
    Unknown,
}

impl VerifyFact {
    /// Get the authority ID associated with this fact, if applicable
    pub fn authority_id(&self) -> Option<AuthorityId> {
        match self {
            VerifyFact::AuthorityRegistered { authority_id, .. } => Some(*authority_id),
            VerifyFact::AuthoritySuspended { authority_id, .. } => Some(*authority_id),
            VerifyFact::AuthorityRevoked { authority_id, .. } => Some(*authority_id),
            VerifyFact::AuthorityReactivated { authority_id, .. } => Some(*authority_id),
            VerifyFact::AuthorityCapabilitiesUpdated { authority_id, .. } => Some(*authority_id),
            VerifyFact::AccountPolicySet { .. } => None,
            VerifyFact::IdentityVerified { authority_id, .. } => Some(*authority_id),
            VerifyFact::ThresholdSignatureVerified { .. } => None,
        }
    }

    /// Get the timestamp for this fact in milliseconds
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            VerifyFact::AuthorityRegistered { registered_at, .. } => registered_at.ts_ms,
            VerifyFact::AuthoritySuspended { suspended_at, .. } => suspended_at.ts_ms,
            VerifyFact::AuthorityRevoked { revoked_at, .. } => revoked_at.ts_ms,
            VerifyFact::AuthorityReactivated { reactivated_at, .. } => reactivated_at.ts_ms,
            VerifyFact::AuthorityCapabilitiesUpdated { updated_at, .. } => updated_at.ts_ms,
            VerifyFact::AccountPolicySet { set_at, .. } => set_at.ts_ms,
            VerifyFact::IdentityVerified { verified_at, .. } => verified_at.ts_ms,
            VerifyFact::ThresholdSignatureVerified { verified_at, .. } => verified_at.ts_ms,
        }
    }

    /// Encode this fact with a canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn try_encode(&self) -> Result<Vec<u8>, FactError> {
        aura_core::types::facts::try_encode_fact(
            verify_fact_type_id(),
            VERIFY_FACT_SCHEMA_VERSION,
            self,
        )
    }

    /// Decode a fact from a canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if deserialization fails or version/type mismatches.
    pub fn try_decode(bytes: &[u8]) -> Result<Self, FactError> {
        aura_core::types::facts::try_decode_fact(
            verify_fact_type_id(),
            VERIFY_FACT_SCHEMA_VERSION,
            bytes,
        )
    }

    /// Encode this fact with proper error handling.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, FactError> {
        self.try_encode()
    }

    /// Decode a fact with proper error handling.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if deserialization fails or version/type mismatches.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FactError> {
        Self::try_decode(bytes)
    }

    /// Create an AuthorityRegistered fact with millisecond timestamp
    pub fn authority_registered_ms(
        authority_id: AuthorityId,
        public_key: PublicKeyBytes,
        capabilities: Cap,
        registered_epoch: Epoch,
        registered_at_ms: u64,
    ) -> Self {
        Self::AuthorityRegistered {
            authority_id,
            public_key,
            capabilities,
            registered_epoch,
            registered_at: PhysicalTime {
                ts_ms: registered_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AuthoritySuspended fact with millisecond timestamp
    pub fn authority_suspended_ms(
        authority_id: AuthorityId,
        reason: RevocationReason,
        suspended_epoch: Epoch,
        suspended_at_ms: u64,
    ) -> Self {
        Self::AuthoritySuspended {
            authority_id,
            reason,
            suspended_epoch,
            suspended_at: PhysicalTime {
                ts_ms: suspended_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AuthorityRevoked fact with millisecond timestamp
    pub fn authority_revoked_ms(
        authority_id: AuthorityId,
        reason: RevocationReason,
        revoked_epoch: Epoch,
        revoked_at_ms: u64,
    ) -> Self {
        Self::AuthorityRevoked {
            authority_id,
            reason,
            revoked_epoch,
            revoked_at: PhysicalTime {
                ts_ms: revoked_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AuthorityReactivated fact with millisecond timestamp
    pub fn authority_reactivated_ms(
        authority_id: AuthorityId,
        reactivated_epoch: Epoch,
        reactivated_at_ms: u64,
    ) -> Self {
        Self::AuthorityReactivated {
            authority_id,
            reactivated_epoch,
            reactivated_at: PhysicalTime {
                ts_ms: reactivated_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AuthorityCapabilitiesUpdated fact with millisecond timestamp
    pub fn authority_capabilities_updated_ms(
        authority_id: AuthorityId,
        new_capabilities: Cap,
        updated_epoch: Epoch,
        updated_at_ms: u64,
    ) -> Self {
        Self::AuthorityCapabilitiesUpdated {
            authority_id,
            new_capabilities,
            updated_epoch,
            updated_at: PhysicalTime {
                ts_ms: updated_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AccountPolicySet fact with millisecond timestamp
    pub fn account_policy_set_ms(
        account_id: AccountId,
        threshold: u16,
        set_epoch: Epoch,
        set_at_ms: u64,
    ) -> Self {
        Self::AccountPolicySet {
            account_id,
            threshold,
            set_epoch,
            set_at: PhysicalTime {
                ts_ms: set_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an IdentityVerified fact with millisecond timestamp
    pub fn identity_verified_ms(
        authority_id: AuthorityId,
        verification_type: VerificationType,
        success: bool,
        confidence: Confidence,
        verified_at_ms: u64,
    ) -> Self {
        Self::IdentityVerified {
            authority_id,
            verification_type,
            success,
            confidence,
            verified_at: PhysicalTime {
                ts_ms: verified_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a ThresholdSignatureVerified fact with millisecond timestamp
    pub fn threshold_signature_verified_ms(
        account_id: AccountId,
        signer_count: u16,
        threshold: u16,
        message_hash: [u8; 32],
        success: bool,
        verified_at_ms: u64,
    ) -> Self {
        Self::ThresholdSignatureVerified {
            account_id,
            signer_count,
            threshold,
            message_hash,
            success,
            verified_at: PhysicalTime {
                ts_ms: verified_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Get the epoch for this fact
    pub fn epoch(&self) -> Option<Epoch> {
        match self {
            VerifyFact::AuthorityRegistered {
                registered_epoch, ..
            } => Some(*registered_epoch),
            VerifyFact::AuthoritySuspended {
                suspended_epoch, ..
            } => Some(*suspended_epoch),
            VerifyFact::AuthorityRevoked { revoked_epoch, .. } => Some(*revoked_epoch),
            VerifyFact::AuthorityReactivated {
                reactivated_epoch, ..
            } => Some(*reactivated_epoch),
            VerifyFact::AuthorityCapabilitiesUpdated { updated_epoch, .. } => Some(*updated_epoch),
            VerifyFact::AccountPolicySet { set_epoch, .. } => Some(*set_epoch),
            VerifyFact::IdentityVerified { .. } => None,
            VerifyFact::ThresholdSignatureVerified { .. } => None,
        }
    }

    /// Get the fact type name for journal keying
    pub fn fact_type(&self) -> &'static str {
        match self {
            VerifyFact::AuthorityRegistered { .. } => "authority_registered",
            VerifyFact::AuthoritySuspended { .. } => "authority_suspended",
            VerifyFact::AuthorityRevoked { .. } => "authority_revoked",
            VerifyFact::AuthorityReactivated { .. } => "authority_reactivated",
            VerifyFact::AuthorityCapabilitiesUpdated { .. } => "authority_capabilities_updated",
            VerifyFact::AccountPolicySet { .. } => "account_policy_set",
            VerifyFact::IdentityVerified { .. } => "identity_verified",
            VerifyFact::ThresholdSignatureVerified { .. } => "threshold_signature_verified",
        }
    }
}

/// Delta type for verification fact application
#[derive(Debug, Clone, Default)]
pub struct VerifyFactDelta {
    /// Authorities registered in this delta
    pub authorities_registered: Vec<AuthorityId>,
    /// Authorities suspended in this delta
    pub authorities_suspended: Vec<AuthorityId>,
    /// Authorities revoked in this delta
    pub authorities_revoked: Vec<AuthorityId>,
    /// Authorities reactivated in this delta
    pub authorities_reactivated: Vec<AuthorityId>,
    /// Verifications performed in this delta
    pub verifications_performed: u64,
    /// Successful verifications in this delta
    pub verifications_successful: u64,
}

impl FactDelta for VerifyFactDelta {
    fn merge(&mut self, other: &Self) {
        self.authorities_registered
            .extend(other.authorities_registered.iter().cloned());
        self.authorities_suspended
            .extend(other.authorities_suspended.iter().cloned());
        self.authorities_revoked
            .extend(other.authorities_revoked.iter().cloned());
        self.authorities_reactivated
            .extend(other.authorities_reactivated.iter().cloned());
        self.verifications_performed += other.verifications_performed;
        self.verifications_successful += other.verifications_successful;
    }
}

/// Reducer for verification facts
#[derive(Debug, Clone, Default)]
pub struct VerifyFactReducer;

impl VerifyFactReducer {
    /// Create a new verification fact reducer
    pub fn new() -> Self {
        Self
    }
}

impl FactDeltaReducer<VerifyFact, VerifyFactDelta> for VerifyFactReducer {
    fn apply(&self, fact: &VerifyFact) -> VerifyFactDelta {
        let mut delta = VerifyFactDelta::default();

        match fact {
            VerifyFact::AuthorityRegistered { authority_id, .. } => {
                delta.authorities_registered.push(*authority_id);
            }
            VerifyFact::AuthoritySuspended { authority_id, .. } => {
                delta.authorities_suspended.push(*authority_id);
            }
            VerifyFact::AuthorityRevoked { authority_id, .. } => {
                delta.authorities_revoked.push(*authority_id);
            }
            VerifyFact::AuthorityReactivated { authority_id, .. } => {
                delta.authorities_reactivated.push(*authority_id);
            }
            VerifyFact::IdentityVerified { success, .. } => {
                delta.verifications_performed += 1;
                if *success {
                    delta.verifications_successful += 1;
                }
            }
            VerifyFact::ThresholdSignatureVerified { success, .. } => {
                delta.verifications_performed += 1;
                if *success {
                    delta.verifications_successful += 1;
                }
            }
            VerifyFact::AuthorityCapabilitiesUpdated { .. }
            | VerifyFact::AccountPolicySet { .. } => {
                // These don't produce cumulative deltas for authority lifecycle
            }
        }

        delta
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aura_core::types::facts::FactDeltaReducer;

    #[test]
    fn test_verify_fact_authority_id() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);

        let fact = VerifyFact::authority_registered_ms(
            authority_id,
            PublicKeyBytes::new([1u8; 32]),
            Cap::top(),
            Epoch::new(1),
            1000,
        );

        assert_eq!(fact.authority_id(), Some(authority_id));
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.epoch(), Some(Epoch::new(1)));
        assert_eq!(fact.fact_type(), "authority_registered");
    }

    #[test]
    fn test_verify_fact_reducer() {
        let reducer = VerifyFactReducer::new();
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);

        let fact = VerifyFact::authority_registered_ms(
            authority_id,
            PublicKeyBytes::new([2u8; 32]),
            Cap::top(),
            Epoch::new(1),
            1000,
        );

        let delta = reducer.apply(&fact);
        assert_eq!(delta.authorities_registered.len(), 1);
        assert_eq!(delta.authorities_registered[0], authority_id);
    }

    #[test]
    fn test_verification_fact() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);

        let fact = VerifyFact::identity_verified_ms(
            authority_id,
            VerificationType::Authority,
            true,
            Confidence::new(1.0).expect("valid confidence"),
            2000,
        );

        assert_eq!(fact.authority_id(), Some(authority_id));
        assert_eq!(fact.timestamp_ms(), 2000);
        assert_eq!(fact.fact_type(), "identity_verified");

        let reducer = VerifyFactReducer::new();
        let delta = reducer.apply(&fact);
        assert_eq!(delta.verifications_performed, 1);
        assert_eq!(delta.verifications_successful, 1);
    }

    #[test]
    fn test_timestamp_ms() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);

        let fact = VerifyFact::authority_suspended_ms(
            authority_id,
            RevocationReason::Administrative,
            Epoch::new(1),
            1234567890,
        );
        assert_eq!(fact.timestamp_ms(), 1234567890);
    }

    #[test]
    fn test_public_key_bytes_validation() {
        let ok = PublicKeyBytes::try_from_slice(&[0u8; 32]);
        assert!(ok.is_ok());

        let bad = PublicKeyBytes::try_from_slice(&[0u8; 31]);
        assert!(bad.is_err());
    }

    #[test]
    fn test_confidence_validation() {
        assert!(Confidence::new(0.0).is_ok());
        assert!(Confidence::new(1.0).is_ok());
        assert!(Confidence::new(-0.1).is_err());
        assert!(Confidence::new(1.1).is_err());
    }
}

/// Property tests for semilattice laws on VerifyFactDelta
#[cfg(test)]
#[allow(clippy::redundant_clone)]
mod proptest_semilattice {
    use super::*;
    use aura_core::types::facts::FactDelta;
    use proptest::prelude::*;

    /// Strategy for generating arbitrary AuthorityId values
    fn arb_authority_id() -> impl Strategy<Value = AuthorityId> {
        any::<[u8; 32]>().prop_map(AuthorityId::new_from_entropy)
    }

    /// Strategy for generating arbitrary VerifyFactDelta values
    fn arb_delta() -> impl Strategy<Value = VerifyFactDelta> {
        (
            prop::collection::vec(arb_authority_id(), 0..5),
            prop::collection::vec(arb_authority_id(), 0..5),
            prop::collection::vec(arb_authority_id(), 0..5),
            prop::collection::vec(arb_authority_id(), 0..5),
            0u64..100,
            0u64..100,
        )
            .prop_map(
                |(
                    authorities_registered,
                    authorities_suspended,
                    authorities_revoked,
                    authorities_reactivated,
                    verifications_performed,
                    verifications_successful,
                )| {
                    VerifyFactDelta {
                        authorities_registered,
                        authorities_suspended,
                        authorities_revoked,
                        authorities_reactivated,
                        verifications_performed,
                        verifications_successful,
                    }
                },
            )
    }

    /// Compare deltas as multisets (order-independent for Vec fields)
    fn deltas_equivalent(a: &VerifyFactDelta, b: &VerifyFactDelta) -> bool {
        // For Vec fields, compare as sorted multisets
        let mut a_registered = a.authorities_registered.clone();
        let mut b_registered = b.authorities_registered.clone();
        a_registered.sort();
        b_registered.sort();

        let mut a_suspended = a.authorities_suspended.clone();
        let mut b_suspended = b.authorities_suspended.clone();
        a_suspended.sort();
        b_suspended.sort();

        let mut a_revoked = a.authorities_revoked.clone();
        let mut b_revoked = b.authorities_revoked.clone();
        a_revoked.sort();
        b_revoked.sort();

        let mut a_reactivated = a.authorities_reactivated.clone();
        let mut b_reactivated = b.authorities_reactivated.clone();
        a_reactivated.sort();
        b_reactivated.sort();

        a_registered == b_registered
            && a_suspended == b_suspended
            && a_revoked == b_revoked
            && a_reactivated == b_reactivated
            && a.verifications_performed == b.verifications_performed
            && a.verifications_successful == b.verifications_successful
    }

    proptest! {
        /// Commutativity: a.merge(&b) == b.merge(&a) (multiset equivalence)
        #[test]
        fn merge_commutative(a in arb_delta(), b in arb_delta()) {
            let mut ab = a.clone();
            ab.merge(&b);

            let mut ba = b.clone();
            ba.merge(&a);

            prop_assert!(
                deltas_equivalent(&ab, &ba),
                "merge should be commutative (as multisets)"
            );
        }

        /// Associativity: (a.merge(&b)).merge(&c) == a.merge(&(b.merge(&c)))
        #[test]
        fn merge_associative(a in arb_delta(), b in arb_delta(), c in arb_delta()) {
            // Left associative: (a merge b) merge c
            let mut left = a.clone();
            left.merge(&b);
            left.merge(&c);

            // Right associative: a merge (b merge c)
            let mut bc = b.clone();
            bc.merge(&c);
            let mut right = a.clone();
            right.merge(&bc);

            prop_assert!(
                deltas_equivalent(&left, &right),
                "merge should be associative (as multisets)"
            );
        }

        /// Identity: merge with default (empty) leaves value unchanged
        #[test]
        fn merge_identity(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&VerifyFactDelta::default());

            prop_assert!(
                deltas_equivalent(&result, &original),
                "merge with identity should preserve value"
            );
        }
    }
}
