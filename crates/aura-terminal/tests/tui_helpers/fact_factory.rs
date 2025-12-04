//! Fact Factory
//!
//! Helper functions for creating test facts.

use aura_core::{
    identifiers::{AuthorityId, ContextId},
    time::{OrderTime, PhysicalTime, TimeStamp},
    Hash32,
};
use aura_journal::fact::{Fact, FactContent, RelationalFact};

/// Create a test context ID
pub fn test_context_id() -> ContextId {
    ContextId::new_from_entropy([0u8; 32])
}

/// Create a test authority ID with a unique seed
pub fn test_authority_id(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

/// Create an order time with a unique index
pub fn order_time(index: u64) -> OrderTime {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&index.to_be_bytes());
    OrderTime(bytes)
}

/// Create a physical timestamp
pub fn physical_time(ms: u64) -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: ms,
        uncertainty: None,
    })
}

/// Create a generic relational fact
pub fn make_generic_fact(binding_type: &str, order_index: u64) -> Fact {
    Fact {
        order: order_time(order_index),
        timestamp: physical_time(1000 + order_index),
        content: FactContent::Relational(RelationalFact::Generic {
            context_id: test_context_id(),
            binding_type: binding_type.to_string(),
            binding_data: vec![order_index as u8],
        }),
    }
}

/// Create a guardian binding fact
pub fn make_guardian_binding_fact(account_seed: u8, guardian_seed: u8, order_index: u64) -> Fact {
    Fact {
        order: order_time(order_index),
        timestamp: physical_time(1000 + order_index),
        content: FactContent::Relational(RelationalFact::GuardianBinding {
            account_id: test_authority_id(account_seed),
            guardian_id: test_authority_id(guardian_seed),
            binding_hash: Hash32([0u8; 32]),
        }),
    }
}

/// Create a recovery grant fact
pub fn make_recovery_grant_fact(account_seed: u8, guardian_seed: u8, order_index: u64) -> Fact {
    Fact {
        order: order_time(order_index),
        timestamp: physical_time(1000 + order_index),
        content: FactContent::Relational(RelationalFact::RecoveryGrant {
            account_id: test_authority_id(account_seed),
            guardian_id: test_authority_id(guardian_seed),
            grant_hash: Hash32([0u8; 32]),
        }),
    }
}

/// Create a channel creation fact
pub fn make_channel_created_fact(order_index: u64) -> Fact {
    make_generic_fact("channel_create", order_index)
}

/// Create a message sent fact
pub fn make_message_sent_fact(order_index: u64) -> Fact {
    make_generic_fact("message_send", order_index)
}

/// Create a block created fact
#[allow(dead_code)]
pub fn make_block_created_fact(order_index: u64) -> Fact {
    make_generic_fact("block_created", order_index)
}

/// Create a resident joined fact
#[allow(dead_code)]
pub fn make_resident_joined_fact(order_index: u64) -> Fact {
    make_generic_fact("resident_joined", order_index)
}

/// Create an invitation created fact
pub fn make_invitation_created_fact(order_index: u64) -> Fact {
    make_generic_fact("invitation_created", order_index)
}

/// Create an invitation accepted fact
#[allow(dead_code)]
pub fn make_invitation_accepted_fact(order_index: u64) -> Fact {
    make_generic_fact("invitation_accepted", order_index)
}

/// Create a recovery initiated fact
#[allow(dead_code)]
pub fn make_recovery_initiated_fact(order_index: u64) -> Fact {
    make_generic_fact("recovery_initiated", order_index)
}

/// Create a recovery completed fact
#[allow(dead_code)]
pub fn make_recovery_completed_fact(order_index: u64) -> Fact {
    make_generic_fact("recovery_completed", order_index)
}

/// Create a threshold updated fact
#[allow(dead_code)]
pub fn make_threshold_updated_fact(order_index: u64) -> Fact {
    make_generic_fact("threshold_updated", order_index)
}

/// Batch of facts for testing a complete scenario
pub struct FactScenario {
    pub facts: Vec<Fact>,
    next_order: u64,
}

impl FactScenario {
    /// Create a new empty scenario
    pub fn new() -> Self {
        Self {
            facts: Vec::new(),
            next_order: 1,
        }
    }

    /// Add a guardian binding
    pub fn add_guardian_binding(&mut self, account_seed: u8, guardian_seed: u8) -> &mut Self {
        self.facts.push(make_guardian_binding_fact(
            account_seed,
            guardian_seed,
            self.next_order,
        ));
        self.next_order += 1;
        self
    }

    /// Add a recovery grant
    pub fn add_recovery_grant(&mut self, account_seed: u8, guardian_seed: u8) -> &mut Self {
        self.facts.push(make_recovery_grant_fact(
            account_seed,
            guardian_seed,
            self.next_order,
        ));
        self.next_order += 1;
        self
    }

    /// Add a channel creation
    pub fn add_channel_created(&mut self) -> &mut Self {
        self.facts.push(make_channel_created_fact(self.next_order));
        self.next_order += 1;
        self
    }

    /// Add a message
    pub fn add_message_sent(&mut self) -> &mut Self {
        self.facts.push(make_message_sent_fact(self.next_order));
        self.next_order += 1;
        self
    }

    /// Build the scenario (consumes self)
    pub fn build(self) -> Vec<Fact> {
        self.facts
    }
}

impl Default for FactScenario {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_generic_fact() {
        let fact = make_generic_fact("test_type", 42);
        match &fact.content {
            FactContent::Relational(RelationalFact::Generic { binding_type, .. }) => {
                assert_eq!(binding_type, "test_type");
            }
            _ => panic!("Expected Generic fact"),
        }
    }

    #[test]
    fn test_make_guardian_binding_fact() {
        let fact = make_guardian_binding_fact(1, 2, 1);
        match &fact.content {
            FactContent::Relational(RelationalFact::GuardianBinding { .. }) => {
                // Success
            }
            _ => panic!("Expected GuardianBinding fact"),
        }
    }

    #[test]
    fn test_fact_scenario() {
        let mut scenario = FactScenario::new();
        scenario
            .add_guardian_binding(1, 2)
            .add_guardian_binding(1, 3)
            .add_channel_created()
            .add_message_sent();
        let facts = scenario.build();

        assert_eq!(facts.len(), 4);
    }
}
