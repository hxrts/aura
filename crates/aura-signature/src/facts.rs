//! Verification Domain Facts
//!
//! Pure fact types for identity verification state changes.
//! These facts are defined here (Layer 2) and committed by higher layers.
//!
//! **Authority Model**: Facts reference authorities and devices using the
//! authority-centric model where authorities hide internal device structure.

use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_core::time::PhysicalTime;
use aura_core::types::epochs::Epoch;
use aura_core::{decode_domain_fact, encode_domain_fact, AccountId, Cap};
use serde::{Deserialize, Serialize};

/// Unique type identifier for verification facts
pub const VERIFY_FACT_TYPE_ID: &str = "verify/v1";
/// Schema version for verification fact encoding
pub const VERIFY_FACT_SCHEMA_VERSION: u16 = 1;

/// Verification domain facts for identity state changes.
///
/// These facts capture device lifecycle events and are used by the
/// journal system to derive device registry state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerifyFact {
    /// Device registered with an authority
    DeviceRegistered {
        /// Authority that owns this device
        authority_id: AuthorityId,
        /// Device being registered
        device_id: DeviceId,
        /// Device public key (serialized)
        public_key: Vec<u8>,
        /// Initial capabilities granted
        capabilities: Cap,
        /// Epoch when device was registered
        registered_epoch: Epoch,
        /// Timestamp when device was registered (uses unified time system)
        registered_at: PhysicalTime,
    },

    /// Device suspended (temporarily disabled)
    DeviceSuspended {
        /// Device being suspended
        device_id: DeviceId,
        /// Authority that owns this device
        authority_id: AuthorityId,
        /// Reason for suspension
        reason: String,
        /// Epoch when device was suspended
        suspended_epoch: Epoch,
        /// Timestamp when device was suspended (uses unified time system)
        suspended_at: PhysicalTime,
    },

    /// Device revoked (permanently disabled)
    DeviceRevoked {
        /// Device being revoked
        device_id: DeviceId,
        /// Authority that owns this device
        authority_id: AuthorityId,
        /// Reason for revocation
        reason: String,
        /// Epoch when device was revoked
        revoked_epoch: Epoch,
        /// Timestamp when device was revoked (uses unified time system)
        revoked_at: PhysicalTime,
    },

    /// Device reactivated after suspension
    DeviceReactivated {
        /// Device being reactivated
        device_id: DeviceId,
        /// Authority that owns this device
        authority_id: AuthorityId,
        /// Epoch when device was reactivated
        reactivated_epoch: Epoch,
        /// Timestamp when device was reactivated (uses unified time system)
        reactivated_at: PhysicalTime,
    },

    /// Device capabilities updated
    DeviceCapabilitiesUpdated {
        /// Device being updated
        device_id: DeviceId,
        /// Authority that owns this device
        authority_id: AuthorityId,
        /// New capabilities
        new_capabilities: Cap,
        /// Epoch when capabilities were updated
        updated_epoch: Epoch,
        /// Timestamp when capabilities were updated (uses unified time system)
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
    /// Device signature verification
    Device,
    /// Guardian signature verification
    Guardian,
    /// Authority signature verification
    Authority,
    /// Threshold signature verification
    Threshold,
}

impl VerifyFact {
    /// Get the authority ID associated with this fact, if applicable
    pub fn authority_id(&self) -> Option<AuthorityId> {
        match self {
            VerifyFact::DeviceRegistered { authority_id, .. } => Some(*authority_id),
            VerifyFact::DeviceSuspended { authority_id, .. } => Some(*authority_id),
            VerifyFact::DeviceRevoked { authority_id, .. } => Some(*authority_id),
            VerifyFact::DeviceReactivated { authority_id, .. } => Some(*authority_id),
            VerifyFact::DeviceCapabilitiesUpdated { authority_id, .. } => Some(*authority_id),
            VerifyFact::AccountPolicySet { .. } => None,
            VerifyFact::IdentityVerified { authority_id, .. } => Some(*authority_id),
            VerifyFact::ThresholdSignatureVerified { .. } => None,
        }
    }

    /// Get the device ID associated with this fact, if applicable
    pub fn device_id(&self) -> Option<DeviceId> {
        match self {
            VerifyFact::DeviceRegistered { device_id, .. } => Some(*device_id),
            VerifyFact::DeviceSuspended { device_id, .. } => Some(*device_id),
            VerifyFact::DeviceRevoked { device_id, .. } => Some(*device_id),
            VerifyFact::DeviceReactivated { device_id, .. } => Some(*device_id),
            VerifyFact::DeviceCapabilitiesUpdated { device_id, .. } => Some(*device_id),
            VerifyFact::AccountPolicySet { .. } => None,
            VerifyFact::IdentityVerified { .. } => None,
            VerifyFact::ThresholdSignatureVerified { .. } => None,
        }
    }

