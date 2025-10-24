//! Test Ledger Utilities
//!
//! Factory functions for creating test AccountLedger instances.
//! Consolidates ledger creation patterns found in multiple test files.

use aura_crypto::Effects;
use aura_journal::{AccountLedger, AccountState, AccountId};
use crate::account::test_account_with_effects;

/// Create a test ledger with given effects
/// 
/// Standard pattern for creating test ledgers with a basic account state.
/// 
/// # Arguments
/// * `effects` - Effects instance for deterministic generation
pub fn test_ledger_with_effects(effects: &Effects) -> AccountLedger {
    let account_state = test_account_with_effects(effects);
    AccountLedger::new(account_state).expect("Ledger creation should succeed")
}

/// Create a test ledger with seed
/// 
/// Convenience function that creates effects and ledger in one call.
/// 
/// # Arguments
/// * `seed` - Random seed for deterministic generation
pub fn test_ledger_with_seed(seed: u64) -> AccountLedger {
    let effects = Effects::deterministic(seed, 1000);
    test_ledger_with_effects(&effects)
}

/// Create a test ledger with specific account state
/// 
/// For tests that need to control the initial account state.
/// 
/// # Arguments
/// * `account_state` - Specific account state to use
pub fn test_ledger_with_state(account_state: AccountState) -> AccountLedger {
    AccountLedger::new(account_state).expect("Ledger creation should succeed")
}

/// Create an empty test ledger
/// 
/// For tests that need to start with a minimal ledger.
pub fn test_ledger_empty() -> AccountLedger {
    let effects = Effects::test();
    test_ledger_with_effects(&effects)
}

/// Create multiple test ledgers with different seeds
/// 
/// Useful for CRDT merge testing where you need multiple independent ledgers.
/// 
/// # Arguments
/// * `count` - Number of ledgers to create
/// * `base_seed` - Base seed (each ledger gets base_seed + index)
pub fn test_ledgers_multiple(count: usize, base_seed: u64) -> Vec<AccountLedger> {
    (0..count)
        .map(|i| test_ledger_with_seed(base_seed + i as u64))
        .collect()
}

/// Create a test ledger with specific account ID
/// 
/// For tests that need predictable account IDs.
/// 
/// # Arguments
/// * `account_id` - Specific account ID to use
/// * `effects` - Effects instance for other random generation
pub fn test_ledger_with_account_id(account_id: AccountId, effects: &Effects) -> AccountLedger {
    use crate::account::test_account_with_id;
    let account_state = test_account_with_id(account_id, effects);
    AccountLedger::new(account_state).expect("Ledger creation should succeed")
}

/// Create a test ledger with custom threshold
/// 
/// For testing different threshold configurations.
/// 
/// # Arguments
/// * `threshold` - M-of-N threshold value
/// * `total` - Total number of participants
/// * `effects` - Effects instance for deterministic generation
pub fn test_ledger_with_threshold(threshold: u16, total: u16, effects: &Effects) -> AccountLedger {
    use crate::account::test_account_with_threshold;
    let account_state = test_account_with_threshold(effects, threshold, total);
    AccountLedger::new(account_state).expect("Ledger creation should succeed")
}