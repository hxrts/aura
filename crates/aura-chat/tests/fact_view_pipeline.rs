//! Chat fact → view reduction pipeline integration tests.
//!
//! Exercises the public API: create channel/message facts, reduce through
//! the view reducer, and verify the resulting deltas.

use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::view_delta::{downcast_delta, ViewDeltaReducer};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::DomainFact;

fn test_context() -> ContextId {
    ContextId::new_from_entropy([1u8; 32])
}

fn test_authority() -> AuthorityId {
    AuthorityId::new_from_entropy([3u8; 32])
}

/// Channel creation fact produces a ChannelAdded delta through the
/// public view reducer API.
#[test]
fn channel_creation_produces_view_delta() {
    let reducer = ChatViewReducer;
    assert_eq!(reducer.handles_type(), CHAT_FACT_TYPE_ID);

    let fact = ChatFact::channel_created_ms(
        test_context(),
        ChannelId::default(),
        "general".to_string(),
        Some("General discussion".to_string()),
        false,
        1000,
        test_authority(),
    );

    let bytes = fact.to_bytes();
    let deltas = reducer.reduce_fact(CHAT_FACT_TYPE_ID, &bytes, None);

    assert_eq!(deltas.len(), 1);
    let chat_delta = downcast_delta::<ChatDelta>(&deltas[0]).expect("should be ChatDelta");
    match chat_delta {
        ChatDelta::ChannelAdded { name, topic, .. } => {
            assert_eq!(name, "general");
            assert_eq!(topic, &Some("General discussion".to_string()));
        }
        other => panic!("Expected ChannelAdded, got {:?}", other),
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
