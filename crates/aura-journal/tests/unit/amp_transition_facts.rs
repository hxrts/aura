use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::Hash32;
use aura_journal::fact::{
    AmpEmergencyAlarm, AmpTransitionAbort, AmpTransitionIdentity, AmpTransitionPolicy,
    AmpTransitionSuppressionScope, AmpTransitionWitnessSignature, CertifiedChannelEpochBump,
    ChannelBumpReason, FinalizedChannelEpochBump, ProposedChannelEpochBump,
    ProtocolRelationalFact,
};
use std::collections::BTreeSet;

fn context() -> ContextId {
    ContextId::new_from_entropy([1u8; 32])
}

fn channel() -> ChannelId {
    ChannelId::from_bytes([2u8; 32])
}

#[test]
fn amp_transition_identity_is_deterministic_and_policy_bound() {
    let identity = AmpTransitionIdentity {
        context: context(),
        channel: channel(),
        parent_epoch: 3,
        parent_commitment: Hash32::new([4u8; 32]),
        successor_epoch: 4,
        successor_commitment: Hash32::new([5u8; 32]),
        membership_commitment: Hash32::new([6u8; 32]),
        transition_policy: AmpTransitionPolicy::NormalTransition,
    };
    let same = identity.clone();
    let mut different_policy = identity.clone();
    different_policy.transition_policy = AmpTransitionPolicy::EmergencyQuarantineTransition;

    assert_eq!(identity.transition_id(), same.transition_id());
    assert_ne!(identity.transition_id(), different_policy.transition_id());
}

#[test]
fn proposed_epoch_bump_binds_transition_identity() {
    let proposal = ProposedChannelEpochBump::new(
        context(),
        channel(),
        0,
        1,
        Hash32::new([9u8; 32]),
        ChannelBumpReason::SuspiciousActivity,
    );

    assert_eq!(
        proposal.transition_policy,
        AmpTransitionPolicy::EmergencyQuarantineTransition
    );
    assert_eq!(proposal.transition_id, proposal.transition_identity().transition_id());
}

#[test]
fn certified_transition_protocol_key_uses_transition_id() {
    let identity = AmpTransitionIdentity {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: Hash32::new([3u8; 32]),
        successor_epoch: 1,
        successor_commitment: Hash32::new([4u8; 32]),
        membership_commitment: Hash32::new([5u8; 32]),
        transition_policy: AmpTransitionPolicy::AdditiveTransition,
    };
    let transition_id = identity.transition_id();
    let cert = CertifiedChannelEpochBump {
        identity,
        transition_id,
        witness_payload_digest: Hash32::new([10u8; 32]),
        committee_digest: Hash32::new([6u8; 32]),
        threshold: 2,
        fault_bound: 1,
        coord_epoch: Some(7),
        generation_min: Some(0),
        generation_max: Some(64),
        witness_signatures: vec![AmpTransitionWitnessSignature {
            witness: AuthorityId::new_from_entropy([8u8; 32]),
            signature: vec![1, 2, 3],
        }],
        equivocation_refs: BTreeSet::new(),
        excluded_authorities: BTreeSet::new(),
        readable_state_destroyed: false,
    };
    let key =
        ProtocolRelationalFact::AmpCertifiedChannelEpochBump(cert.clone()).binding_key();

    assert_eq!(
        ProtocolRelationalFact::AmpCertifiedChannelEpochBump(cert).context_id(),
        context()
    );
    assert_eq!(key.sub_type(), "amp-certified-epoch-bump");
    assert!(!key.data().is_empty());
}

#[test]
fn emergency_transition_facts_bind_exclusions_and_cryptoshred_metadata() {
    let suspect = AuthorityId::new_from_entropy([12u8; 32]);
    let identity = AmpTransitionIdentity {
        context: context(),
        channel: channel(),
        parent_epoch: 8,
        parent_commitment: Hash32::new([13u8; 32]),
        successor_epoch: 9,
        successor_commitment: Hash32::new([14u8; 32]),
        membership_commitment: Hash32::new([15u8; 32]),
        transition_policy: AmpTransitionPolicy::EmergencyCryptoshredTransition,
    };
    let transition_id = identity.transition_id();
    let excluded_authorities = BTreeSet::from([suspect]);
    let cert = CertifiedChannelEpochBump {
        identity: identity.clone(),
        transition_id,
        witness_payload_digest: Hash32::new([16u8; 32]),
        committee_digest: Hash32::new([17u8; 32]),
        threshold: 3,
        fault_bound: 1,
        coord_epoch: None,
        generation_min: None,
        generation_max: None,
        witness_signatures: vec![AmpTransitionWitnessSignature {
            witness: AuthorityId::new_from_entropy([18u8; 32]),
            signature: vec![4, 5, 6],
        }],
        equivocation_refs: BTreeSet::new(),
        excluded_authorities: excluded_authorities.clone(),
        readable_state_destroyed: true,
    };
    let finalized = FinalizedChannelEpochBump {
        identity,
        transition_id,
        consensus_id: Hash32::new([19u8; 32]),
        transcript_ref: Some(Hash32::new([20u8; 32])),
        excluded_authorities,
        readable_state_destroyed: true,
    };

    assert!(cert.excluded_authorities.contains(&suspect));
    assert!(cert.readable_state_destroyed);
    assert_eq!(
        ProtocolRelationalFact::AmpFinalizedChannelEpochBump(finalized)
            .binding_key()
            .sub_type(),
        "amp-finalized-epoch-bump"
    );
}

#[test]
fn emergency_alarm_and_abort_are_context_scoped_protocol_facts() {
    let alarm = AmpEmergencyAlarm {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: Hash32::new([3u8; 32]),
        suspect: AuthorityId::new_from_entropy([4u8; 32]),
        raised_by: AuthorityId::new_from_entropy([5u8; 32]),
        evidence_id: Hash32::new([6u8; 32]),
    };
    let abort = AmpTransitionAbort {
        context: context(),
        channel: channel(),
        parent_epoch: 0,
        parent_commitment: Hash32::new([3u8; 32]),
        transition_id: Hash32::new([7u8; 32]),
        evidence_id: Hash32::new([8u8; 32]),
        scope: AmpTransitionSuppressionScope::A2LiveOnly,
    };

    assert_eq!(
        ProtocolRelationalFact::AmpEmergencyAlarm(alarm).context_id(),
        context()
    );
    assert_eq!(
        ProtocolRelationalFact::AmpTransitionAbort(abort).binding_key().sub_type(),
        "amp-transition-abort"
    );
}
