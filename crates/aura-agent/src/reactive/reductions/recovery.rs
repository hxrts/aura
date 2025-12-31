//! Recovery View Reduction
//!
//! Transforms recovery-related journal facts into `RecoveryDelta` updates.
//! Delegates to `RecoveryViewReducer` from the `aura-recovery` crate.

use crate::reactive::scheduler::ViewReduction;
use aura_composition::{downcast_delta, ViewDeltaReducer};
use aura_core::identifiers::AuthorityId;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_recovery::{RecoveryDelta, RecoveryViewReducer, RECOVERY_FACT_TYPE_ID};

/// Reduction adapter for recovery view
///
/// Delegates to `RecoveryViewReducer` from `aura-recovery` crate.
pub struct RecoveryReduction;

impl ViewReduction<RecoveryDelta> for RecoveryReduction {
    fn reduce(&self, facts: &[Fact], own_authority: Option<AuthorityId>) -> Vec<RecoveryDelta> {
        let reducer = RecoveryViewReducer;

        facts
            .iter()
            .filter_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::Generic {
                    binding_type,
                    binding_data,
                    ..
                }) if binding_type == RECOVERY_FACT_TYPE_ID => {
                    // Use the domain reducer and downcast back to RecoveryDelta
                    let view_deltas =
                        reducer.reduce_fact(binding_type, binding_data, own_authority);
                    view_deltas
                        .into_iter()
                        .filter_map(|vd| downcast_delta::<RecoveryDelta>(&vd).cloned())
                        .next()
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_journal::DomainFact;
    use aura_recovery::RecoveryFact;

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
        Fact {
            order,
            timestamp,
            content,
        }
    }

    #[test]
    fn test_recovery_reduction() {
        let reduction = RecoveryReduction;

        let setup_initiated = RecoveryFact::guardian_setup_initiated_ms(
            test_context_id(),
            AuthorityId::new_from_entropy([1u8; 32]),
            vec![
                AuthorityId::new_from_entropy([2u8; 32]),
                AuthorityId::new_from_entropy([3u8; 32]),
            ],
            2,
            1234567890,
        );

        let facts = vec![make_test_fact(
            1,
            FactContent::Relational(setup_initiated.to_generic()),
        )];

        let deltas = reduction.reduce(&facts, None);
        assert_eq!(deltas.len(), 1);
        assert!(matches!(
            &deltas[0],
            RecoveryDelta::GuardianSetupStarted { .. }
        ));
    }
}
