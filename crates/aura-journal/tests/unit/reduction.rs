use aura_core::time::{OrderTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::Hash32;
use aura_journal::fact::{
    AmpEmergencyAlarm, AmpTransitionAbort, AmpTransitionConflict, AmpTransitionIdentity,
    AmpTransitionPolicy, AmpTransitionSupersession, AmpTransitionSuppressionScope,
    AmpTransitionWitnessSignature, CertifiedChannelEpochBump, ChannelBumpReason, ChannelCheckpoint,
    FinalizedChannelEpochBump, ProposedChannelEpochBump,
};
use aura_journal::reduction::{
    can_prune_checkpoint, can_prune_proposed_bump, compute_checkpoint_pruning_boundary,
    reduce_authority, reduce_context, AmpTransitionReductionStatus, ReductionNamespaceError,
    RelationalBindingType,
};
use aura_journal::{
    Fact, FactAttestedOp, FactContent, FactJournal, JournalNamespace, RelationalFact, TreeOpKind,
};
use std::collections::BTreeSet;

#[test]
fn reduce_empty_authority_journal() {
    let auth_id = AuthorityId::new_from_entropy([13u8; 32]);
    let journal = FactJournal::new(JournalNamespace::Authority(auth_id));

    let state = reduce_authority(&journal).unwrap();
    assert_eq!(state.facts.len(), 0);
}

#[test]
fn reduce_context_with_bindings() {
    let ctx_id = ContextId::new_from_entropy([14u8; 32]);
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx_id));

    let fact = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::GuardianBinding {
                account_id: AuthorityId::new_from_entropy([15u8; 32]),
                guardian_id: AuthorityId::new_from_entropy([16u8; 32]),
                binding_hash: Hash32::default(),
            },
        )),
    );

    journal.add_fact(fact).unwrap();

    let state = reduce_context(&journal).unwrap();
    assert_eq!(state.bindings.len(), 1);
    assert!(matches!(
        state.bindings[0].binding_type,
        RelationalBindingType::GuardianBinding { .. }
    ));
}

#[test]
fn amp_routine_bump_respects_spacing_rule() {
    let ctx_id = ContextId::new_from_entropy([17u8; 32]);
    let channel = ChannelId::from_bytes([1u8; 32]);
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx_id));

    let checkpoint = ChannelCheckpoint {
        context: ctx_id,
        channel,
        chan_epoch: 0,
        base_gen: 0,
        window: 1024,
        ck_commitment: Hash32::default(),
        skip_window_override: None,
    };
    let proposed = ProposedChannelEpochBump::new(
        ctx_id,
        channel,
        0,
        1,
        Hash32::new([2u8; 32]),
        ChannelBumpReason::Routine,
    );

    journal
        .add_fact(Fact::new(
            OrderTime([9u8; 32]),
            TimeStamp::OrderClock(OrderTime([9u8; 32])),
            FactContent::Relational(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
            )),
        ))
        .unwrap();
    journal
        .add_fact(Fact::new(
            OrderTime([10u8; 32]),
            TimeStamp::OrderClock(OrderTime([10u8; 32])),
            FactContent::Relational(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposed),
            )),
        ))
        .unwrap();

    let state = reduce_context(&journal).unwrap();
    let ch_state = state.channel_epochs.get(&channel).unwrap();
    assert!(ch_state.pending_bump.is_none());
    assert_eq!(ch_state.chan_epoch, 0);
    assert_eq!(ch_state.skip_window, 1024);
}

