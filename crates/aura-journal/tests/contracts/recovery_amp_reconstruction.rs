//! Recovery AMP tests for journal state reconstruction.

use aura_core::time::{OrderTime, TimeStamp};
use aura_core::Hash32;
use aura_core::{ChannelId, ContextId};
use aura_journal::fact::{ChannelCheckpoint, CommittedChannelEpochBump, RelationalFact};
use aura_journal::ProtocolRelationalFact;
use aura_journal::{reduce_context, Fact, FactContent, FactJournal as Journal, JournalNamespace};

#[test]
fn recovery_from_journal_reconstructs_channel_state() {
    let ctx = ContextId::new_from_entropy([21u8; 32]);
    let channel = ChannelId::from_bytes([5u8; 32]);

    let checkpoint = Fact::new(
        OrderTime([1u8; 32]),
        TimeStamp::OrderClock(OrderTime([1u8; 32])),
        FactContent::Relational(RelationalFact::Protocol(
            ProtocolRelationalFact::AmpChannelCheckpoint(ChannelCheckpoint {
                context: ctx,
                channel,
                chan_epoch: 0,
                base_gen: 42,
                window: 32,
                ck_commitment: Hash32::new([7u8; 32]),
                skip_window_override: Some(32),
            }),
        )),
    );

    let proposal = aura_journal::fact::ProposedChannelEpochBump::new(
        ctx,
        channel,
        0,
        1,
        Hash32::new([9u8; 32]),
        aura_journal::fact::ChannelBumpReason::Routine,
    );
    let committed = Fact::new(
        OrderTime([2u8; 32]),
        TimeStamp::OrderClock(OrderTime([2u8; 32])),
        FactContent::Relational(RelationalFact::Protocol(
            ProtocolRelationalFact::AmpCommittedChannelEpochBump(
                CommittedChannelEpochBump::from_proposal(&proposal, Hash32::new([8u8; 32]), None),
            ),
        )),
    );

    let mut journal = Journal::new(JournalNamespace::Context(ctx));
    journal.add_fact(checkpoint).unwrap();
    journal.add_fact(committed).unwrap();

    let state = reduce_context(&journal).unwrap();
    let ch_state = state.channel_epochs.get(&channel).unwrap();
    assert_eq!(ch_state.chan_epoch, 1);
    assert_eq!(ch_state.last_checkpoint_gen, 42);
    assert_eq!(ch_state.skip_window, 32);
}
