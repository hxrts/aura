//! Clean test fixtures that don't violate architectural boundaries
//!
//! This module provides test utilities that follow the clean architecture
//! principles by not depending on the effect system for basic operations
//! like ID generation.

#![allow(clippy::disallowed_methods)]

use aura_core::{AccountId, DeviceId};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

/// Monotonic counter for deterministic-but-unique test IDs.
static DEVICE_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACCOUNT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Clean test fixtures that don't violate architectural boundaries
pub struct TestFixtures;

impl TestFixtures {
    /// Generate a unique device ID for tests (deterministic via counter)
    pub fn device_id() -> DeviceId {
        let n = DEVICE_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self::device_id_from_seed(n)
    }

    /// Generate device ID from seed for reproducible tests
    pub fn device_id_from_seed(seed: u64) -> DeviceId {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&seed.to_le_bytes());
        DeviceId(Uuid::from_bytes(bytes))
    }

    /// Generate a unique account ID for tests (deterministic via counter)
    pub fn account_id() -> AccountId {
        let n = ACCOUNT_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self::account_id_from_seed(n)
    }

    /// Generate account ID from seed for reproducible tests
    pub fn account_id_from_seed(seed: u64) -> AccountId {
        let mut bytes = [0u8; 16];
        bytes[8..].copy_from_slice(&seed.to_le_bytes());
        AccountId(Uuid::from_bytes(bytes))
    }

    /// Create account and device pair for tests
    pub fn test_account() -> (AccountId, DeviceId) {
        (Self::account_id(), Self::device_id())
    }

    /// Create account and device pair from seeds for reproducible tests
    pub fn test_account_from_seeds(account_seed: u64, device_seed: u64) -> (AccountId, DeviceId) {
        (
            Self::account_id_from_seed(account_seed),
            Self::device_id_from_seed(device_seed),
        )
    }

    /// Create multiple device IDs from a base seed
    pub fn device_ids_from_base_seed(base_seed: u64, count: usize) -> Vec<DeviceId> {
        (0..count)
            .map(|i| Self::device_id_from_seed(base_seed + i as u64))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id_generation() {
        let id1 = TestFixtures::device_id();
        let id2 = TestFixtures::device_id();

        // Should generate different IDs
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_deterministic_device_id() {
        let id1 = TestFixtures::device_id_from_seed(42);
        let id2 = TestFixtures::device_id_from_seed(42);

        // Should generate same ID from same seed
        assert_eq!(id1, id2);

        let id3 = TestFixtures::device_id_from_seed(43);
        // Should generate different ID from different seed
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_account_creation() {
        let (account_id, device_id) = TestFixtures::test_account();

        // Should generate valid IDs
        assert!(!account_id.0.is_nil());
        assert!(!device_id.0.is_nil());

        // Should be different
        assert_ne!(account_id.0, device_id.0);
    }

    #[test]
    fn test_deterministic_account_creation() {
        let (account1, device1) = TestFixtures::test_account_from_seeds(100, 200);
        let (account2, device2) = TestFixtures::test_account_from_seeds(100, 200);

        // Should generate same IDs from same seeds
        assert_eq!(account1, account2);
        assert_eq!(device1, device2);
    }

    #[test]
    fn test_multiple_device_ids() {
        let devices = TestFixtures::device_ids_from_base_seed(1000, 5);

        assert_eq!(devices.len(), 5);

        // All should be unique
        for i in 0..devices.len() {
            for j in (i + 1)..devices.len() {
                assert_ne!(devices[i], devices[j]);
            }
        }

        // Should be deterministic
        let devices2 = TestFixtures::device_ids_from_base_seed(1000, 5);
        assert_eq!(devices, devices2);
    }
}
