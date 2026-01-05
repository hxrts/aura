//! Home View Reduction
//!
//! Transforms social/home-related journal facts into `HomeDelta` updates.
//! Handles home creation, resident changes, and storage statistics.

use crate::reactive::scheduler::ViewReduction;
use aura_core::identifiers::AuthorityId;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use aura_social::{SocialFact, SOCIAL_FACT_TYPE_ID};

/// Delta type for home view
#[derive(Debug, Clone, PartialEq)]
pub enum HomeDelta {
    /// A new home was created
    HomeCreated {
        home_id: String,
        name: String,
        created_at: u64,
        creator_id: String,
    },
    /// A resident joined the home
    ResidentAdded {
        authority_id: String,
        name: String,
        joined_at: u64,
    },
    /// A resident left the home
    ResidentRemoved { authority_id: String, left_at: u64 },
    /// Home storage statistics updated
    StorageUpdated {
        used_bytes: u64,
        total_bytes: u64,
        updated_at: u64,
    },
}

/// Reduction function for home view
pub struct HomeReduction;

impl ViewReduction<HomeDelta> for HomeReduction {
    fn reduce(&self, facts: &[Fact], _own_authority: Option<AuthorityId>) -> Vec<HomeDelta> {
        facts
            .iter()
            .filter_map(|fact| match &fact.content {
                FactContent::Relational(RelationalFact::Generic { envelope, .. }) => {
                    // Only process social facts
                    if envelope.type_id.as_str() != SOCIAL_FACT_TYPE_ID {
                        return None;
                    }

                    // Deserialize the SocialFact from envelope
                    let social_fact = SocialFact::from_envelope(envelope)?;

                    match social_fact {
                        SocialFact::HomeCreated {
                            home_id,
                            created_at,
                            creator_id,
                            name,
                            ..
                        } => Some(HomeDelta::HomeCreated {
                            home_id: format!("{}", home_id),
                            name,
                            created_at: created_at.ts_ms,
                            creator_id: creator_id.to_string(),
                        }),
                        SocialFact::ResidentJoined {
                            authority_id,
                            joined_at,
                            name,
                            ..
                        } => Some(HomeDelta::ResidentAdded {
                            authority_id: authority_id.to_string(),
                            name,
                            joined_at: joined_at.ts_ms,
                        }),
                        SocialFact::ResidentLeft {
                            authority_id,
                            left_at,
                            ..
                        } => Some(HomeDelta::ResidentRemoved {
                            authority_id: authority_id.to_string(),
                            left_at: left_at.ts_ms,
                        }),
                        SocialFact::StorageUpdated {
                            used_bytes,
                            total_bytes,
                            updated_at,
                            ..
                        } => Some(HomeDelta::StorageUpdated {
                            used_bytes,
                            total_bytes,
                            updated_at: updated_at.ts_ms,
                        }),
                        // Other social facts don't map to HomeDelta
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
    use aura_core::identifiers::{AuthorityId, ContextId};
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_social::HomeId;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([0u8; 32])
    }

    fn test_home_id() -> HomeId {
        HomeId::from_bytes([1u8; 32])
    }

    fn test_authority_id() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
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
    fn test_home_reduction() {
        let reduction = HomeReduction;

        let home_created = SocialFact::home_created_ms(
            test_home_id(),
            test_context_id(),
            1000,
            test_authority_id(),
            "Test Home".to_string(),
        );

        let resident_joined = SocialFact::resident_joined_ms(
            test_authority_id(),
            test_home_id(),
            test_context_id(),
            2000,
            "Alice".to_string(),
        );

        let facts = vec![
            make_test_fact(1, FactContent::Relational(home_created.to_generic())),
            make_test_fact(2, FactContent::Relational(resident_joined.to_generic())),
        ];

        let deltas = reduction.reduce(&facts, None);
        assert_eq!(deltas.len(), 2);
        assert!(matches!(&deltas[0], HomeDelta::HomeCreated { name, .. } if name == "Test Home"));
        assert!(matches!(&deltas[1], HomeDelta::ResidentAdded { name, .. } if name == "Alice"));
    }
}
