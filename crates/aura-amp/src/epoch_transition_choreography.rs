//! AMP channel epoch-transition choreography messages.
//!
//! The Telltale protocol declares the coordinator/witness/finalizer message
//! flow. These message structs keep the protocol payloads bound to the same
//! transition identity used by the journal facts.

use aura_core::types::identifiers::AuthorityId;
use aura_core::Hash32;
use aura_journal::fact::{
    AmpEmergencyAlarm, AmpTransitionConflict, AmpTransitionIdentity, AmpTransitionPolicy,
    AmpTransitionWitnessSignature, CertifiedChannelEpochBump, FinalizedChannelEpochBump,
    ProposedChannelEpochBump, ProtocolRelationalFact,
};
use aura_macros::tell;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

tell!(include_str!("src/epoch_transition.tell"));

/// Emergency quarantine choreography declaration.
pub mod quarantine {
    use aura_macros::tell;

    tell!(include_str!("src/epoch_transition_quarantine.tell"));
}

/// Emergency cryptoshred choreography declaration.
pub mod cryptoshred {
    use aura_macros::tell;

    tell!(include_str!("src/epoch_transition_cryptoshred.tell"));
}

/// Proposal payload for normal, quarantine, or cryptoshred AMP epoch transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpTransitionProposalMessage {
    /// Canonical transition identity.
    pub identity: AmpTransitionIdentity,
    /// Canonical transition identity digest.
    pub transition_id: Hash32,
    /// Coordinator proposing the transition.
    pub coordinator: AuthorityId,
    /// Digest of authenticated proposal bytes.
    pub proposal_digest: Hash32,
}

impl AmpTransitionProposalMessage {
    /// Build a proposal message and bind the canonical transition id.
    pub fn new(
        identity: AmpTransitionIdentity,
        coordinator: AuthorityId,
        proposal_digest: Hash32,
    ) -> Self {
        let transition_id = identity.transition_id();
        Self {
            identity,
            transition_id,
            coordinator,
            proposal_digest,
        }
    }

    /// Return true when the message id matches its canonical identity.
    pub fn is_identity_bound(&self) -> bool {
        self.transition_id == self.identity.transition_id()
    }

    /// Convert this choreography payload to the proposal fact emitted by adapters.
    pub fn to_protocol_fact(&self) -> ProtocolRelationalFact {
        ProtocolRelationalFact::AmpProposedChannelEpochBump(ProposedChannelEpochBump {
            context: self.identity.context,
            channel: self.identity.channel,
            parent_epoch: self.identity.parent_epoch,
            new_epoch: self.identity.successor_epoch,
            bump_id: self.identity.successor_commitment,
            reason: policy_reason(self.identity.transition_policy),
            parent_commitment: self.identity.parent_commitment,
            successor_commitment: self.identity.successor_commitment,
            membership_commitment: self.identity.membership_commitment,
            transition_policy: self.identity.transition_policy,
            transition_id: self.transition_id,
        })
    }
}

/// Witness response over the canonical AMP transition payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpTransitionWitnessMessage {
    /// Canonical transition identity digest.
    pub transition_id: Hash32,
    /// Parent prestate commitment witnessed.
    pub parent_commitment: Hash32,
    /// Successor state commitment witnessed.
    pub successor_commitment: Hash32,
    /// Successor membership commitment witnessed.
    pub membership_commitment: Hash32,
    /// Policy class witnessed.
    pub transition_policy: AmpTransitionPolicy,
    /// Witness signature over the canonical payload.
    pub witness_signature: AmpTransitionWitnessSignature,
}

impl AmpTransitionWitnessMessage {
    /// Build a witness message from a transition identity and witness signature.
    pub fn new(
        identity: &AmpTransitionIdentity,
        witness_signature: AmpTransitionWitnessSignature,
    ) -> Self {
        Self {
            transition_id: identity.transition_id(),
            parent_commitment: identity.parent_commitment,
            successor_commitment: identity.successor_commitment,
            membership_commitment: identity.membership_commitment,
            transition_policy: identity.transition_policy,
            witness_signature,
        }
    }
}

