//! Web of Trust Domain Facts
//!
//! Pure fact types for authorization and flow budget state changes.
//! These facts are defined here (Layer 2) and committed by higher layers.
//!
//! **Authority Model**: Facts reference authorities using the authority-centric
//! model where authorities hide internal device structure.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::PhysicalTime;
use aura_core::types::epochs::Epoch;
use aura_core::util::serialization::{from_slice, to_vec, SemanticVersion, VersionedMessage};
use serde::{Deserialize, Serialize};

/// Unique type identifier for WoT facts
pub const WOT_FACT_TYPE_ID: &str = "wot/v1";
/// Schema version for WoT facts
pub const WOT_FACT_SCHEMA_VERSION: u16 = 1;

/// Web of Trust domain facts for authorization state changes.
///
/// These facts capture flow budget charges, capability delegations,
/// and epoch rotations that affect authorization state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WotFact {
    /// Flow budget charged for a message send
    ///
    /// Per CLAUDE.md: "only the `spent` counters are facts; limits are derived at runtime"
    FlowBudgetCharged {
        /// Relational context where charge occurred
        context_id: ContextId,
        /// Sending authority
        sender: AuthorityId,
        /// Receiving authority
        recipient: AuthorityId,
        /// Cost charged
        cost: u32,
        /// New spent total after charge
        new_spent: u64,
        /// Epoch when charge occurred
        epoch: Epoch,
        /// Timestamp when charge occurred (unified time system)
        charged_at: PhysicalTime,
    },

    /// Flow budget epoch rotated (resets spent counters)
    FlowBudgetEpochRotated {
        /// Relational context where rotation occurred
        context_id: ContextId,
        /// Authority whose budget epoch rotated
        authority_id: AuthorityId,
        /// Peer authority for this budget
        peer_id: AuthorityId,
        /// Previous epoch
        old_epoch: Epoch,
        /// New epoch
        new_epoch: Epoch,
        /// Timestamp when rotation occurred (unified time system)
        rotated_at: PhysicalTime,
    },

    /// Capability delegated from one authority to another
    CapabilityDelegated {
        /// Authority granting the capability
        grantor: AuthorityId,
        /// Authority receiving the capability
        grantee: AuthorityId,
        /// Scope of the delegated capability (serialized ResourceScope)
        scope: Vec<u8>,
        /// Capabilities being delegated (serialized Cap)
        capabilities: Vec<u8>,
        /// Epoch when delegation occurred
        delegation_epoch: Epoch,
        /// Timestamp when delegation occurred (unified time system)
        delegated_at: PhysicalTime,
    },

    /// Capability revoked
    CapabilityRevoked {
        /// Authority revoking the capability
        revoker: AuthorityId,
        /// Authority losing the capability
        revokee: AuthorityId,
        /// Reason for revocation
        reason: String,
        /// Epoch when revocation occurred
        revocation_epoch: Epoch,
        /// Timestamp when revocation occurred (unified time system)
        revoked_at: PhysicalTime,
    },

    /// Token issued for an authority
    TokenIssued {
        /// Issuing authority (root)
        issuer: AuthorityId,
        /// Authority receiving the token
        recipient: AuthorityId,
        /// Token fingerprint (hash of token for reference)
        token_fingerprint: [u8; 32],
        /// Initial capabilities in the token
        initial_capabilities: Vec<String>,
        /// Epoch when token was issued
        issued_epoch: Epoch,
        /// Timestamp when token was issued (unified time system)
        issued_at: PhysicalTime,
    },

    /// Token attenuated (capabilities reduced)
    TokenAttenuated {
        /// Authority performing attenuation
        attenuator: AuthorityId,
        /// Original token fingerprint
        original_fingerprint: [u8; 32],
        /// New token fingerprint
        new_fingerprint: [u8; 32],
        /// Attenuation description
        attenuation_type: String,
        /// Timestamp when attenuation occurred (unified time system)
        attenuated_at: PhysicalTime,
    },
}