#[test]
fn amp_emergency_bump_remains_observed_before_a2_certificate() {
    let ctx_id = ContextId::new_from_entropy([18u8; 32]);
    let channel = ChannelId::from_bytes([3u8; 32]);
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx_id));

    let checkpoint = ChannelCheckpoint {
        context: ctx_id,
        channel,
        chan_epoch: 0,
        base_gen: 0,
        window: 1024,
        ck_commitment: Hash32::default(),
        skip_window_override: None,
    };
    let emergency = ProposedChannelEpochBump::new(
        ctx_id,
        channel,
        0,
        1,
        Hash32::new([4u8; 32]),
        ChannelBumpReason::SuspiciousActivity,
    );

    journal
        .add_fact(Fact::new(
            OrderTime([11u8; 32]),
            TimeStamp::OrderClock(OrderTime([11u8; 32])),
            FactContent::Relational(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
            )),
        ))
        .unwrap();
    journal
        .add_fact(Fact::new(
            OrderTime([12u8; 32]),
            TimeStamp::OrderClock(OrderTime([12u8; 32])),
            FactContent::Relational(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(emergency),
            )),
        ))
        .unwrap();

    let state = reduce_context(&journal).unwrap();
    let ch_state = state.channel_epochs.get(&channel).unwrap();
    assert!(ch_state.pending_bump.is_none());
    let transition = ch_state.transition.as_ref().unwrap();
    assert_eq!(transition.status, AmpTransitionReductionStatus::Observed);
    assert_eq!(transition.observed_transition_ids.len(), 1);
}

#[test]
fn reduce_wrong_namespace_type() {
    let ctx_id = ContextId::new_from_entropy([19u8; 32]);
    let journal = FactJournal::new(JournalNamespace::Context(ctx_id));
    let result = reduce_authority(&journal);
    assert!(matches!(
        result,
        Err(ReductionNamespaceError::ContextAsAuthority)
    ));
}

#[test]
fn reduce_authority_multiple_add_leafs_with_same_order() {
    let auth_id = AuthorityId::new_from_entropy([21u8; 32]);
    let mut journal = FactJournal::new(JournalNamespace::Authority(auth_id));
    let shared_order = OrderTime([42u8; 32]);

    let add_leaf_1 = Fact::new(
        shared_order,
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::AttestedOp(FactAttestedOp {
            tree_op: TreeOpKind::AddLeaf {
                public_key: vec![1u8; 32],
                role: aura_core::tree::LeafRole::Device,
            },
            parent_commitment: Hash32::default(),
            new_commitment: Hash32::new([3u8; 32]),
            witness_threshold: 1,
            signature: vec![0xAA],
        }),
    );
    let add_leaf_2 = Fact::new(
        OrderTime([42u8; 32]),
        TimeStamp::OrderClock(OrderTime([2u8; 32])),
        FactContent::AttestedOp(FactAttestedOp {
            tree_op: TreeOpKind::AddLeaf {
                public_key: vec![2u8; 32],
                role: aura_core::tree::LeafRole::Device,
            },
            parent_commitment: Hash32::new([3u8; 32]),
            new_commitment: Hash32::new([4u8; 32]),
            witness_threshold: 1,
            signature: vec![0xBB],
        }),
    );

    journal.add_fact(add_leaf_1).unwrap();
    journal.add_fact(add_leaf_2).unwrap();

    assert_eq!(journal.facts.len(), 2);
    let state = reduce_authority(&journal).unwrap();
    assert_eq!(state.tree_state.device_count(), 2);
}

#[test]
fn amp_reduction_order_independent() {
    let ctx = ContextId::new_from_entropy([20u8; 32]);
    let channel = ChannelId::from_bytes([7u8; 32]);

    let checkpoint = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(ChannelCheckpoint {
                context: ctx,
                channel,
                chan_epoch: 0,
                base_gen: 10,
                window: 16,
                ck_commitment: Hash32::new([8u8; 32]),
                skip_window_override: Some(16),
            }),
        )),
    );
    let proposed = Fact::new(
        OrderTime([2u8; 32]),
        TimeStamp::OrderClock(OrderTime([2u8; 32])),
        FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(
                ProposedChannelEpochBump::new(
                    ctx,
                    channel,
                    0,
                    1,
                    Hash32::new([9u8; 32]),
                    ChannelBumpReason::Routine,
                ),
            ),
        )),
    );

    let mut journal_a = FactJournal::new(JournalNamespace::Context(ctx));
    journal_a.add_fact(checkpoint.clone()).unwrap();
    journal_a.add_fact(proposed.clone()).unwrap();

    let mut journal_b = FactJournal::new(JournalNamespace::Context(ctx));
    journal_b.add_fact(proposed).unwrap();
    journal_b.add_fact(checkpoint).unwrap();

    let state_a = reduce_context(&journal_a).unwrap();
    let state_b = reduce_context(&journal_b).unwrap();
    assert_eq!(
        state_a.channel_epochs.get(&channel),
        state_b.channel_epochs.get(&channel)
    );
}

