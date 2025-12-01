//! Clean Journal API
//!
//! This module provides an API for journal operations
//! that hides CRDT implementation details.

use crate::fact::{Fact, FactContent, Journal as FactJournal, JournalNamespace};
use crate::semilattice::{AccountState, OpLog};

use aura_core::effects::time::{LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::semilattice::JoinSemilattice;
use aura_core::time::{OrderTime, TimeDomain, TimeStamp};
use aura_core::{
    effects::{CryptoEffects, RandomEffects},
    AccountId, AuraError, Ed25519VerifyingKey,
};
use serde::{Deserialize, Serialize};

/// Simplified Journal interface using fact-based architecture
///
/// # Stability: STABLE
/// This is the main journal API with semver guarantees.
#[derive(Debug, Clone)]
pub struct Journal {
    /// Account state for epoch and guardian management
    account_state: AccountState,
    /// Operation log for tracking applied operations
    op_log: OpLog,
    /// Fact-based journal for new architecture
    fact_journal: FactJournal,
}

fn derive_context_for_fact(fact: &JournalFact) -> ContextId {
    let mut input = Vec::new();
    input.extend_from_slice(&fact.source_authority.to_bytes());
    input.extend_from_slice(fact.content.as_bytes());
    ContextId::new_from_entropy(hash(&input))
}

impl Journal {
    /// Create a new journal for an account
    pub async fn new(account_id: AccountId, crypto: &dyn CryptoEffects) -> Result<Self, AuraError> {
        // Generate keypair through effects system
        let (_, public_key_bytes) = crypto
            .ed25519_generate_keypair()
            .await
            .map_err(|e| AuraError::internal(format!("Failed to generate keypair: {}", e)))?;

        let group_key = Ed25519VerifyingKey(public_key_bytes);

        // Create authority ID from account ID for namespace
        let authority_id = AuthorityId::from_uuid(account_id.0);
        let namespace = JournalNamespace::Authority(authority_id);

        Ok(Self {
            account_state: AccountState::new(account_id, group_key),
            op_log: OpLog::default(),
            fact_journal: FactJournal::new(namespace),
        })
    }

    /// Create a new journal for an account with specific group key bytes
    pub fn new_with_group_key_bytes(account_id: AccountId, group_key_bytes: Vec<u8>) -> Self {
        // Create authority ID from account ID for namespace
        let authority_id = AuthorityId::from_uuid(account_id.0);
        let namespace = JournalNamespace::Authority(authority_id);

        let group_key = Ed25519VerifyingKey(group_key_bytes);

        Self {
            account_state: AccountState::new(account_id, group_key),
            op_log: OpLog::default(),
            fact_journal: FactJournal::new(namespace),
        }
    }

    /// Merge with another journal
    pub fn merge(&mut self, other: &Journal) -> Result<(), AuraError> {
        // Merge semilattice components
        self.account_state = self.account_state.join(&other.account_state);
        self.op_log = self.op_log.join(&other.op_log);

        // Merge fact journals
        self.fact_journal.join_assign(&other.fact_journal);

        Ok(())
    }

    /// Add a fact to the journal using the default order-clock domain
    pub async fn add_fact(
        &mut self,
        journal_fact: JournalFact,
        random: &dyn RandomEffects,
    ) -> Result<(), AuraError> {
        // Bridge RandomEffects into an order-clock generator for the default path.
        struct RandomOrder<'a> {
            rand: &'a dyn RandomEffects,
        }
        #[async_trait::async_trait]
        impl<'a> OrderClockEffects for RandomOrder<'a> {
            async fn order_time(&self) -> Result<OrderTime, aura_core::effects::time::TimeError> {
                Ok(OrderTime(self.rand.random_bytes_32().await))
            }
        }

        self.add_fact_with_domain(
            journal_fact,
            TimeDomain::OrderClock,
            &RandomOrder { rand: random },
            None,
            None,
        )
        .await
    }

    /// Add a fact to the journal using a specified time domain
    pub async fn add_fact_with_domain(
        &mut self,
        journal_fact: JournalFact,
        domain: TimeDomain,
        order_clock: &dyn OrderClockEffects,
        physical_clock: Option<&dyn PhysicalTimeEffects>,
        logical_clock: Option<&dyn LogicalClockEffects>,
    ) -> Result<(), AuraError> {
        // Thread through effect context using the fact's source authority
        let _ctx = crate::fact::EffectContext::with_authority(journal_fact.source_authority);
        let ts = match domain {
            TimeDomain::OrderClock => {
                let id = order_clock
                    .order_time()
                    .await
                    .map_err(|e| AuraError::internal(e.to_string()))?;
                TimeStamp::OrderClock(id)
            }
            TimeDomain::PhysicalClock => {
                let clock = physical_clock.ok_or_else(|| {
                    AuraError::invalid("Physical clock requested but no provider supplied")
                })?;
                TimeStamp::PhysicalClock(
                    clock
                        .physical_time()
                        .await
                        .map_err(|e| AuraError::internal(e.to_string()))?,
                )
            }
            TimeDomain::LogicalClock => {
                let clock = logical_clock.ok_or_else(|| {
                    AuraError::invalid("Logical clock requested but no provider supplied")
                })?;
                TimeStamp::LogicalClock(
                    clock
                        .logical_now()
                        .await
                        .map_err(|e| AuraError::internal(e.to_string()))?,
                )
            }
            TimeDomain::Range => {
                return Err(AuraError::invalid(
                    "Range domain must accompany a base domain",
                ))
            }
        };
        let order = match &ts {
            TimeStamp::OrderClock(id) => id.clone(),
            // If not order clock, synthesize an order token for deterministic insertion
            _ => OrderTime(aura_core::hash::hash(format!("{:?}", &ts).as_bytes())),
        };
        let fact = Fact {
            timestamp: ts,
            order,
            content: FactContent::Relational(crate::fact::RelationalFact::Generic {
                context_id: derive_context_for_fact(&journal_fact),
                binding_type: journal_fact.content.clone(),
                binding_data: journal_fact.content.clone().into_bytes(),
            }),
        };

        self.fact_journal.add_fact(fact)?;
        Ok(())
    }

    /// Get account state summary
    pub fn account_summary(&self) -> AccountSummary {
        // Derive device count from authority facts in TreeState
        let device_count = self.get_device_count_from_tree_state();

        AccountSummary {
            account_id: self.account_state.account_id,
            device_count,
            guardian_count: self.account_state.guardian_registry.guardians.len(),
            last_epoch: self.account_state.epoch_counter.value,
        }
    }

    /// Derive device count from authority facts in TreeState
    fn get_device_count_from_tree_state(&self) -> usize {
        // Use the reduction function to derive tree state from facts
        use crate::reduction::reduce_authority;

        // Reduce the authority facts to get current tree state
        // AccountJournal always uses Authority namespace, so Ok is expected
        match reduce_authority(&self.fact_journal) {
            Ok(authority_state) => authority_state.tree_state.device_count() as usize,
            Err(_) => {
                // This should not happen for an AccountJournal
                tracing::warn!("AccountJournal has unexpected namespace, returning 0 devices");
                0
            }
        }
    }

    /// Get account ID
    pub fn account_id(&self) -> AccountId {
        self.account_state.account_id
    }

    /// Get fact journal for advanced usage
    pub fn fact_journal(&self) -> &FactJournal {
        &self.fact_journal
    }
}

/// Fact to be added to the journal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFact {
    /// Content of the fact being recorded
    pub content: String,
    /// Time when the fact was created (using unified time system)
    pub timestamp: TimeStamp,
    /// Authority that originated this fact
    pub source_authority: AuthorityId,
}

// Use ContextId from aura-core instead of defining our own

/// Simplified account summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    /// The account identifier
    pub account_id: AccountId,
    /// Number of devices in the account
    pub device_count: usize,
    /// Number of guardians configured for the account
    pub guardian_count: usize,
    /// Latest epoch number for this account
    pub last_epoch: u64,
}

impl AccountSummary {
    /// Create a new account summary
    pub fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            device_count: 0,
            guardian_count: 0,
            last_epoch: 0,
        }
    }
}
