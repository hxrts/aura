//! Web of Trust Domain Facts
//!
//! Pure fact types for authorization and flow budget state changes.
//! These facts are defined here (Layer 2) and committed by higher layers.
//!
//! **Authority Model**: Facts reference authorities using the authority-centric
//! model where authorities hide internal device structure.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::scope::{AuthorizationOp, ResourceScope};
use aura_core::time::PhysicalTime;
use aura_core::types::facts::{
    FactDelta, FactDeltaReducer, FactEncoding, FactEnvelope, FactError, FactTypeId,
    MAX_FACT_PAYLOAD_BYTES,
};
use aura_core::types::Epoch;
use aura_core::util::serialization::{from_slice, to_vec, SerializationError};
use aura_core::Cap;
use serde::{Deserialize, Serialize};

/// Unique type identifier for WoT facts
pub static WOT_FACT_TYPE_ID: FactTypeId = FactTypeId::new("wot/v1");
/// Schema version for WoT facts
pub const WOT_FACT_SCHEMA_VERSION: u16 = 1;

/// Get the typed fact ID for WoT facts
pub fn wot_fact_type_id() -> &'static FactTypeId {
    &WOT_FACT_TYPE_ID
}

/// Web of Trust domain facts for authorization state changes.
///
/// These facts capture flow budget charges, capability delegations,
/// and epoch rotations that affect authorization state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
        /// Scope of the delegated capability
        scope: ResourceScope,
        /// Capabilities being delegated
        capabilities: Cap,
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
        initial_capabilities: Vec<AuthorizationOp>,
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
    ///
    /// # Errors
    ///
    /// Returns `FactError` if serialization fails.
    pub fn try_encode(&self) -> Result<Vec<u8>, FactError> {
        let payload = to_vec(self)?;
        if payload.len() > MAX_FACT_PAYLOAD_BYTES {
            return Err(FactError::PayloadTooLarge {
                size: payload.len() as u64,
                max: MAX_FACT_PAYLOAD_BYTES as u64,
            });
        }
        let envelope = FactEnvelope {
            type_id: wot_fact_type_id().clone(),
            schema_version: WOT_FACT_SCHEMA_VERSION,
            encoding: FactEncoding::DagCbor,
            payload,
        };
        let bytes = to_vec(&envelope)?;
        Ok(bytes)
    }

    /// Decode a fact from a canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns `FactError` if deserialization fails or version/type mismatches.
    pub fn try_decode(bytes: &[u8]) -> Result<Self, FactError> {
        let envelope: FactEnvelope = from_slice(bytes)?;

        if envelope.type_id.as_str() != wot_fact_type_id().as_str() {
            return Err(FactError::TypeMismatch {
                expected: wot_fact_type_id().to_string(),
                actual: envelope.type_id.to_string(),
            });
        }

        if envelope.schema_version != WOT_FACT_SCHEMA_VERSION {
            return Err(FactError::VersionMismatch {
                expected: WOT_FACT_SCHEMA_VERSION,
                actual: envelope.schema_version,
            });
        }

        let fact = match envelope.encoding {
            FactEncoding::DagCbor => from_slice(&envelope.payload)?,
            FactEncoding::Json => serde_json::from_slice(&envelope.payload).map_err(|err| {
                FactError::Serialization(SerializationError::InvalidFormat(format!(
                    "JSON decode failed: {err}"
                )))
            })?,
        };
        Ok(fact)
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

impl FactDelta for WotFactDelta {
    fn merge(&mut self, other: &Self) {
        self.flow_charges += other.flow_charges;
        self.epoch_rotations += other.epoch_rotations;
        self.capabilities_delegated += other.capabilities_delegated;
        self.capabilities_revoked += other.capabilities_revoked;
        self.tokens_issued += other.tokens_issued;
        self.tokens_attenuated += other.tokens_attenuated;
    }
}

/// Reducer for WoT facts
#[derive(Debug, Clone, Default)]
pub struct WotFactReducer;

impl WotFactReducer {
    /// Create a new WoT fact reducer
    pub fn new() -> Self {
        Self
    }
}

impl FactDeltaReducer<WotFact, WotFactDelta> for WotFactReducer {
    fn apply(&self, fact: &WotFact) -> WotFactDelta {
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
    use aura_core::scope::ContextOp;
    use aura_core::types::facts::FactDeltaReducer;

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
            epoch: Epoch::new(1),
            charged_at: pt(1000),
        };

        assert_eq!(fact.authority_id(), sender);
        assert_eq!(fact.context_id(), Some(context_id));
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.epoch(), Some(Epoch::new(1)));
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
            epoch: Epoch::new(1),
            charged_at: pt(1000),
        };

        let delta = reducer.apply(&fact);
        assert_eq!(delta.flow_charges, 100);
    }

    #[test]
    fn test_capability_delegated_fact() {
        let grantor = AuthorityId::new_from_entropy([1u8; 32]);
        let grantee = AuthorityId::new_from_entropy([2u8; 32]);
        let context_id = ContextId::new_from_entropy([9u8; 32]);
        let scope = ResourceScope::Context {
            context_id,
            operation: ContextOp::AddBinding,
        };
        let capabilities = Cap::new();

        let fact = WotFact::CapabilityDelegated {
            grantor,
            grantee,
            scope,
            capabilities,
            delegation_epoch: Epoch::new(1),
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
            initial_capabilities: vec![AuthorizationOp::Read, AuthorizationOp::Write],
            issued_epoch: Epoch::new(1),
            issued_at: pt(3000),
        };

        assert_eq!(fact.authority_id(), issuer);
        assert_eq!(fact.timestamp_ms(), 3000);
        assert_eq!(fact.fact_type(), "token_issued");

        let reducer = WotFactReducer::new();
        let delta = reducer.apply(&fact);
        assert_eq!(delta.tokens_issued, 1);
    }

    #[test]
    fn test_capability_revoked_fact() {
        let revoker = AuthorityId::new_from_entropy([4u8; 32]);
        let revokee = AuthorityId::new_from_entropy([5u8; 32]);

        let fact = WotFact::CapabilityRevoked {
            revoker,
            revokee,
            reason: "expired".to_string(),
            revocation_epoch: Epoch::new(3),
            revoked_at: pt(4000),
        };

        assert_eq!(fact.authority_id(), revoker);
        assert_eq!(fact.timestamp_ms(), 4000);
        assert_eq!(fact.epoch(), Some(Epoch::new(3)));
        assert_eq!(fact.fact_type(), "capability_revoked");

        let reducer = WotFactReducer::new();
        let delta = reducer.apply(&fact);
        assert_eq!(delta.capabilities_revoked, 1);
    }

    #[test]
    fn test_token_attenuated_fact() {
        let attenuator = AuthorityId::new_from_entropy([6u8; 32]);

        let fact = WotFact::TokenAttenuated {
            attenuator,
            original_fingerprint: [1u8; 32],
            new_fingerprint: [2u8; 32],
            attenuation_type: "restrict_read".to_string(),
            attenuated_at: pt(5000),
        };

        assert_eq!(fact.authority_id(), attenuator);
        assert_eq!(fact.timestamp_ms(), 5000);
        assert_eq!(fact.epoch(), None);
        assert_eq!(fact.fact_type(), "token_attenuated");

        let reducer = WotFactReducer::new();
        let delta = reducer.apply(&fact);
        assert_eq!(delta.tokens_attenuated, 1);
    }
}

