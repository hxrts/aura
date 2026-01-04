//! Chat View Reduction
//!
//! Transforms chat-related journal facts into `ChatDelta` updates.
//! Delegates to `ChatViewReducer` from the `aura-chat` crate.

use crate::reactive::scheduler::ViewReduction;
use aura_chat::{ChatDelta, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_composition::{downcast_delta, ViewDelta, ViewDeltaReducer};
use aura_core::identifiers::AuthorityId;
use aura_journal::fact::{Fact, FactContent, RelationalFact};

/// Reduction adapter for chat view
///
/// Delegates to `ChatViewReducer` from `aura-chat` crate.
pub struct ChatReduction;

fn downcast_chat_deltas(view_deltas: Vec<ViewDelta>) -> Vec<ChatDelta> {
    view_deltas
        .into_iter()
        .filter_map(|vd| downcast_delta::<ChatDelta>(&vd).cloned())
        .collect()
}

impl ViewReduction<ChatDelta> for ChatReduction {
    fn reduce(&self, facts: &[Fact], own_authority: Option<AuthorityId>) -> Vec<ChatDelta> {
        let reducer = ChatViewReducer;

        facts
            .iter()
            .flat_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::Generic {
                    binding_type,
                    binding_data,
                    ..
                }) if binding_type == CHAT_FACT_TYPE_ID => {
                    // Use the domain reducer and downcast back to ChatDelta
                    let view_deltas =
                        reducer.reduce_fact(binding_type, binding_data, own_authority);
                    downcast_chat_deltas(view_deltas)
                }
                _ => Vec::new(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_chat::ChatFact;
    use aura_composition::IntoViewDelta;
    use aura_core::identifiers::{ChannelId, ContextId};
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_journal::DomainFact;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([0u8; 32])
    }

    fn make_test_fact(order_index: u64, content: FactContent) -> Fact {
        let mut order_bytes = [0u8; 32];
        order_bytes[..8].copy_from_slice(&order_index.to_be_bytes());
        let order = OrderTime(order_bytes);
        let timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000 + order_index,
            uncertainty: None,
        });
        Fact::new(order, timestamp, content)
    }

    #[test]
    fn test_chat_reduction() {
        let reduction = ChatReduction;

        let channel_fact = ChatFact::channel_created_ms(
            test_context_id(),
            ChannelId::default(),
            "general".to_string(),
            Some("General discussion".to_string()),
            false,
            1234567890,
            AuthorityId::new_from_entropy([1u8; 32]),
        );

        let facts = vec![make_test_fact(
            1,
            FactContent::Relational(RelationalFact::Generic {
                context_id: test_context_id(),
                binding_type: CHAT_FACT_TYPE_ID.to_string(),
                binding_data: channel_fact.to_bytes(),
            }),
        )];

        let test_authority = Some(AuthorityId::new_from_entropy([99u8; 32]));
        let deltas = reduction.reduce(&facts, test_authority);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(&deltas[0], ChatDelta::ChannelAdded { name, .. } if name == "general"));
    }

    #[test]
    fn test_downcast_preserves_all_deltas() {
        let view_deltas = vec![
            ChatDelta::ChannelAdded {
                channel_id: "chan".to_string(),
                name: "general".to_string(),
                topic: None,
                is_dm: false,
                member_count: 1,
                created_at: 1,
                creator_id: "creator".to_string(),
            }
            .into_view_delta(),
            ChatDelta::MessageAdded {
                channel_id: "chan".to_string(),
                message_id: "msg".to_string(),
                sender_id: "sender".to_string(),
                sender_name: "Alice".to_string(),
                content: "Hello".to_string(),
                timestamp: 2,
                reply_to: None,
                epoch_hint: Some(1),
            }
            .into_view_delta(),
        ];

        let deltas = downcast_chat_deltas(view_deltas);
        assert_eq!(deltas.len(), 2);
    }
}