#[test]
fn amp_single_a2_certificate_exposes_one_live_successor() {
    let ctx = ContextId::new_from_entropy([51u8; 32]);
    let channel = ChannelId::from_bytes([52u8; 32]);
    let proposal = ProposedChannelEpochBump::new(
        ctx,
        channel,
        0,
        1,
        Hash32::new([53u8; 32]),
        ChannelBumpReason::Routine,
    );
    let cert = certified_bump(proposal.transition_identity(), 2, [54u8; 32]);
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx));
    add_protocol_fact(
        &mut journal,
        1,
        aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposal.clone()),
    );
    add_protocol_fact(
        &mut journal,
        2,
        aura_journal::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(cert),
    );

    let state = reduce_context(&journal).unwrap();
    let ch_state = state.channel_epochs.get(&channel).unwrap();
    let transition = ch_state.transition.as_ref().unwrap();

    assert_eq!(transition.status, AmpTransitionReductionStatus::A2Live);
    assert_eq!(transition.live_transition_id, Some(proposal.transition_id));
    assert_eq!(
        ch_state.pending_bump.as_ref().unwrap().transition_id,
        proposal.transition_id
    );
}

#[test]
fn amp_conflicting_a2_certificates_expose_no_live_successor() {
    let ctx = ContextId::new_from_entropy([61u8; 32]);
    let channel = ChannelId::from_bytes([62u8; 32]);
    let left = transition_identity(ctx, channel, 0, 1, [63u8; 32]);
    let right = transition_identity(ctx, channel, 0, 1, [64u8; 32]);
    let left_id = left.transition_id();
    let right_id = right.transition_id();
    let conflict = AmpTransitionConflict {
        context: ctx,
        channel,
        parent_epoch: 0,
        parent_commitment: Hash32::new([40u8; 32]),
        first_transition_id: left_id,
        second_transition_id: right_id,
        equivocating_witness: Some(AuthorityId::new_from_entropy([65u8; 32])),
        evidence_id: Hash32::new([66u8; 32]),
    };
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx));
    add_protocol_fact(
        &mut journal,
        1,
        aura_journal::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(certified_bump(
            left, 2, [67u8; 32],
        )),
    );
    add_protocol_fact(
        &mut journal,
        2,
        aura_journal::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(certified_bump(
            right, 2, [68u8; 32],
        )),
    );
    add_protocol_fact(
        &mut journal,
        3,
        aura_journal::ProtocolRelationalFact::AmpTransitionConflict(conflict),
    );

    let state = reduce_context(&journal).unwrap();
    let transition = state
        .amp_transitions
        .values()
        .find(|transition| transition.parent.channel == channel)
        .unwrap();

    assert_eq!(transition.status, AmpTransitionReductionStatus::A2Conflict);
    assert!(transition.live_transition_id.is_none());
    assert!(state
        .channel_epochs
        .get(&channel)
        .unwrap()
        .pending_bump
        .is_none());
}

