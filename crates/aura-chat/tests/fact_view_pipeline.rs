//! Chat fact → view reduction pipeline integration tests.
//!
//! Exercises the public API: create channel/message facts, reduce through
//! the view reducer, and verify the resulting deltas.

#[path = "support.rs"]
mod common;

use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::view_delta::{downcast_delta, ViewDeltaReducer};
use aura_journal::DomainFact;

/// Channel creation fact produces a ChannelAdded delta through the
/// public view reducer API.
#[test]
fn channel_creation_produces_view_delta() {
    let reducer = ChatViewReducer;
    assert_eq!(reducer.handles_type(), CHAT_FACT_TYPE_ID);

    let fact = ChatFact::channel_created_ms(
        common::test_context_id(1),
        common::test_channel_id(0),
        "general".to_string(),
        Some("General discussion".to_string()),
        false,
        1000,
        common::test_authority_id(3),
    );

    let bytes = fact.to_bytes();
    let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &bytes, None);

    assert_eq!(deltas.len(), 1);
    let Some(chat_delta) = downcast_delta::<ChatDelta>(&deltas[0]) else {
        panic!("expected ChatDelta");
    };
    match chat_delta {
        ChatDelta::ChannelAdded { name, topic, .. } => {
            assert_eq!(name, "general");
            assert_eq!(topic, &Some("General discussion".to_string()));
        }
        other => panic!("Expected ChannelAdded, got {other:?}"),
    }
}

/// Operation categories: send-message is A, create-group is C.
#[test]
fn operation_categories_are_consistent() {
    assert_eq!(
        aura_chat::operation_category("chat:send-message"),
        Some("A")
    );
    assert_eq!(
        aura_chat::operation_category("chat:create-group"),
        Some("C")
    );
}