/// A2 certificate publication handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpTransitionCertificateMessage {
    /// Canonical transition identity.
    pub identity: AmpTransitionIdentity,
    /// Canonical transition identity digest.
    pub transition_id: Hash32,
    /// Digest of the witness committee used for this certificate.
    pub committee_digest: Hash32,
    /// Digest of the canonical witness payload signed by witnesses.
    pub witness_payload_digest: Hash32,
    /// Required quorum threshold.
    pub threshold: u16,
    /// Declared Byzantine fault bound.
    pub fault_bound: u16,
    /// Witness signatures included in the certificate.
    pub witness_signatures: Vec<AmpTransitionWitnessSignature>,
    /// Authorities explicitly excluded by the successor policy.
    #[serde(default)]
    pub excluded_authorities: BTreeSet<AuthorityId>,
    /// Whether readable pre-transition state is destroyed at A2Live.
    #[serde(default)]
    pub readable_state_destroyed: bool,
}

impl AmpTransitionCertificateMessage {
    /// Return true when the certificate message binds the canonical transition id.
    pub fn is_identity_bound(&self) -> bool {
        self.transition_id == self.identity.transition_id()
    }

    /// Convert this choreography payload to the A2 certificate fact emitted by adapters.
    pub fn to_protocol_fact(&self) -> ProtocolRelationalFact {
        ProtocolRelationalFact::AmpCertifiedChannelEpochBump(CertifiedChannelEpochBump {
            identity: self.identity.clone(),
            transition_id: self.transition_id,
            witness_payload_digest: self.witness_payload_digest,
            committee_digest: self.committee_digest,
            threshold: self.threshold,
            fault_bound: self.fault_bound,
            coord_epoch: None,
            generation_min: None,
            generation_max: None,
            witness_signatures: self.witness_signatures.clone(),
            equivocation_refs: BTreeSet::new(),
            excluded_authorities: self.excluded_authorities.clone(),
            readable_state_destroyed: self.readable_state_destroyed,
        })
    }
}

/// A3 finalization handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpTransitionFinalizationMessage {
    /// Canonical transition identity.
    pub identity: AmpTransitionIdentity,
    /// Canonical transition identity digest.
    pub transition_id: Hash32,
    /// Consensus identifier that finalized the transition.
    pub consensus_id: Hash32,
    /// Optional transcript reference for finalized key material.
    pub transcript_ref: Option<Hash32>,
}

impl AmpTransitionFinalizationMessage {
    /// Return true when finalization binds the canonical transition id.
    pub fn is_identity_bound(&self) -> bool {
        self.transition_id == self.identity.transition_id()
    }

    /// Convert this choreography payload to the A3 finalization fact emitted by adapters.
    pub fn to_protocol_fact(&self) -> ProtocolRelationalFact {
        ProtocolRelationalFact::AmpFinalizedChannelEpochBump(FinalizedChannelEpochBump {
            identity: self.identity.clone(),
            transition_id: self.transition_id,
            consensus_id: self.consensus_id,
            transcript_ref: self.transcript_ref,
            excluded_authorities: BTreeSet::new(),
            readable_state_destroyed: false,
        })
    }
}

/// Emergency alarm observation payload for quarantine or cryptoshred paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpEmergencyAlarmMessage {
    /// Canonical transition identity for the parent where the alarm applies.
    pub identity: AmpTransitionIdentity,
    /// Suspected compromised authority.
    pub suspect: AuthorityId,
    /// Authority raising the alarm.
    pub raised_by: AuthorityId,
    /// Alarm evidence digest.
    pub evidence_id: Hash32,
}

impl AmpEmergencyAlarmMessage {
    /// Convert this choreography payload to the informational emergency alarm fact.
    pub fn to_protocol_fact(&self) -> ProtocolRelationalFact {
        ProtocolRelationalFact::AmpEmergencyAlarm(AmpEmergencyAlarm {
            context: self.identity.context,
            channel: self.identity.channel,
            parent_epoch: self.identity.parent_epoch,
            parent_commitment: self.identity.parent_commitment,
            suspect: self.suspect,
            raised_by: self.raised_by,
            evidence_id: self.evidence_id,
        })
    }
}

/// Conflict evidence payload for duplicate or conflicting transition witnesses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmpTransitionConflictMessage {
    /// Parent transition group where the conflict occurred.
    pub identity: AmpTransitionIdentity,
    /// First conflicting transition id.
    pub first_transition_id: Hash32,
    /// Second conflicting transition id.
    pub second_transition_id: Hash32,
    /// Witness accused of duplicate signing, if known.
    pub equivocating_witness: Option<AuthorityId>,
    /// Conflict evidence digest.
    pub evidence_id: Hash32,
}

