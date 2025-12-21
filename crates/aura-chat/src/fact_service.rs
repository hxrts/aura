//! Fact-first chat service
//!
//! Provides guard-compatible operations that emit `ChatFact` instances and
//! return explicit effect commands for an async interpreter to execute.

use aura_core::identifiers::ChannelId;

use crate::facts::ChatFact;
use crate::guards::{check_capability, check_flow_budget, costs, EffectCommand, GuardOutcome, GuardSnapshot};

/// Guard-compatible fact-first chat operations.
#[derive(Debug, Clone, Default)]
pub struct ChatFactService;

impl ChatFactService {
    /// Create a new fact-first chat service.
    pub fn new() -> Self {
        Self
    }

    /// Prepare a channel creation fact.
    pub fn prepare_create_channel(
        &self,
        snapshot: &GuardSnapshot,
        channel_id: ChannelId,
        name: String,
        topic: Option<String>,
        is_dm: bool,
    ) -> GuardOutcome {
        if let Some(outcome) = check_capability(snapshot, costs::CAP_CHAT_CHANNEL_CREATE) {
            return outcome;
        }
        if let Some(outcome) = check_flow_budget(snapshot, costs::CHAT_CHANNEL_CREATE_COST) {
            return outcome;
        }

        let fact = ChatFact::channel_created_ms(
            snapshot.context_id,
            channel_id,
            name,
            topic,
            is_dm,
            snapshot.now_ms,
            snapshot.authority_id,
        );

        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::CHAT_CHANNEL_CREATE_COST,
            },
            EffectCommand::JournalAppend { fact },
        ])
    }

    /// Prepare a message-sent fact with an opaque payload.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_send_message_sealed(
        &self,
        snapshot: &GuardSnapshot,
        channel_id: ChannelId,
        message_id: String,
        sender_name: String,
        payload: Vec<u8>,
        reply_to: Option<String>,
    ) -> GuardOutcome {
        if let Some(outcome) = check_capability(snapshot, costs::CAP_CHAT_MESSAGE_SEND) {
            return outcome;
        }
        if let Some(outcome) = check_flow_budget(snapshot, costs::CHAT_MESSAGE_SEND_COST) {
            return outcome;
        }

        let fact = ChatFact::message_sent_sealed_ms(
            snapshot.context_id,
            channel_id,
            message_id,
            snapshot.authority_id,
            sender_name,
            payload,
            snapshot.now_ms,
            reply_to,
        );

        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::CHAT_MESSAGE_SEND_COST,
            },
            EffectCommand::JournalAppend { fact },
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AuthorityId, ContextId};

    #[test]
    fn denied_when_missing_capability() {
        let service = ChatFactService::new();
        let snapshot = GuardSnapshot::new(
            AuthorityId::new_from_entropy([1u8; 32]),
            ContextId::new_from_entropy([2u8; 32]),
            10,
            vec![],
            123,
        );

        let out = service.prepare_create_channel(
            &snapshot,
            ChannelId::default(),
            "general".into(),
            None,
            false,
        );
        assert!(matches!(out.decision, crate::guards::GuardDecision::Deny { .. }));
    }

    #[test]
    fn approved_orders_budget_before_journal_append() {
        let service = ChatFactService::new();
        let snapshot = GuardSnapshot::new(
            AuthorityId::new_from_entropy([1u8; 32]),
            ContextId::new_from_entropy([2u8; 32]),
            10,
            vec![costs::CAP_CHAT_MESSAGE_SEND.to_string()],
            123,
        );

        let out = service.prepare_send_message_sealed(
            &snapshot,
            ChannelId::default(),
            "msg-1".to_string(),
            "Alice".to_string(),
            vec![1, 2, 3],
            None,
        );

        assert!(matches!(out.decision, crate::guards::GuardDecision::Allow));
        assert!(matches!(
            out.effects.as_slice(),
            [EffectCommand::ChargeFlowBudget { .. }, EffectCommand::JournalAppend { .. }]
        ));
    }
}
