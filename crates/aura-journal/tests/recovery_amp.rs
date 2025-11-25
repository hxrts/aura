//! Recovery AMP tests for journal state reconstruction.

use aura_core::time::{OrderTime, TimeStamp};
use aura_core::Hash32;
use aura_core::{ChannelId, ContextId};
use aura_journal::fact::{ChannelCheckpoint, CommittedChannelEpochBump, RelationalFact};
use aura_journal::{reduce_context, Fact, FactContent, FactJournal as Journal, JournalNamespace};

#[test]
fn recovery_from_journal_reconstructs_channel_state() {
    let ctx = ContextId::new();
    let channel = ChannelId::from_bytes([5u8; 32]);

    let checkpoint = Fact {
        order: OrderTime([1u8; 32]),
        timestamp: TimeStamp::OrderClock(OrderTime([1u8; 32])),
        content: FactContent::Relational(RelationalFact::AmpChannelCheckpoint(ChannelCheckpoint {
            context: ctx,
            channel,
            chan_epoch: 0,
            base_gen: 42,
            window: 32,
            ck_commitment: Hash32::new([7u8; 32]),
            skip_window_override: Some(32),
        })),
    };

    let committed = Fact {
        order: OrderTime([2u8; 32]),
        timestamp: TimeStamp::OrderClock(OrderTime([2u8; 32])),
        content: FactContent::Relational(RelationalFact::AmpCommittedChannelEpochBump(
            CommittedChannelEpochBump {
                context: ctx,
                channel,
                parent_epoch: 0,
                new_epoch: 1,
                chosen_bump_id: Hash32::new([9u8; 32]),
                consensus_id: Hash32::new([8u8; 32]),
            },
        )),
    };

    let mut journal = Journal::new(JournalNamespace::Context(ctx));
    journal.add_fact(checkpoint).unwrap();
    journal.add_fact(committed).unwrap();

    let state = reduce_context(&journal);
    let ch_state = state.channel_epochs.get(&channel).unwrap(); // Test expectation
    assert_eq!(ch_state.chan_epoch, 1);
    assert_eq!(ch_state.last_checkpoint_gen, 42);
    assert_eq!(ch_state.skip_window, 32);
}
