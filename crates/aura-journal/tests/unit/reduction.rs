use aura_core::time::{OrderTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::Hash32;
use aura_journal::fact::{ChannelBumpReason, ChannelCheckpoint, ProposedChannelEpochBump};
use aura_journal::reduction::{
    can_prune_checkpoint, can_prune_proposed_bump, compute_checkpoint_pruning_boundary,
    reduce_authority, reduce_context, ReductionNamespaceError, RelationalBindingType,
};
use aura_journal::{
    Fact, FactAttestedOp, FactContent, FactJournal, JournalNamespace, RelationalFact, TreeOpKind,
};

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
    let proposed = ProposedChannelEpochBump {
        context: ctx_id,
        channel,
        parent_epoch: 0,
        new_epoch: 1,
        bump_id: Hash32::new([2u8; 32]),
        reason: ChannelBumpReason::Routine,
    };

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
fn amp_emergency_bump_bypasses_spacing_rule() {
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
    let emergency = ProposedChannelEpochBump {
        context: ctx_id,
        channel,
        parent_epoch: 0,
        new_epoch: 1,
        bump_id: Hash32::new([4u8; 32]),
        reason: ChannelBumpReason::SuspiciousActivity,
    };

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
    let pending = ch_state.pending_bump.as_ref().unwrap();
    assert_eq!(pending.new_epoch, 1);
    assert_eq!(pending.reason, ChannelBumpReason::SuspiciousActivity);
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
                ProposedChannelEpochBump {
                    context: ctx,
                    channel,
                    parent_epoch: 0,
                    new_epoch: 1,
                    bump_id: Hash32::new([9u8; 32]),
                    reason: ChannelBumpReason::Routine,
                },
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