impl WotFact {
    fn version() -> SemanticVersion {
        SemanticVersion::new(WOT_FACT_SCHEMA_VERSION, 0, 0)
    }

    /// Get the primary authority ID associated with this fact
    pub fn authority_id(&self) -> AuthorityId {
        match self {
            WotFact::FlowBudgetCharged { sender, .. } => *sender,
            WotFact::FlowBudgetEpochRotated { authority_id, .. } => *authority_id,
            WotFact::CapabilityDelegated { grantor, .. } => *grantor,
            WotFact::CapabilityRevoked { revoker, .. } => *revoker,
            WotFact::TokenIssued { issuer, .. } => *issuer,
            WotFact::TokenAttenuated { attenuator, .. } => *attenuator,
        }
    }

    /// Get the context ID if applicable
    pub fn context_id(&self) -> Option<ContextId> {
        match self {
            WotFact::FlowBudgetCharged { context_id, .. } => Some(*context_id),
            WotFact::FlowBudgetEpochRotated { context_id, .. } => Some(*context_id),
            _ => None,
        }
    }

    /// Get the timestamp for this fact in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            WotFact::FlowBudgetCharged { charged_at, .. } => charged_at.ts_ms,
            WotFact::FlowBudgetEpochRotated { rotated_at, .. } => rotated_at.ts_ms,
            WotFact::CapabilityDelegated { delegated_at, .. } => delegated_at.ts_ms,
            WotFact::CapabilityRevoked { revoked_at, .. } => revoked_at.ts_ms,
            WotFact::TokenIssued { issued_at, .. } => issued_at.ts_ms,
            WotFact::TokenAttenuated { attenuated_at, .. } => attenuated_at.ts_ms,
        }
    }

    /// Get the epoch for this fact if applicable
    pub fn epoch(&self) -> Option<Epoch> {
        match self {
            WotFact::FlowBudgetCharged { epoch, .. } => Some(*epoch),
            WotFact::FlowBudgetEpochRotated { new_epoch, .. } => Some(*new_epoch),
            WotFact::CapabilityDelegated {
                delegation_epoch, ..
            } => Some(*delegation_epoch),
            WotFact::CapabilityRevoked {
                revocation_epoch, ..
            } => Some(*revocation_epoch),
            WotFact::TokenIssued { issued_epoch, .. } => Some(*issued_epoch),
            WotFact::TokenAttenuated { .. } => None,
        }
    }

    /// Get the fact type name for journal keying
    pub fn fact_type(&self) -> &'static str {
        match self {
            WotFact::FlowBudgetCharged { .. } => "flow_budget_charged",
            WotFact::FlowBudgetEpochRotated { .. } => "flow_budget_epoch_rotated",
            WotFact::CapabilityDelegated { .. } => "capability_delegated",
            WotFact::CapabilityRevoked { .. } => "capability_revoked",
            WotFact::TokenIssued { .. } => "token_issued",
            WotFact::TokenAttenuated { .. } => "token_attenuated",
        }
    }

    /// Encode this fact with a canonical envelope.
    pub fn to_bytes(&self) -> Vec<u8> {
        let message = VersionedMessage::new(self.clone(), Self::version())
            .with_metadata("type".to_string(), WOT_FACT_TYPE_ID.to_string());
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
}

/// Delta type for WoT fact application
#[derive(Debug, Clone, Default)]
pub struct WotFactDelta {
    /// Total flow charges in this delta
    pub flow_charges: u64,
    /// Epoch rotations in this delta
    pub epoch_rotations: u64,
    /// Capabilities delegated in this delta
    pub capabilities_delegated: u64,
    /// Capabilities revoked in this delta
    pub capabilities_revoked: u64,
    /// Tokens issued in this delta
    pub tokens_issued: u64,
    /// Tokens attenuated in this delta
    pub tokens_attenuated: u64,
}

