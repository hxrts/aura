//! Tests for convergence certificates and reversion facts in relational reduction.

use aura_core::threshold::{ConvergenceCert, ReversionFact};
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::Hash32;
use aura_core::{AuthorityId, ContextId};
use aura_journal::fact::RelationalFact;
use aura_journal::reduction::RelationalBindingType;
use aura_journal::ProtocolRelationalFact;
use aura_journal::{reduce_context, Fact, FactContent, FactJournal as Journal, JournalNamespace};
use std::collections::BTreeSet;

#[test]
fn reduce_context_emits_convergence_and_reversion_bindings() {
    let ctx = ContextId::new_from_entropy([31u8; 32]);
    let op_id = Hash32::new([11u8; 32]);
    let prestate_hash = Hash32::new([12u8; 32]);
    let winner_op_id = Hash32::new([13u8; 32]);

    let cert = ConvergenceCert {
        context: ctx,
        op_id,
        prestate_hash,
        coord_epoch: 7,
        ack_set: Some(BTreeSet::from([AuthorityId::new_from_entropy([1u8; 32])])),
        window: 42,
    };

    let reversion = ReversionFact {
        context: ctx,
        op_id,
        winner_op_id,
        coord_epoch: 8,
    };

    let cert_fact = Fact {
        order: OrderTime([1u8; 32]),
        timestamp: TimeStamp::OrderClock(OrderTime([1u8; 32])),
        content: FactContent::Relational(RelationalFact::Protocol(
            ProtocolRelationalFact::ConvergenceCert(cert),
        )),
    };

    let revert_fact = Fact {
        order: OrderTime([2u8; 32]),
        timestamp: TimeStamp::OrderClock(OrderTime([2u8; 32])),
        content: FactContent::Relational(RelationalFact::Protocol(
            ProtocolRelationalFact::ReversionFact(reversion),
        )),
    };

    let mut journal = Journal::new(JournalNamespace::Context(ctx));
    journal.add_fact(cert_fact).unwrap();
    journal.add_fact(revert_fact).unwrap();

    let state = reduce_context(&journal).unwrap();

    assert!(state.bindings.iter().any(|binding| {
        matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref name) if name == "convergence_cert"
        ) && binding.context_id == ctx
            && binding.data == op_id.0.to_vec()
    }));

    assert!(state.bindings.iter().any(|binding| {
        matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref name) if name == "reversion_fact"
        ) && binding.context_id == ctx
            && binding.data == op_id.0.to_vec()
    }));
}
