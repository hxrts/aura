//! Unified account state using semilattice architecture
//!
//! This module provides AccountState using the semilattice system.

use super::EpochLog;
use crate::types::GuardianMetadata;
use aura_core::crypto::ed25519::Ed25519SigningKey;
use aura_core::Ed25519VerifyingKey;
use aura_core::{
    identifiers::AccountId,
    semilattice::{Bottom, CvState, JoinSemilattice},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Modern account state using semilattice architecture
///
/// This replaces the previous Automerge-based AccountState with a composition
/// of multiple semilattice CRDTs, providing the same functionality with
/// better performance and composability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    /// Account identifier (immutable)
    pub account_id: AccountId,
    /// Group public key for threshold signature verification (immutable)
    pub group_public_key: Ed25519VerifyingKey,
    /// Guardian registry using grow-only semantics
    pub guardian_registry: GuardianRegistry,
    /// Epoch counter using max-counter CRDT
    pub epoch_counter: MaxCounter,
    /// Lamport clock using max-counter CRDT
    pub lamport_clock: MaxCounter,
    /// Applied operations log
    pub applied_operations: EpochLog<String>,
}

/// Guardian registry using grow-only CRDT semantics
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianRegistry {
    /// Registered guardians with their metadata
    pub guardians: BTreeMap<String, GuardianMetadata>, // Using email as key
}

/// Max-counter CRDT for epoch and lamport clock management
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaxCounter {
    /// Current counter value
    pub value: u64,
}

impl AccountState {
    /// Create a new account state
    pub fn new(account_id: AccountId, group_public_key: Ed25519VerifyingKey) -> Self {
        Self {
            account_id,
            group_public_key,
            guardian_registry: GuardianRegistry::new(),
            epoch_counter: MaxCounter::new(),
            lamport_clock: MaxCounter::new(),
            applied_operations: EpochLog::new(),
        }
    }

    // Epoch Management

    /// Get current epoch
    pub fn get_epoch(&self) -> u64 {
        self.epoch_counter.value
    }

    /// Increment epoch
    pub fn increment_epoch(&mut self) {
        self.epoch_counter.increment();
        self.lamport_clock.increment();
    }

    /// Set epoch if higher than current (for sync)
    pub fn set_epoch_if_higher(&mut self, new_epoch: u64) {
        if new_epoch > self.epoch_counter.value {
            self.epoch_counter.set_max(new_epoch);
        }
    }

    /// Get lamport clock
    pub fn get_lamport_clock(&self) -> u64 {
        self.lamport_clock.value
    }

    // Guardian Management

    /// Add a guardian
    pub fn add_guardian(&mut self, guardian: GuardianMetadata) {
        self.guardian_registry.add_guardian(guardian);
    }

    /// Get all guardians
    pub fn get_guardians(&self) -> Vec<GuardianMetadata> {
        self.guardian_registry.guardians.values().cloned().collect()
    }

    // Operation Tracking

    /// Check if an operation has been applied
    pub fn has_operation(&self, op_id: &str) -> bool {
        // Simple check - in practice you'd have more sophisticated indexing
        self.applied_operations.ops.values().any(|op| op == op_id)
    }

    /// Mark an operation as applied
    pub fn mark_operation_applied(&mut self, op_id: String) {
        let epoch = self.get_epoch();
        self.applied_operations.add_operation(epoch, op_id);
    }

    // Serialization/Persistence

    /// Convert to bytes for storage using DAG-CBOR
    pub fn to_bytes(&self) -> Result<Vec<u8>, aura_core::util::serialization::SerializationError> {
        aura_core::util::serialization::to_vec(self)
    }

    /// Load from bytes
    pub fn from_bytes(
        bytes: &[u8],
    ) -> Result<Self, aura_core::util::serialization::SerializationError> {
        aura_core::util::serialization::from_slice(bytes)
    }
}

impl GuardianRegistry {
    /// Create a new empty guardian registry
    pub fn new() -> Self {
        Self {
            guardians: BTreeMap::new(),
        }
    }

    /// Add a guardian
    pub fn add_guardian(&mut self, guardian: GuardianMetadata) {
        self.guardians.insert(guardian.email.clone(), guardian);
    }

    /// Get guardian by email
    pub fn get_guardian(&self, email: &str) -> Option<&GuardianMetadata> {
        self.guardians.get(email)
    }

