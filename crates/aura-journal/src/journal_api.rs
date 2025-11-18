//! Clean Journal API (Phase 1 API Cleanup)
//!
//! This module provides a clean, simplified API for journal operations
//! that hides CRDT implementation details behind user-friendly abstractions.

use crate::semilattice::*;
use aura_core::relationships::ContextId;
use aura_core::semilattice::JoinSemilattice;
use aura_core::{AccountId, AuraError, DeviceId};
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
}

impl Journal {
    /// Create a new journal for an account
    pub fn new(account_id: AccountId) -> Self {
        // Use proper constructors for the CRDT types
        let ed25519_key = ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(); // placeholder
        Self {
            journal_map: JournalMap::default(),
            account_state: ModernAccountState::new(account_id, ed25519_key),
            op_log: OpLog::default(),
        }
    }

    /// Create a new journal for an account with specific group key
    pub fn new_with_group_key(
        account_id: AccountId,
        group_key: ed25519_dalek::VerifyingKey,
    ) -> Self {
        Self {
            journal_map: JournalMap::default(),
            account_state: ModernAccountState::new(account_id, group_key),
            op_log: OpLog::default(),
        }
    }

    /// Add a device to the account (for testing)
    pub fn add_device(&mut self, device: crate::DeviceMetadata) -> Result<(), AuraError> {
        self.account_state.add_device(device);
        Ok(())
    }

    /// Merge with another journal (CRDT join operation)
    pub fn merge(&mut self, other: &Journal) -> Result<(), AuraError> {
        // Hide the CRDT implementation details
        self.journal_map = self.journal_map.join(&other.journal_map);
        self.account_state = self.account_state.join(&other.account_state);
        self.op_log = self.op_log.join(&other.op_log);
        Ok(())
    }

    /// Add a fact to the journal
    pub fn add_fact(&mut self, _fact: JournalFact) -> Result<(), AuraError> {
        // Implementation details hidden
        todo!("Add fact implementation")
    }

    /// Get current capabilities for a context
    pub fn get_capabilities(&self, _context: &ContextId) -> CapabilitySet {
        // Implementation details hidden
        todo!("Get capabilities implementation")
    }

    /// Get account state summary
    pub fn account_summary(&self) -> AccountSummary {
        AccountSummary {
            account_id: self.account_state.account_id,
            device_count: self.account_state.device_registry.devices.len(),
            guardian_count: self.account_state.guardian_registry.guardians.len(),
            last_epoch: self.account_state.epoch_counter.value,
        }
    }

    /// Get account ID
    pub fn account_id(&self) -> AccountId {
        self.account_state.account_id
    }

    /// Get devices for testing purposes
    pub fn devices(&self) -> &std::collections::BTreeMap<DeviceId, crate::DeviceMetadata> {
        &self.account_state.device_registry.devices
    }
}

/// Fact to be added to the journal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalFact {
    pub content: String,
    pub timestamp: u64,
    pub source_device: DeviceId,
}

// Use ContextId from aura-core instead of defining our own

/// Simplified account summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub account_id: AccountId,
    pub device_count: usize,
    pub guardian_count: usize,
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