/// Property tests for semilattice laws on WotFactDelta
#[cfg(test)]
mod proptest_semilattice {
    use super::*;
    use aura_core::types::facts::FactDelta;
    use proptest::prelude::*;

    /// Strategy for generating arbitrary WotFactDelta values
    fn arb_delta() -> impl Strategy<Value = WotFactDelta> {
        (
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
            0u64..1000,
        )
            .prop_map(
                |(
                    flow_charges,
                    epoch_rotations,
                    capabilities_delegated,
                    capabilities_revoked,
                    tokens_issued,
                    tokens_attenuated,
                )| {
                    WotFactDelta {
                        flow_charges,
                        epoch_rotations,
                        capabilities_delegated,
                        capabilities_revoked,
                        tokens_issued,
                        tokens_attenuated,
                    }
                },
            )
    }

    /// Helper to check if two deltas are equal
    fn deltas_equal(a: &WotFactDelta, b: &WotFactDelta) -> bool {
        a.flow_charges == b.flow_charges
            && a.epoch_rotations == b.epoch_rotations
            && a.capabilities_delegated == b.capabilities_delegated
            && a.capabilities_revoked == b.capabilities_revoked
            && a.tokens_issued == b.tokens_issued
            && a.tokens_attenuated == b.tokens_attenuated
    }

    proptest! {
        /// Idempotence: merging with self doubles the value (additive merge)
        #[test]
        fn merge_idempotent(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&original);
            // For additive deltas: a + a = 2a
            prop_assert_eq!(result.flow_charges, original.flow_charges * 2);
            prop_assert_eq!(result.epoch_rotations, original.epoch_rotations * 2);
        }

        /// Commutativity: a.merge(&b) == b.merge(&a) (result equivalence)
        #[test]
        fn merge_commutative(a in arb_delta(), b in arb_delta()) {
            let mut ab = a.clone();
            ab.merge(&b);

            let mut ba = b.clone();
            ba.merge(&a);

            prop_assert!(deltas_equal(&ab, &ba), "merge should be commutative");
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

            prop_assert!(deltas_equal(&left, &right), "merge should be associative");
        }

        /// Identity: merge with default (zero) leaves value unchanged
        #[test]
        fn merge_identity(a in arb_delta()) {
            let original = a.clone();
            let mut result = a.clone();
            result.merge(&WotFactDelta::default());

            prop_assert!(deltas_equal(&result, &original), "merge with identity should preserve value");
        }
    }
}