    /// Get the timestamp for this fact in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            VerifyFact::DeviceRegistered { registered_at, .. } => registered_at.ts_ms,
            VerifyFact::DeviceSuspended { suspended_at, .. } => suspended_at.ts_ms,
            VerifyFact::DeviceRevoked { revoked_at, .. } => revoked_at.ts_ms,
            VerifyFact::DeviceReactivated { reactivated_at, .. } => reactivated_at.ts_ms,
            VerifyFact::DeviceCapabilitiesUpdated { updated_at, .. } => updated_at.ts_ms,
            VerifyFact::AccountPolicySet { set_at, .. } => set_at.ts_ms,
            VerifyFact::IdentityVerified { verified_at, .. } => verified_at.ts_ms,
            VerifyFact::ThresholdSignatureVerified { verified_at, .. } => verified_at.ts_ms,
        }
    }

    /// Encode this fact with a canonical envelope.
    pub fn to_bytes(&self) -> Vec<u8> {
        encode_domain_fact(VERIFY_FACT_TYPE_ID, VERIFY_FACT_SCHEMA_VERSION, self)
    }

    /// Decode a fact from a canonical envelope.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        decode_domain_fact(VERIFY_FACT_TYPE_ID, VERIFY_FACT_SCHEMA_VERSION, bytes)
    }

    /// Create a DeviceRegistered fact with millisecond timestamp (backward compatibility)
    pub fn device_registered_ms(
        authority_id: AuthorityId,
        device_id: DeviceId,
        public_key: Vec<u8>,
        capabilities: Cap,
        registered_epoch: Epoch,
        registered_at_ms: u64,
    ) -> Self {
        Self::DeviceRegistered {
            authority_id,
            device_id,
            public_key,
            capabilities,
            registered_epoch,
            registered_at: PhysicalTime {
                ts_ms: registered_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a DeviceSuspended fact with millisecond timestamp (backward compatibility)
    pub fn device_suspended_ms(
        device_id: DeviceId,
        authority_id: AuthorityId,
        reason: String,
        suspended_epoch: Epoch,
        suspended_at_ms: u64,
    ) -> Self {
        Self::DeviceSuspended {
            device_id,
            authority_id,
            reason,
            suspended_epoch,
            suspended_at: PhysicalTime {
                ts_ms: suspended_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a DeviceRevoked fact with millisecond timestamp (backward compatibility)
    pub fn device_revoked_ms(
        device_id: DeviceId,
        authority_id: AuthorityId,
        reason: String,
        revoked_epoch: Epoch,
        revoked_at_ms: u64,
    ) -> Self {
        Self::DeviceRevoked {
            device_id,
            authority_id,
            reason,
            revoked_epoch,
            revoked_at: PhysicalTime {
                ts_ms: revoked_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a DeviceReactivated fact with millisecond timestamp (backward compatibility)
    pub fn device_reactivated_ms(
        device_id: DeviceId,
        authority_id: AuthorityId,
        reactivated_epoch: Epoch,
        reactivated_at_ms: u64,
    ) -> Self {
        Self::DeviceReactivated {
            device_id,
            authority_id,
            reactivated_epoch,
            reactivated_at: PhysicalTime {
                ts_ms: reactivated_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a DeviceCapabilitiesUpdated fact with millisecond timestamp (backward compatibility)
    pub fn device_capabilities_updated_ms(
        device_id: DeviceId,
        authority_id: AuthorityId,
        new_capabilities: Cap,
        updated_epoch: Epoch,
        updated_at_ms: u64,
    ) -> Self {
        Self::DeviceCapabilitiesUpdated {
            device_id,
            authority_id,
            new_capabilities,
            updated_epoch,
            updated_at: PhysicalTime {
                ts_ms: updated_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AccountPolicySet fact with millisecond timestamp (backward compatibility)
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

    /// Create an IdentityVerified fact with millisecond timestamp (backward compatibility)
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

    /// Create a ThresholdSignatureVerified fact with millisecond timestamp (backward compatibility)
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
            VerifyFact::DeviceRegistered {
                registered_epoch, ..
            } => Some(*registered_epoch),
            VerifyFact::DeviceSuspended {
                suspended_epoch, ..
            } => Some(*suspended_epoch),
            VerifyFact::DeviceRevoked { revoked_epoch, .. } => Some(*revoked_epoch),
            VerifyFact::DeviceReactivated {
                reactivated_epoch, ..
            } => Some(*reactivated_epoch),
            VerifyFact::DeviceCapabilitiesUpdated { updated_epoch, .. } => Some(*updated_epoch),
            VerifyFact::AccountPolicySet { set_epoch, .. } => Some(*set_epoch),
            VerifyFact::IdentityVerified { .. } => None,
            VerifyFact::ThresholdSignatureVerified { .. } => None,
        }
    }

    /// Get the fact type name for journal keying
    pub fn fact_type(&self) -> &'static str {
        match self {
            VerifyFact::DeviceRegistered { .. } => "device_registered",
            VerifyFact::DeviceSuspended { .. } => "device_suspended",
            VerifyFact::DeviceRevoked { .. } => "device_revoked",
            VerifyFact::DeviceReactivated { .. } => "device_reactivated",
            VerifyFact::DeviceCapabilitiesUpdated { .. } => "device_capabilities_updated",
            VerifyFact::AccountPolicySet { .. } => "account_policy_set",
            VerifyFact::IdentityVerified { .. } => "identity_verified",
            VerifyFact::ThresholdSignatureVerified { .. } => "threshold_signature_verified",
        }
    }
}

/// Delta type for verification fact application
#[derive(Debug, Clone, Default)]
pub struct VerifyFactDelta {
    /// Devices registered in this delta
    pub devices_registered: Vec<DeviceId>,
    /// Devices suspended in this delta
    pub devices_suspended: Vec<DeviceId>,
    /// Devices revoked in this delta
    pub devices_revoked: Vec<DeviceId>,
    /// Devices reactivated in this delta
    pub devices_reactivated: Vec<DeviceId>,
    /// Verifications performed in this delta
    pub verifications_performed: u64,
    /// Successful verifications in this delta
    pub verifications_successful: u64,
}

/// Reducer for verification facts
#[derive(Debug, Clone, Default)]
pub struct VerifyFactReducer;

impl VerifyFactReducer {
    /// Create a new verification fact reducer
    pub fn new() -> Self {
        Self
    }

    /// Apply a fact to produce a delta
    pub fn apply(&self, fact: &VerifyFact) -> VerifyFactDelta {
        let mut delta = VerifyFactDelta::default();

        match fact {
            VerifyFact::DeviceRegistered { device_id, .. } => {
                delta.devices_registered.push(*device_id);
            }
            VerifyFact::DeviceSuspended { device_id, .. } => {
                delta.devices_suspended.push(*device_id);
            }
            VerifyFact::DeviceRevoked { device_id, .. } => {
                delta.devices_revoked.push(*device_id);
            }
            VerifyFact::DeviceReactivated { device_id, .. } => {
                delta.devices_reactivated.push(*device_id);
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
            VerifyFact::DeviceCapabilitiesUpdated { .. } | VerifyFact::AccountPolicySet { .. } => {
                // These don't produce cumulative deltas for device lifecycle
            }
        }

        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_fact_device_id() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let device_id = DeviceId::new_from_entropy([2u8; 32]);

        let fact = VerifyFact::device_registered_ms(
            authority_id,
            device_id,
            vec![1, 2, 3, 4],
            Cap::top(),
            Epoch(1),
            1000,
        );

        assert_eq!(fact.authority_id(), Some(authority_id));
        assert_eq!(fact.device_id(), Some(device_id));
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.epoch(), Some(Epoch(1)));
        assert_eq!(fact.fact_type(), "device_registered");
    }

    #[test]
    fn test_verify_fact_reducer() {
        let reducer = VerifyFactReducer::new();
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let device_id = DeviceId::new_from_entropy([2u8; 32]);

        let fact = VerifyFact::device_registered_ms(
            authority_id,
            device_id,
            vec![1, 2, 3, 4],
            Cap::top(),
            Epoch(1),
            1000,
        );

        let delta = reducer.apply(&fact);
        assert_eq!(delta.devices_registered.len(), 1);
        assert_eq!(delta.devices_registered[0], device_id);
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
    fn test_timestamp_ms_backward_compat() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let device_id = DeviceId::new_from_entropy([2u8; 32]);

        let fact = VerifyFact::device_suspended_ms(
            device_id,
            authority_id,
            "test reason".to_string(),
            Epoch(1),
            1234567890,
        );
        assert_eq!(fact.timestamp_ms(), 1234567890);
    }
}