    /// Check if guardian exists
    pub fn has_guardian(&self, email: &str) -> bool {
        self.guardians.contains_key(email)
    }

    /// Get number of guardians
    pub fn len(&self) -> usize {
        self.guardians.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.guardians.is_empty()
    }
}

impl MaxCounter {
    /// Create a new counter at zero
    pub fn new() -> Self {
        Self { value: 0 }
    }

    /// Create with initial value
    pub fn with_value(value: u64) -> Self {
        Self { value }
    }

    /// Increment the counter
    pub fn increment(&mut self) {
        self.value += 1;
    }

    /// Set to maximum of current and provided value
    pub fn set_max(&mut self, other: u64) {
        self.value = self.value.max(other);
    }

    /// Get current value
    pub fn get(&self) -> u64 {
        self.value
    }
}

// Semilattice implementations

impl JoinSemilattice for AccountState {
    fn join(&self, other: &Self) -> Self {
        // Account ID and group key must match for meaningful join
        assert_eq!(self.account_id, other.account_id);
        assert_eq!(self.group_public_key, other.group_public_key);

        Self {
            account_id: self.account_id,
            group_public_key: self.group_public_key.clone(),
            guardian_registry: self.guardian_registry.join(&other.guardian_registry),
            epoch_counter: self.epoch_counter.join(&other.epoch_counter),
            lamport_clock: self.lamport_clock.join(&other.lamport_clock),
            applied_operations: self.applied_operations.join(&other.applied_operations),
        }
    }
}

impl Bottom for AccountState {
    fn bottom() -> Self {
        let verifying_key = Ed25519SigningKey::from_bytes([0u8; 32])
            .verifying_key()
            .expect("static signing key should produce a valid verifying key");
        AccountState::new(AccountId::new_from_entropy([4u8; 32]), verifying_key)
    }
}

impl CvState for AccountState {}

impl JoinSemilattice for GuardianRegistry {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        // Merge guardians (later registration timestamp wins)
        for (email, guardian) in &other.guardians {
            if let Some(existing) = result.guardians.get(email) {
                use aura_core::time::{OrderingPolicy, TimeOrdering};
                if matches!(
                    guardian
                        .added_at
                        .compare(&existing.added_at, OrderingPolicy::DeterministicTieBreak),
                    TimeOrdering::After
                ) {
                    result.guardians.insert(email.clone(), guardian.clone());
                }
            } else {
                result.guardians.insert(email.clone(), guardian.clone());
            }
        }

        result
    }
}

impl Bottom for GuardianRegistry {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for GuardianRegistry {}

impl JoinSemilattice for MaxCounter {
    fn join(&self, other: &Self) -> Self {
        Self {
            value: self.value.max(other.value),
        }
    }
}

impl Bottom for MaxCounter {
    fn bottom() -> Self {
        Self::new()
    }
}

impl CvState for MaxCounter {}

impl Default for GuardianRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for MaxCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_state_creation() {
        let account_id = AccountId(uuid::Uuid::from_bytes([1u8; 16]));
        let (_sk, group_public_key) = aura_core::util::test_utils::test_key_pair(1);

        let state = AccountState::new(account_id, group_public_key);
        assert_eq!(state.get_epoch(), 0);
    }

    #[test]
    fn test_epoch_management() {
        let account_id = AccountId(uuid::Uuid::from_bytes([3u8; 16]));
        let (_sk, group_public_key) = aura_core::util::test_utils::test_key_pair(2);

        let mut state = AccountState::new(account_id, group_public_key);

        assert_eq!(state.get_epoch(), 0);

        state.increment_epoch();
        assert_eq!(state.get_epoch(), 1);

        state.set_epoch_if_higher(5);
        assert_eq!(state.get_epoch(), 5);

        state.set_epoch_if_higher(3); // Should not change
        assert_eq!(state.get_epoch(), 5);
    }

    #[test]
    fn test_join_semilattice() {
        let account_id = AccountId(uuid::Uuid::from_bytes([4u8; 16]));
        let (_sk, group_public_key) = aura_core::util::test_utils::test_key_pair(3);

        let mut state1 = AccountState::new(account_id, group_public_key.clone());
        let mut state2 = AccountState::new(account_id, group_public_key);

        state1.set_epoch_if_higher(3);
        state2.set_epoch_if_higher(5);

        // Join states
        let merged = state1.join(&state2);

        // Should have higher epoch
        assert_eq!(merged.get_epoch(), 5);
    }
}
