//! Guardian View Reduction
//!
//! Transforms guardian-related journal facts into `GuardianDelta` updates.
//! Handles guardian network state changes (additions, removals, status changes).

use crate::reactive::scheduler::ViewReduction;
use aura_core::types::identifiers::AuthorityId;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_recovery::{RecoveryFact, RECOVERY_FACT_TYPE_ID};

/// Delta type for guardians view
#[derive(Debug, Clone, PartialEq)]
pub enum GuardianDelta {
    /// A new guardian was added to the recovery network
    GuardianAdded {
        authority_id: String,
        name: String,
        added_at: u64,
        share_index: Option<u32>,
    },
    /// A guardian's status changed
    GuardianStatusChanged {
        authority_id: String,
        old_status: String,
        new_status: String,
        last_seen: Option<u64>,
    },
    /// Recovery threshold was updated
    ThresholdUpdated { threshold: u32, total: u32 },
}

/// Reduction function for guardians view
pub struct GuardianReduction;

impl ViewReduction<GuardianDelta> for GuardianReduction {
    fn reduce(&self, facts: &[Fact], _own_authority: Option<AuthorityId>) -> Vec<GuardianDelta> {
        facts
            .iter()
            .filter_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::GuardianBinding { guardian_id, .. },
                )) => Some(GuardianDelta::GuardianAdded {
                    authority_id: format!("{:?}", guardian_id),
                    name: "unknown".to_string(),
                    added_at: 0,
                    share_index: None,
                }),
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == RECOVERY_FACT_TYPE_ID =>
                {
                    match RecoveryFact::from_envelope(envelope)? {
                        RecoveryFact::MembershipChangeCompleted {
                            new_guardian_ids,
                            new_threshold,
                            ..
                        } => Some(GuardianDelta::ThresholdUpdated {
                            threshold: u32::from(new_threshold),
                            total: u32::try_from(new_guardian_ids.len()).unwrap_or(u32::MAX),
                        }),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_core::types::identifiers::{AuthorityId, ContextId};
    use aura_core::Hash32;

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
    fn test_guardian_reduction() {
        let reduction = GuardianReduction;
        let facts = vec![
            make_test_fact(
                1,
                FactContent::Relational(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::GuardianBinding {
                        account_id: AuthorityId::new_from_entropy([1u8; 32]),
                        guardian_id: AuthorityId::new_from_entropy([2u8; 32]),
                        binding_hash: Hash32([0u8; 32]),
                    },
                )),
            ),
            make_test_fact(
                2,
                FactContent::Relational(
                    aura_recovery::RecoveryFact::membership_change_completed_ms(
                        test_context_id(),
                        Hash32([7u8; 32]),
                        vec![
                            AuthorityId::new_from_entropy([2u8; 32]),
                            AuthorityId::new_from_entropy([3u8; 32]),
                            AuthorityId::new_from_entropy([4u8; 32]),
                        ],
                        2,
                        1_000,
                    )
                    .to_generic(),
                ),
            ),
        ];

        let deltas = reduction.reduce(&facts, None);
        assert_eq!(deltas.len(), 2);
        assert!(matches!(&deltas[0], GuardianDelta::GuardianAdded { .. }));
        assert!(matches!(
            &deltas[1],
            GuardianDelta::ThresholdUpdated {
                threshold: 2,
                total: 3
            }
        ));
    }
}
