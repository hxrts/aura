//! Fact-first chat service
//!
//! Provides guard-compatible operations that emit `ChatFact` instances and
//! return explicit effect commands for an async interpreter to execute.

use aura_core::types::identifiers::ChannelId;

use crate::capabilities::ChatCapability;
use crate::facts::ChatFact;
use crate::guards::{
    check_capability, check_flow_budget, check_membership, check_moderation, costs, EffectCommand,
    GuardOutcome, GuardSnapshot,
};

/// Guard-compatible fact-first chat operations.
#[derive(Debug, Clone, Default)]
pub struct ChatFactService;

impl ChatFactService {
    /// Create a new fact-first chat service.
    pub fn new() -> Self {
        Self
    }

    fn allowed_fact_append(cost: aura_core::FlowCost, fact: ChatFact) -> GuardOutcome {
        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget { cost },
            EffectCommand::JournalAppend { fact },
        ])
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
        if let Some(outcome) = check_capability(snapshot, &ChatCapability::ChannelCreate.as_name())
        {
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

        Self::allowed_fact_append(costs::CHAT_CHANNEL_CREATE_COST, fact)
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
        epoch_hint: Option<u32>,
    ) -> GuardOutcome {
        if let Some(outcome) = check_capability(snapshot, &ChatCapability::MessageSend.as_name()) {
            return outcome;
        }
        if let Some(outcome) = check_moderation(snapshot) {
            return outcome;
        }
        if let Some(outcome) = check_membership(snapshot, channel_id) {
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
            epoch_hint,
        );

        Self::allowed_fact_append(costs::CHAT_MESSAGE_SEND_COST, fact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_channel_id, test_guard_snapshot};
    use crate::{guards::MembershipSnapshot, guards::CHAT_MEMBERSHIP_SNAPSHOT_MAX_AGE_MS};
    use aura_core::FlowCost;

    fn current_membership_snapshot(
        snapshot: &GuardSnapshot,
        channel_id: ChannelId,
    ) -> MembershipSnapshot {
        MembershipSnapshot::new(
            snapshot.context_id,
            channel_id,
            snapshot.authority_id,
            true,
            snapshot.now_ms,
        )
    }

    /// Channel creation denied without required capability — chat operations
    /// are capability-gated.
    #[test]
    fn denied_when_missing_capability() {
        let service = ChatFactService::new();
        let snapshot = test_guard_snapshot(1, 2, FlowCost::new(10), vec![], 123);

        let out = service.prepare_create_channel(
            &snapshot,
            test_channel_id(0),
            "general".into(),
            None,
            false,
        );
        assert!(matches!(
            out.decision,
            crate::guards::GuardDecision::Deny { .. }
        ));
    }

    /// Budget charge precedes journal append in the effect list — enforces
    /// charge-before-send ordering.
    #[test]
    fn approved_orders_budget_before_journal_append() {
        let service = ChatFactService::new();
        let snapshot = test_guard_snapshot(
            1,
            2,
            FlowCost::new(10),
            vec![ChatCapability::MessageSend.as_name()],
            123,
        );
        let channel_id = test_channel_id(0);
        let membership = current_membership_snapshot(&snapshot, channel_id);
        let snapshot = snapshot.with_membership_snapshot(membership);

        let out = service.prepare_send_message_sealed(
            &snapshot,
            channel_id,
            "msg-1".to_string(),
            "Alice".to_string(),
            vec![1, 2, 3],
            None,
            Some(1), // epoch_hint for test
        );

        assert!(matches!(out.decision, crate::guards::GuardDecision::Allow));
        assert!(matches!(
            out.effects.as_slice(),
            [
                EffectCommand::ChargeFlowBudget { .. },
                EffectCommand::JournalAppend { .. }
            ]
        ));
    }

    #[test]
    fn send_denied_when_sender_is_muted_before_flow_budget() {
        let service = ChatFactService::new();
        let snapshot = test_guard_snapshot(
            5,
            6,
            FlowCost::new(10),
            vec![ChatCapability::MessageSend.as_name()],
            123,
        )
        .with_moderation_status(false, true);

        let out = service.prepare_send_message_sealed(
            &snapshot,
            test_channel_id(0),
            "message-1".into(),
            "you".into(),
            b"payload".to_vec(),
            None,
            None,
        );
        assert!(matches!(
            out.decision,
            crate::guards::GuardDecision::Deny { .. }
        ));
        assert!(out.effects.is_empty());
    }

    #[test]
    fn send_denied_without_membership_snapshot() {
        let service = ChatFactService::new();
        let snapshot = test_guard_snapshot(
            1,
            2,
            FlowCost::new(10),
            vec![ChatCapability::MessageSend.as_name()],
            123,
        );

        let out = service.prepare_send_message_sealed(
            &snapshot,
            test_channel_id(0),
            "msg-1".to_string(),
            "Alice".to_string(),
            vec![1, 2, 3],
            None,
            None,
        );

        assert!(matches!(
            out.decision,
            crate::guards::GuardDecision::Deny { .. }
        ));
    }

    #[test]
    fn send_denied_when_membership_snapshot_is_stale() {
        let service = ChatFactService::new();
        let snapshot = test_guard_snapshot(
            1,
            2,
            FlowCost::new(10),
            vec![ChatCapability::MessageSend.as_name()],
            100_000,
        );
        let channel_id = test_channel_id(0);
        let stale_observed_at = snapshot.now_ms - CHAT_MEMBERSHIP_SNAPSHOT_MAX_AGE_MS - 1;
        let membership = MembershipSnapshot::new(
            snapshot.context_id,
            channel_id,
            snapshot.authority_id,
            true,
            stale_observed_at,
        );
        let snapshot = snapshot.with_membership_snapshot(membership);

        let out = service.prepare_send_message_sealed(
            &snapshot,
            channel_id,
            "msg-1".to_string(),
            "Alice".to_string(),
            vec![1, 2, 3],
            None,
            None,
        );

        assert!(matches!(
            out.decision,
            crate::guards::GuardDecision::Deny { .. }
        ));
    }

    #[test]
    fn send_denied_when_sender_is_not_channel_member() {
        let service = ChatFactService::new();
        let snapshot = test_guard_snapshot(
            1,
            2,
            FlowCost::new(10),
            vec![ChatCapability::MessageSend.as_name()],
            123,
        );
        let channel_id = test_channel_id(0);
        let membership = MembershipSnapshot::new(
            snapshot.context_id,
            channel_id,
            snapshot.authority_id,
            false,
            snapshot.now_ms,
        );
        let snapshot = snapshot.with_membership_snapshot(membership);

        let out = service.prepare_send_message_sealed(
            &snapshot,
            channel_id,
            "msg-1".to_string(),
            "Alice".to_string(),
            vec![1, 2, 3],
            None,
            None,
        );

        assert!(matches!(
            out.decision,
            crate::guards::GuardDecision::Deny { .. }
        ));
    }
}