#[test]
fn amp_conflict_replay_is_order_independent() {
    let ctx = ContextId::new_from_entropy([69u8; 32]);
    let channel = ChannelId::from_bytes([70u8; 32]);
    let left = transition_identity(ctx, channel, 0, 1, [75u8; 32]);
    let right = transition_identity(ctx, channel, 0, 1, [76u8; 32]);
    let left_fact = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(certified_bump(
                left, 2, [77u8; 32],
            )),
        )),
    );
    let right_fact = Fact::new(
        OrderTime([2u8; 32]),
        TimeStamp::OrderClock(OrderTime([2u8; 32])),
        FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(certified_bump(
                right, 2, [78u8; 32],
            )),
        )),
    );
    let mut journal_a = FactJournal::new(JournalNamespace::Context(ctx));
    journal_a.add_fact(left_fact.clone()).unwrap();
    journal_a.add_fact(right_fact.clone()).unwrap();
    let mut journal_b = FactJournal::new(JournalNamespace::Context(ctx));
    journal_b.add_fact(right_fact).unwrap();
    journal_b.add_fact(left_fact).unwrap();

    let state_a = reduce_context(&journal_a).unwrap();
    let state_b = reduce_context(&journal_b).unwrap();

    assert_eq!(state_a.amp_transitions, state_b.amp_transitions);
    assert_eq!(
        state_a.amp_transitions.values().next().unwrap().status,
        AmpTransitionReductionStatus::A2Conflict
    );
}

#[test]
fn amp_a3_finalization_advances_durable_epoch() {
    let ctx = ContextId::new_from_entropy([71u8; 32]);
    let channel = ChannelId::from_bytes([72u8; 32]);
    let identity = transition_identity(ctx, channel, 0, 1, [73u8; 32]);
    let transition_id = identity.transition_id();
    let finalized = FinalizedChannelEpochBump {
        identity: identity.clone(),
        transition_id,
        consensus_id: Hash32::new([74u8; 32]),
        transcript_ref: None,
        excluded_authorities: BTreeSet::new(),
        readable_state_destroyed: false,
    };
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx));
    add_protocol_fact(
        &mut journal,
        1,
        aura_journal::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(certified_bump(
            identity, 2, [99u8; 32],
        )),
    );
    add_protocol_fact(
        &mut journal,
        2,
        aura_journal::ProtocolRelationalFact::AmpFinalizedChannelEpochBump(finalized),
    );

    let state = reduce_context(&journal).unwrap();
    let ch_state = state.channel_epochs.get(&channel).unwrap();
    let transition = state
        .amp_transitions
        .values()
        .find(|transition| transition.parent.channel == channel)
        .unwrap();

    assert_eq!(ch_state.chan_epoch, 1);
    assert_eq!(transition.status, AmpTransitionReductionStatus::A3Finalized);
    assert_eq!(transition.finalized_transition_id, Some(transition_id));
}

#[test]
fn amp_conflicting_a3_finalizations_expose_no_durable_successor() {
    let ctx = ContextId::new_from_entropy([79u8; 32]);
    let channel = ChannelId::from_bytes([80u8; 32]);
    let left = transition_identity(ctx, channel, 0, 1, [81u8; 32]);
    let right = transition_identity(ctx, channel, 0, 1, [82u8; 32]);
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx));
    add_protocol_fact(
        &mut journal,
        1,
        aura_journal::ProtocolRelationalFact::AmpFinalizedChannelEpochBump(finalized_bump(
            left, [83u8; 32],
        )),
    );
    add_protocol_fact(
        &mut journal,
        2,
        aura_journal::ProtocolRelationalFact::AmpFinalizedChannelEpochBump(finalized_bump(
            right, [84u8; 32],
        )),
    );

    let state = reduce_context(&journal).unwrap();
    let transition = state.amp_transitions.values().next().unwrap();

    assert_eq!(transition.status, AmpTransitionReductionStatus::A3Conflict);
    assert!(transition.finalized_transition_id.is_none());
}

