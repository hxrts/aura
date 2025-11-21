//! Clean Journal API (Phase 1 API Cleanup)
//!
//! This module provides a clean, simplified API for journal operations
//! that hides CRDT implementation details behind user-friendly abstractions.

use crate::fact_journal::{Fact, FactContent, FactId, Journal as FactJournal, JournalNamespace};
use crate::semilattice::*;

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::semilattice::JoinSemilattice;
use aura_core::{AccountId, AuraError};
use serde::{Deserialize, Serialize};

/// Simplified Journal interface hiding CRDT internals
///
/// # Stability: STABLE
/// This is the main journal API with semver guarantees.
#[derive(Debug, Clone)]
pub struct Journal {
    /// Internal CRDT state (hidden from public API)
    journal_map: JournalMap,
    /// Internal account state (hidden from public API)
    account_state: ModernAccountState,
    /// Internal operation log (hidden from public API)
    op_log: OpLog,
    /// Fact-based journal for new architecture
    fact_journal: FactJournal,
}

impl Journal {
    /// Create a new journal for an account
    pub fn new(account_id: AccountId) -> Self {
        // Use proper constructors for the CRDT types
        let ed25519_key = ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(); // placeholder

        // Create authority ID from account ID for namespace
        let authority_id = AuthorityId::from_uuid(account_id.0);
        let namespace = JournalNamespace::Authority(authority_id);

        Self {
            journal_map: JournalMap::default(),
            account_state: ModernAccountState::new(account_id, ed25519_key),
            op_log: OpLog::default(),
            fact_journal: FactJournal::new(namespace),
        }
    }

    /// Create a new journal for an account with specific group key
    pub fn new_with_group_key(
        account_id: AccountId,
        group_key: ed25519_dalek::VerifyingKey,
    ) -> Self {
        // Create authority ID from account ID for namespace
        let authority_id = AuthorityId::from_uuid(account_id.0);
        let namespace = JournalNamespace::Authority(authority_id);

        Self {
            journal_map: JournalMap::default(),
            account_state: ModernAccountState::new(account_id, group_key),
            op_log: OpLog::default(),
            fact_journal: FactJournal::new(namespace),
        }
    }

    /// Merge with another journal (CRDT join operation)
    pub fn merge(&mut self, other: &Journal) -> Result<(), AuraError> {
        // Hide the CRDT implementation details
        self.journal_map = self.journal_map.join(&other.journal_map);
        self.account_state = self.account_state.join(&other.account_state);
        self.op_log = self.op_log.join(&other.op_log);

        // Merge fact journals
        self.fact_journal.join_assign(&other.fact_journal);

        Ok(())
    }

    /// Add a fact to the journal
    pub fn add_fact(&mut self, journal_fact: JournalFact) -> Result<(), AuraError> {
        let source_authority = journal_fact.source_authority;

        // Convert JournalFact to proper Fact with FactContent
        let fact = Fact {
            fact_id: FactId::new(),
            content: FactContent::FlowBudget(crate::fact_journal::FlowBudgetFact {
                context_id: ContextId::new(), // placeholder - should come from journal_fact or context
                source: source_authority,
                destination: source_authority, // placeholder
                spent_amount: 0,
                epoch: 0,
            }),
        };

        // Add to fact journal
        self.fact_journal.add_fact(fact)?;
        Ok(())
    }

    /// Get account state summary
    pub fn account_summary(&self) -> AccountSummary {
        AccountSummary {
            account_id: self.account_state.account_id,
            device_count: 0, // TODO: Derive device count from authority facts in TreeState
            guardian_count: self.account_state.guardian_registry.guardians.len(),
            last_epoch: self.account_state.epoch_counter.value,
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
    /// Unix timestamp when the fact was created
    pub timestamp: u64,
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