/// Reducer for WoT facts
#[derive(Debug, Clone, Default)]
pub struct WotFactReducer;

impl WotFactReducer {
    /// Create a new WoT fact reducer
    pub fn new() -> Self {
        Self
    }

    /// Apply a fact to produce a delta
    pub fn apply(&self, fact: &WotFact) -> WotFactDelta {
        let mut delta = WotFactDelta::default();

        match fact {
            WotFact::FlowBudgetCharged { cost, .. } => {
                delta.flow_charges = *cost as u64;
            }
            WotFact::FlowBudgetEpochRotated { .. } => {
                delta.epoch_rotations = 1;
            }
            WotFact::CapabilityDelegated { .. } => {
                delta.capabilities_delegated = 1;
            }
            WotFact::CapabilityRevoked { .. } => {
                delta.capabilities_revoked = 1;
            }
            WotFact::TokenIssued { .. } => {
                delta.tokens_issued = 1;
            }
            WotFact::TokenAttenuated { .. } => {
                delta.tokens_attenuated = 1;
            }
        }

        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_flow_budget_charged_fact() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let sender = AuthorityId::new_from_entropy([2u8; 32]);
        let recipient = AuthorityId::new_from_entropy([3u8; 32]);

        let fact = WotFact::FlowBudgetCharged {
            context_id,
            sender,
            recipient,
            cost: 100,
            new_spent: 500,
            epoch: Epoch(1),
            charged_at: pt(1000),
        };

        assert_eq!(fact.authority_id(), sender);
        assert_eq!(fact.context_id(), Some(context_id));
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.epoch(), Some(Epoch(1)));
        assert_eq!(fact.fact_type(), "flow_budget_charged");
    }

    #[test]
    fn test_wot_fact_reducer() {
        let reducer = WotFactReducer::new();
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let sender = AuthorityId::new_from_entropy([2u8; 32]);
        let recipient = AuthorityId::new_from_entropy([3u8; 32]);

        let fact = WotFact::FlowBudgetCharged {
            context_id,
            sender,
            recipient,
            cost: 100,
            new_spent: 500,
            epoch: Epoch(1),
            charged_at: pt(1000),
        };

        let delta = reducer.apply(&fact);
        assert_eq!(delta.flow_charges, 100);
    }

    #[test]
    fn test_capability_delegated_fact() {
        let grantor = AuthorityId::new_from_entropy([1u8; 32]);
        let grantee = AuthorityId::new_from_entropy([2u8; 32]);

        let fact = WotFact::CapabilityDelegated {
            grantor,
            grantee,
            scope: vec![1, 2, 3],
            capabilities: vec![4, 5, 6],
            delegation_epoch: Epoch(1),
            delegated_at: pt(2000),
        };

        assert_eq!(fact.authority_id(), grantor);
        assert_eq!(fact.context_id(), None);
        assert_eq!(fact.timestamp_ms(), 2000);
        assert_eq!(fact.fact_type(), "capability_delegated");

        let reducer = WotFactReducer::new();
        let delta = reducer.apply(&fact);
        assert_eq!(delta.capabilities_delegated, 1);
    }

    #[test]
    fn test_token_issued_fact() {
        let issuer = AuthorityId::new_from_entropy([1u8; 32]);
        let recipient = AuthorityId::new_from_entropy([2u8; 32]);

        let fact = WotFact::TokenIssued {
            issuer,
            recipient,
            token_fingerprint: [0u8; 32],
            initial_capabilities: vec!["read".to_string(), "write".to_string()],
            issued_epoch: Epoch(1),
            issued_at: pt(3000),
        };

        assert_eq!(fact.authority_id(), issuer);
        assert_eq!(fact.timestamp_ms(), 3000);
        assert_eq!(fact.fact_type(), "token_issued");

        let reducer = WotFactReducer::new();
        let delta = reducer.apply(&fact);
        assert_eq!(delta.tokens_issued, 1);
    }
}