#[test]
fn amp_abort_and_supersession_suppress_observed_transition() {
    let ctx = ContextId::new_from_entropy([85u8; 32]);
    let channel = ChannelId::from_bytes([86u8; 32]);
    let proposal = ProposedChannelEpochBump::new(
        ctx,
        channel,
        0,
        1,
        Hash32::new([87u8; 32]),
        ChannelBumpReason::Routine,
    );
    let mut aborted = FactJournal::new(JournalNamespace::Context(ctx));
    add_protocol_fact(
        &mut aborted,
        1,
        aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposal.clone()),
    );
    add_protocol_fact(
        &mut aborted,
        2,
        aura_journal::ProtocolRelationalFact::AmpTransitionAbort(AmpTransitionAbort {
            context: ctx,
            channel,
            parent_epoch: 0,
            parent_commitment: proposal.parent_commitment,
            transition_id: proposal.transition_id,
            evidence_id: Hash32::new([88u8; 32]),
            scope: AmpTransitionSuppressionScope::A2AndA3,
        }),
    );

    let aborted_state = reduce_context(&aborted).unwrap();
    assert_eq!(
        aborted_state
            .amp_transitions
            .values()
            .next()
            .unwrap()
            .status,
        AmpTransitionReductionStatus::Aborted
    );

    let successor = ProposedChannelEpochBump::new(
        ctx,
        channel,
        0,
        1,
        Hash32::new([89u8; 32]),
        ChannelBumpReason::SuspiciousActivity,
    );
    let mut superseded = FactJournal::new(JournalNamespace::Context(ctx));
    add_protocol_fact(
        &mut superseded,
        1,
        aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposal.clone()),
    );
    add_protocol_fact(
        &mut superseded,
        2,
        aura_journal::ProtocolRelationalFact::AmpTransitionSupersession(
            AmpTransitionSupersession {
                context: ctx,
                channel,
                parent_epoch: 0,
                parent_commitment: proposal.parent_commitment,
                superseded_transition_id: proposal.transition_id,
                superseding_transition_id: successor.transition_id,
                evidence_id: Hash32::new([90u8; 32]),
                scope: AmpTransitionSuppressionScope::A2AndA3,
            },
        ),
    );

    let superseded_state = reduce_context(&superseded).unwrap();
    assert_eq!(
        superseded_state
            .amp_transitions
            .values()
            .next()
            .unwrap()
            .status,
        AmpTransitionReductionStatus::Superseded
    );
}

#[test]
fn amp_emergency_reducer_exposes_suspect_quarantine_and_prune_metadata() {
    let ctx = ContextId::new_from_entropy([91u8; 32]);
    let channel = ChannelId::from_bytes([92u8; 32]);
    let suspect = AuthorityId::new_from_entropy([93u8; 32]);
    let quarantine = transition_identity_with_policy(
        ctx,
        channel,
        0,
        1,
        [94u8; 32],
        AmpTransitionPolicy::EmergencyQuarantineTransition,
    );
    let cryptoshred = transition_identity_with_policy(
        ctx,
        channel,
        0,
        1,
        [95u8; 32],
        AmpTransitionPolicy::EmergencyCryptoshredTransition,
    );
    let mut quarantine_cert = certified_bump(quarantine, 2, [96u8; 32]);
    quarantine_cert.excluded_authorities.insert(suspect);
    let mut cryptoshred_commit = finalized_bump(cryptoshred, [97u8; 32]);
    cryptoshred_commit.excluded_authorities.insert(suspect);
    cryptoshred_commit.readable_state_destroyed = true;
    let mut journal = FactJournal::new(JournalNamespace::Context(ctx));
    add_protocol_fact(
        &mut journal,
        1,
        aura_journal::ProtocolRelationalFact::AmpEmergencyAlarm(AmpEmergencyAlarm {
            context: ctx,
            channel,
            parent_epoch: 0,
            parent_commitment: Hash32::new([40u8; 32]),
            suspect,
            raised_by: AuthorityId::new_from_entropy([98u8; 32]),
            evidence_id: Hash32::new([99u8; 32]),
        }),
    );
    add_protocol_fact(
        &mut journal,
        2,
        aura_journal::ProtocolRelationalFact::AmpCertifiedChannelEpochBump(quarantine_cert),
    );
    add_protocol_fact(
        &mut journal,
        3,
        aura_journal::ProtocolRelationalFact::AmpFinalizedChannelEpochBump(cryptoshred_commit),
    );

    let state = reduce_context(&journal).unwrap();
    let transition = state.amp_transitions.values().next().unwrap();

    assert!(transition.emergency_suspects.contains(&suspect));
    assert!(transition.quarantine_epochs.contains(&1));
    assert!(transition.prune_before_epochs.contains(&0));
}

