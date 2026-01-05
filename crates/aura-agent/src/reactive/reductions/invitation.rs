//! Invitation View Reduction
//!
//! Transforms invitation-related journal facts into `InvitationDelta` updates.
//! Delegates to `InvitationViewReducer` from the `aura-invitation` crate.

use crate::reactive::scheduler::ViewReduction;
use aura_composition::{downcast_delta, ViewDelta, ViewDeltaReducer};
use aura_core::identifiers::AuthorityId;
use aura_invitation::{InvitationDelta, InvitationViewReducer, INVITATION_FACT_TYPE_ID};
use aura_journal::fact::{Fact, FactContent, RelationalFact};

/// Reduction adapter for invitations view
///
/// Delegates to `InvitationViewReducer` from `aura-invitation` crate.
pub struct InvitationReduction;

fn downcast_invitation_deltas(view_deltas: Vec<ViewDelta>) -> Vec<InvitationDelta> {
    view_deltas
        .into_iter()
        .filter_map(|vd| downcast_delta::<InvitationDelta>(&vd).cloned())
        .collect()
}

impl ViewReduction<InvitationDelta> for InvitationReduction {
    fn reduce(&self, facts: &[Fact], own_authority: Option<AuthorityId>) -> Vec<InvitationDelta> {
        let reducer = InvitationViewReducer;

        facts
            .iter()
            .flat_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == INVITATION_FACT_TYPE_ID =>
                {
                    // Use the domain reducer and downcast back to InvitationDelta
                    let view_deltas = reducer.reduce_fact(
                        envelope.type_id.as_str(),
                        &envelope.payload,
                        own_authority,
                    );
                    downcast_invitation_deltas(view_deltas)
                }
                _ => Vec::new(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_composition::IntoViewDelta;
    use aura_core::identifiers::{AuthorityId, ContextId, InvitationId};
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_invitation::InvitationFact;
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
    fn test_invitation_reduction() {
        let reduction = InvitationReduction;

        let sent_fact = InvitationFact::sent_ms(
            test_context_id(),
            InvitationId::new("inv-123"),
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            aura_invitation::InvitationType::Contact { nickname: None },
            1234567890,
            None,
            None,
        );

        let facts = vec![make_test_fact(
            1,
            FactContent::Relational(sent_fact.to_generic()),
        )];

        let test_authority = Some(AuthorityId::new_from_entropy([99u8; 32]));
        let deltas = reduction.reduce(&facts, test_authority);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(
            &deltas[0],
            InvitationDelta::InvitationAdded { .. }
        ));
    }

    #[test]
    fn test_downcast_preserves_all_deltas() {
        let view_deltas = vec![
            InvitationDelta::InvitationAdded {
                invitation_id: InvitationId::new("inv-1"),
                direction: "outbound".to_string(),
                other_party_id: "other".to_string(),
                other_party_name: "Other".to_string(),
                invitation_type: aura_invitation::InvitationType::Contact { nickname: None },
                created_at: 1,
                expires_at: None,
                message: None,
            }
            .into_view_delta(),
            InvitationDelta::InvitationStatusChanged {
                invitation_id: InvitationId::new("inv-1"),
                old_status: "pending".to_string(),
                new_status: "accepted".to_string(),
                changed_at: 2,
            }
            .into_view_delta(),
        ];

        let deltas = downcast_invitation_deltas(view_deltas);
        assert_eq!(deltas.len(), 2);
    }
}
