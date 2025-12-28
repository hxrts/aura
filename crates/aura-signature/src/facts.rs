//! Verification Domain Facts
//!
//! Pure fact types for identity verification state changes.
//! These facts are defined here (Layer 2) and committed by higher layers.
//!
//! **Authority Model**: Facts reference authorities using the
//! authority-centric model where authorities hide internal device structure.

use aura_core::time::PhysicalTime;
use aura_core::types::epochs::Epoch;
use aura_core::types::facts::{FactDelta, FactDeltaReducer};
use aura_core::util::serialization::{from_slice, to_vec, SemanticVersion, VersionedMessage};
use aura_core::AuthorityId;
use aura_core::{AccountId, Cap};
use serde::{Deserialize, Serialize};

/// Unique type identifier for verification facts
pub const VERIFY_FACT_TYPE_ID: &str = "verify/v1";
/// Schema version for verification fact encoding
pub const VERIFY_FACT_SCHEMA_VERSION: u16 = 2;

/// Verification domain facts for identity state changes.
///
/// These facts capture authority lifecycle events and are used by the
/// journal system to derive authority registry state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerifyFact {
    /// Authority registered
    AuthorityRegistered {
        /// Authority being registered
        authority_id: AuthorityId,
        /// Authority public key (serialized)
        public_key: Vec<u8>,
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
        reason: String,
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
        reason: String,
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
        confidence: f64,
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

impl VerifyFact {
    fn version() -> SemanticVersion {
        SemanticVersion::new(VERIFY_FACT_SCHEMA_VERSION, 0, 0)
    }

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
    pub fn to_bytes(&self) -> Vec<u8> {
        let message = VersionedMessage::new(self.clone(), Self::version())
            .with_metadata("type".to_string(), VERIFY_FACT_TYPE_ID.to_string());
        to_vec(&message).unwrap_or_default()
    }

    /// Decode a fact from a canonical envelope.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let message: VersionedMessage<Self> = from_slice(bytes).ok()?;
        if !message.version.is_compatible(&Self::version()) {
            return None;
        }
        Some(message.payload)
    }

    /// Create an AuthorityRegistered fact with millisecond timestamp
    pub fn authority_registered_ms(
        authority_id: AuthorityId,
        public_key: Vec<u8>,
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
        reason: String,
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
        reason: String,
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
        confidence: f64,
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
mod tests {
    use super::*;
    use aura_core::types::facts::FactDeltaReducer;

    #[test]
    fn test_verify_fact_authority_id() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);

        let fact = VerifyFact::authority_registered_ms(
            authority_id,
            vec![1, 2, 3, 4],
            Cap::top(),
            Epoch(1),
            1000,
        );

        assert_eq!(fact.authority_id(), Some(authority_id));
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.epoch(), Some(Epoch(1)));
        assert_eq!(fact.fact_type(), "authority_registered");
    }

    #[test]
    fn test_verify_fact_reducer() {
        let reducer = VerifyFactReducer::new();
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);

        let fact = VerifyFact::authority_registered_ms(
            authority_id,
            vec![1, 2, 3, 4],
            Cap::top(),
            Epoch(1),
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
            1.0,
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
            "test reason".to_string(),
            Epoch(1),
            1234567890,
        );
        assert_eq!(fact.timestamp_ms(), 1234567890);
    }
}