fn transition_identity(
    context: ContextId,
    channel: ChannelId,
    parent_epoch: u64,
    successor_epoch: u64,
    successor_commitment: [u8; 32],
) -> AmpTransitionIdentity {
    transition_identity_with_policy(
        context,
        channel,
        parent_epoch,
        successor_epoch,
        successor_commitment,
        AmpTransitionPolicy::NormalTransition,
    )
}

fn transition_identity_with_policy(
    context: ContextId,
    channel: ChannelId,
    parent_epoch: u64,
    successor_epoch: u64,
    successor_commitment: [u8; 32],
    transition_policy: AmpTransitionPolicy,
) -> AmpTransitionIdentity {
    AmpTransitionIdentity {
        context,
        channel,
        parent_epoch,
        parent_commitment: Hash32::new([40u8; 32]),
        successor_epoch,
        successor_commitment: Hash32::new(successor_commitment),
        membership_commitment: Hash32::new([41u8; 32]),
        transition_policy,
    }
}

fn finalized_bump(
    identity: AmpTransitionIdentity,
    consensus_seed: [u8; 32],
) -> FinalizedChannelEpochBump {
    FinalizedChannelEpochBump {
        transition_id: identity.transition_id(),
        identity,
        consensus_id: Hash32::new(consensus_seed),
        transcript_ref: None,
        excluded_authorities: BTreeSet::new(),
        readable_state_destroyed: false,
    }
}

fn certified_bump(
    identity: AmpTransitionIdentity,
    threshold: u16,
    payload_seed: [u8; 32],
) -> CertifiedChannelEpochBump {
    CertifiedChannelEpochBump {
        transition_id: identity.transition_id(),
        identity,
        witness_payload_digest: Hash32::new(payload_seed),
        committee_digest: Hash32::new([42u8; 32]),
        threshold,
        fault_bound: 1,
        coord_epoch: None,
        generation_min: None,
        generation_max: None,
        witness_signatures: vec![
            AmpTransitionWitnessSignature {
                witness: AuthorityId::new_from_entropy([43u8; 32]),
                signature: vec![1],
            },
            AmpTransitionWitnessSignature {
                witness: AuthorityId::new_from_entropy([44u8; 32]),
                signature: vec![2],
            },
        ],
        equivocation_refs: BTreeSet::new(),
        excluded_authorities: BTreeSet::new(),
        readable_state_destroyed: false,
    }
}

fn add_protocol_fact(
    journal: &mut FactJournal,
    order: u8,
    fact: aura_journal::ProtocolRelationalFact,
) {
    let result = journal.add_fact(Fact::new(
        OrderTime([order; 32]),
        TimeStamp::OrderClock(OrderTime([order; 32])),
        FactContent::Relational(RelationalFact::Protocol(fact)),
    ));
    assert!(
        result.is_ok(),
        "test protocol fact should be accepted into the journal: {result:?}"
    );
}

#[test]
fn checkpoint_pruning_boundary() {
    assert_eq!(compute_checkpoint_pruning_boundary(5000, None), 2440);
    assert_eq!(compute_checkpoint_pruning_boundary(5000, Some(512)), 3464);
    assert_eq!(compute_checkpoint_pruning_boundary(1000, None), 0);
}

#[test]
fn can_prune_checkpoint_boundary() {
    assert!(can_prune_checkpoint(1000, 5000, None));
    assert!(!can_prune_checkpoint(3000, 5000, None));
    assert!(!can_prune_checkpoint(2440, 5000, None));
    assert!(can_prune_checkpoint(2439, 5000, None));
}

#[test]
fn can_prune_proposed_bump_helper() {
    let committed = vec![(0, 1), (1, 2), (3, 4)];

    assert!(can_prune_proposed_bump(0, &committed));
    assert!(can_prune_proposed_bump(1, &committed));
    assert!(can_prune_proposed_bump(2, &committed));
    assert!(!can_prune_proposed_bump(4, &committed));
    assert!(!can_prune_proposed_bump(5, &committed));
}