impl AmpTransitionConflictMessage {
    /// Convert this choreography payload to the reducer-visible conflict fact.
    pub fn to_protocol_fact(&self) -> ProtocolRelationalFact {
        ProtocolRelationalFact::AmpTransitionConflict(AmpTransitionConflict {
            context: self.identity.context,
            channel: self.identity.channel,
            parent_epoch: self.identity.parent_epoch,
            parent_commitment: self.identity.parent_commitment,
            first_transition_id: self.first_transition_id,
            second_transition_id: self.second_transition_id,
            equivocating_witness: self.equivocating_witness,
            evidence_id: self.evidence_id,
        })
    }
}

fn policy_reason(policy: AmpTransitionPolicy) -> aura_journal::fact::ChannelBumpReason {
    match policy {
        AmpTransitionPolicy::EmergencyQuarantineTransition => {
            aura_journal::fact::ChannelBumpReason::SuspiciousActivity
        }
        AmpTransitionPolicy::EmergencyCryptoshredTransition => {
            aura_journal::fact::ChannelBumpReason::ConfirmedCompromise
        }
        AmpTransitionPolicy::NormalTransition
        | AmpTransitionPolicy::AdditiveTransition
        | AmpTransitionPolicy::SubtractiveTransition => {
            aura_journal::fact::ChannelBumpReason::Routine
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::identifiers::{ChannelId, ContextId};

    #[test]
    fn proposal_witness_certificate_and_finalization_bind_transition_identity() {
        let identity = AmpTransitionIdentity {
            context: ContextId::new_from_entropy([1u8; 32]),
            channel: ChannelId::from_bytes([2u8; 32]),
            parent_epoch: 4,
            parent_commitment: Hash32::new([3u8; 32]),
            successor_epoch: 5,
            successor_commitment: Hash32::new([4u8; 32]),
            membership_commitment: Hash32::new([5u8; 32]),
            transition_policy: AmpTransitionPolicy::EmergencyQuarantineTransition,
        };
        let transition_id = identity.transition_id();
        let witness_signature = AmpTransitionWitnessSignature {
            witness: AuthorityId::new_from_entropy([6u8; 32]),
            signature: vec![1, 2, 3],
        };
        let proposal = AmpTransitionProposalMessage::new(
            identity.clone(),
            AuthorityId::new_from_entropy([7u8; 32]),
            Hash32::new([8u8; 32]),
        );
        let witness = AmpTransitionWitnessMessage::new(&identity, witness_signature.clone());
        let certificate = AmpTransitionCertificateMessage {
            identity: identity.clone(),
            transition_id,
            committee_digest: Hash32::new([9u8; 32]),
            witness_payload_digest: Hash32::new([11u8; 32]),
            threshold: 1,
            fault_bound: 0,
            witness_signatures: vec![witness_signature],
            excluded_authorities: BTreeSet::new(),
            readable_state_destroyed: false,
        };
        let finalization = AmpTransitionFinalizationMessage {
            identity,
            transition_id,
            consensus_id: Hash32::new([10u8; 32]),
            transcript_ref: None,
        };

        assert!(proposal.is_identity_bound());
        assert_eq!(witness.transition_id, transition_id);
        assert_eq!(witness.parent_commitment, Hash32::new([3u8; 32]));
        assert_eq!(witness.successor_commitment, Hash32::new([4u8; 32]));
        assert_eq!(witness.membership_commitment, Hash32::new([5u8; 32]));
        assert_eq!(
            witness.transition_policy,
            AmpTransitionPolicy::EmergencyQuarantineTransition
        );
        assert!(certificate.is_identity_bound());
        assert!(finalization.is_identity_bound());
    }

    #[test]
    fn normal_choreography_messages_match_runtime_adapter_facts() {
        let identity = transition_identity(AmpTransitionPolicy::NormalTransition);
        let transition_id = identity.transition_id();
        let proposal = AmpTransitionProposalMessage::new(
            identity.clone(),
            AuthorityId::new_from_entropy([20u8; 32]),
            Hash32::new([21u8; 32]),
        );
        let certificate = certificate_message(identity.clone(), false);
        let finalization = AmpTransitionFinalizationMessage {
            identity,
            transition_id,
            consensus_id: Hash32::new([22u8; 32]),
            transcript_ref: None,
        };

        assert!(matches!(
            proposal.to_protocol_fact(),
            ProtocolRelationalFact::AmpProposedChannelEpochBump(ref fact)
                if fact.transition_id == transition_id
        ));
        assert!(matches!(
            certificate.to_protocol_fact(),
            ProtocolRelationalFact::AmpCertifiedChannelEpochBump(ref fact)
                if fact.transition_id == transition_id
                    && fact.identity.transition_policy == AmpTransitionPolicy::NormalTransition
        ));
        assert!(matches!(
            finalization.to_protocol_fact(),
            ProtocolRelationalFact::AmpFinalizedChannelEpochBump(ref fact)
                if fact.transition_id == transition_id
        ));
    }

    #[test]
    fn quarantine_choreography_messages_match_runtime_adapter_facts() {
        let suspect = AuthorityId::new_from_entropy([30u8; 32]);
        let identity = transition_identity(AmpTransitionPolicy::EmergencyQuarantineTransition);
        let transition_id = identity.transition_id();
        let alarm = AmpEmergencyAlarmMessage {
            identity: identity.clone(),
            suspect,
            raised_by: AuthorityId::new_from_entropy([31u8; 32]),
            evidence_id: Hash32::new([32u8; 32]),
        };
        let mut certificate = certificate_message(identity, false);
        certificate.excluded_authorities.insert(suspect);

        assert!(matches!(
            alarm.to_protocol_fact(),
            ProtocolRelationalFact::AmpEmergencyAlarm(ref fact)
                if fact.suspect == suspect
        ));
        assert!(matches!(
            certificate.to_protocol_fact(),
            ProtocolRelationalFact::AmpCertifiedChannelEpochBump(ref fact)
                if fact.transition_id == transition_id
                    && fact.excluded_authorities.contains(&suspect)
                    && fact.identity.transition_policy
                        == AmpTransitionPolicy::EmergencyQuarantineTransition
        ));
    }

    #[test]
    fn cryptoshred_choreography_models_readable_state_destruction_at_a2() {
        let identity = transition_identity(AmpTransitionPolicy::EmergencyCryptoshredTransition);
        let certificate = certificate_message(identity, true);

        assert!(matches!(
            certificate.to_protocol_fact(),
            ProtocolRelationalFact::AmpCertifiedChannelEpochBump(ref fact)
                if fact.readable_state_destroyed
                    && fact.identity.transition_policy
                        == AmpTransitionPolicy::EmergencyCryptoshredTransition
        ));
    }

    #[test]
    fn conflict_choreography_message_matches_runtime_adapter_fact() {
        let identity = transition_identity(AmpTransitionPolicy::NormalTransition);
        let conflict = AmpTransitionConflictMessage {
            identity,
            first_transition_id: Hash32::new([40u8; 32]),
            second_transition_id: Hash32::new([41u8; 32]),
            equivocating_witness: Some(AuthorityId::new_from_entropy([42u8; 32])),
            evidence_id: Hash32::new([43u8; 32]),
        };

        assert!(matches!(
            conflict.to_protocol_fact(),
            ProtocolRelationalFact::AmpTransitionConflict(ref fact)
                if fact.first_transition_id == Hash32::new([40u8; 32])
                    && fact.second_transition_id == Hash32::new([41u8; 32])
        ));
    }

    fn transition_identity(policy: AmpTransitionPolicy) -> AmpTransitionIdentity {
        AmpTransitionIdentity {
            context: ContextId::new_from_entropy([1u8; 32]),
            channel: ChannelId::from_bytes([2u8; 32]),
            parent_epoch: 4,
            parent_commitment: Hash32::new([3u8; 32]),
            successor_epoch: 5,
            successor_commitment: Hash32::new([4u8; 32]),
            membership_commitment: Hash32::new([5u8; 32]),
            transition_policy: policy,
        }
    }

    fn certificate_message(
        identity: AmpTransitionIdentity,
        readable_state_destroyed: bool,
    ) -> AmpTransitionCertificateMessage {
        AmpTransitionCertificateMessage {
            transition_id: identity.transition_id(),
            identity,
            committee_digest: Hash32::new([50u8; 32]),
            witness_payload_digest: Hash32::new([51u8; 32]),
            threshold: 1,
            fault_bound: 0,
            witness_signatures: vec![AmpTransitionWitnessSignature {
                witness: AuthorityId::new_from_entropy([52u8; 32]),
                signature: vec![1, 2, 3],
            }],
            excluded_authorities: BTreeSet::new(),
            readable_state_destroyed,
        }
    }
}
