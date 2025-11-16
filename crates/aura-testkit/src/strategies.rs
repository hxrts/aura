//! Property test strategies for Aura types
//!
//! This module provides proptest strategies for generating deterministic test data
//! for core Aura types. These strategies are designed to be composable and reusable
//! across all Layer 4+ crates.
//!
//! # Architecture Note
//!
//! These strategies are for cross-cutting concerns (DeviceId, AccountId, etc.).
//! Domain-specific strategies should be added here if used by 3+ Layer 4+ crates.

use proptest::prelude::*;

// Re-export proptest for convenience
pub use proptest;

use aura_core::tree::{AttestedOp, LeafId, LeafNode, LeafRole, NodeIndex, TreeOp, TreeOpKind};
use aura_core::{AccountId, DeviceId, SessionId};
use aura_journal::semilattice::OpLog;
use ed25519_dalek::{SigningKey, VerifyingKey};

/// Strategy for generating deterministic DeviceIds
///
/// Generates DeviceIds from seed values in the range 0..10000, ensuring
/// deterministic and reproducible test data.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_device_id;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_device_property(device_id in arb_device_id()) {
///         assert_ne!(device_id.to_string(), "");
///     }
/// }
/// ```
pub fn arb_device_id() -> impl Strategy<Value = DeviceId> {
    (0u64..10000).prop_map(|seed| {
        // Use aura-core's test_utils for deterministic generation
        // This works because tests can access test_utils via #[cfg(test)]
        use aura_core::hash::hash;
        use uuid::Uuid;
        let hash_input = format!("device-{}", seed);
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
        DeviceId(Uuid::from_bytes(uuid_bytes))
    })
}

/// Strategy for generating deterministic AccountIds
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_account_id;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_account_property(account_id in arb_account_id()) {
///         assert_ne!(account_id.to_string(), "");
///     }
/// }
/// ```
pub fn arb_account_id() -> impl Strategy<Value = AccountId> {
    (0u64..10000).prop_map(|seed| {
        use aura_core::hash::hash;
        use uuid::Uuid;
        let hash_input = format!("account-{}", seed);
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
        AccountId(Uuid::from_bytes(uuid_bytes))
    })
}

/// Strategy for generating deterministic SessionIds
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_session_id;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_session_property(session_id in arb_session_id()) {
///         assert_ne!(session_id.to_string(), "");
///     }
/// }
/// ```
pub fn arb_session_id() -> impl Strategy<Value = SessionId> {
    (0u64..10000).prop_map(|seed| {
        use aura_core::hash::hash;
        use uuid::Uuid;
        let hash_input = format!("session-{}", seed);
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
        SessionId(Uuid::from_bytes(uuid_bytes))
    })
}

/// Strategy for generating deterministic Ed25519 key pairs
///
/// Returns (SigningKey, VerifyingKey) tuples for testing.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_key_pair;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_key_property((sk, vk) in arb_key_pair()) {
///         assert_eq!(sk.verifying_key(), vk);
///     }
/// }
/// ```
pub fn arb_key_pair() -> impl Strategy<Value = (SigningKey, VerifyingKey)> {
    (0u64..10000).prop_map(|seed| {
        let mut key_bytes = [0u8; 32];
        key_bytes[..8].copy_from_slice(&seed.to_le_bytes());
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();
        (signing_key, verifying_key)
    })
}

/// Strategy for generating realistic timestamps
///
/// Generates timestamps in the range from 2020-09-13 to 2027-01-10,
/// representing realistic time values for testing.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_timestamp;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_timestamp_property(ts in arb_timestamp()) {
///         assert!(ts > 1_600_000_000);
///         assert!(ts < 1_800_000_000);
///     }
/// }
/// ```
pub fn arb_timestamp() -> impl Strategy<Value = u64> {
    1_600_000_000u64..1_800_000_000u64
}

/// Strategy for generating non-empty byte vectors
///
/// Useful for testing data payloads, messages, etc.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_non_empty_bytes;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_bytes_property(data in arb_non_empty_bytes(1, 100)) {
///         assert!(!data.is_empty());
///         assert!(data.len() <= 100);
///     }
/// }
/// ```
pub fn arb_non_empty_bytes(min: usize, max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), min..=max)
}

/// Strategy for generating non-empty vectors of any type
///
/// Generic helper for creating non-empty collections.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::{arb_non_empty_vec, arb_device_id};
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_device_list(devices in arb_non_empty_vec(arb_device_id(), 1, 10)) {
///         assert!(!devices.is_empty());
///         assert!(devices.len() <= 10);
///     }
/// }
/// ```
pub fn arb_non_empty_vec<T: Strategy>(
    element: T,
    min: usize,
    max: usize,
) -> impl Strategy<Value = Vec<T::Value>> {
    prop::collection::vec(element, min..=max)
}

/// Strategy for generating small positive integers (useful for counts, indices)
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_small_count;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_count_property(count in arb_small_count()) {
///         assert!(count > 0);
///         assert!(count <= 100);
///     }
/// }
/// ```
pub fn arb_small_count() -> impl Strategy<Value = usize> {
    1usize..=100usize
}

/// Strategy for generating device counts suitable for threshold scenarios
///
/// Returns (threshold, total) tuples where threshold <= total.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_threshold_config;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_threshold_property((threshold, total) in arb_threshold_config()) {
///         assert!(threshold <= total);
///         assert!(threshold > 0);
///         assert!(total > 0);
///     }
/// }
/// ```
pub fn arb_threshold_config() -> impl Strategy<Value = (u16, u16)> {
    (1u16..=10u16).prop_flat_map(|threshold| {
        (Just(threshold), threshold..=20u16)
    })
}

// ============================================================================
// CRDT and Tree Operation Strategies
// ============================================================================

/// Helper function to create a test TreeOp with deterministic values
///
/// This is used by the OpLog strategy and can be used directly in tests.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::create_test_tree_op;
///
/// let op = create_test_tree_op([0u8; 32], 1, 42);
/// assert_eq!(op.op.parent_epoch, 1);
/// ```
pub fn create_test_tree_op(commitment: [u8; 32], epoch: u64, leaf_id: u64) -> AttestedOp {
    AttestedOp {
        op: TreeOp {
            parent_commitment: commitment,
            parent_epoch: epoch,
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode {
                    leaf_id: LeafId(
                        leaf_id
                            .try_into()
                            .unwrap_or_else(|e| panic!("Invalid leaf_id: {}", e)),
                    ),
                    device_id: {
                        use aura_core::hash::hash;
                        use uuid::Uuid;
                        let hash_input = format!("device-{}", leaf_id);
                        let hash_bytes = hash(hash_input.as_bytes());
                        let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
                        DeviceId(Uuid::from_bytes(uuid_bytes))
                    },
                    role: LeafRole::Device,
                    public_key: vec![1, 2, 3],
                    meta: vec![],
                },
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 0,
    }
}

/// Strategy for generating OpLog instances
///
/// Generates OpLog with 0-10 operations for property testing.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_oplog;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_oplog_property(oplog in arb_oplog()) {
///         // Test CRDT properties...
///     }
/// }
/// ```
pub fn arb_oplog() -> impl Strategy<Value = OpLog> {
    prop::collection::vec(
        (prop::array::uniform32(any::<u8>()), 1u64..=10, 1u64..=100),
        0..=10,
    )
    .prop_map(|ops| {
        let mut oplog = OpLog::new();
        for (commitment, epoch, leaf_id) in ops {
            oplog.add_operation(create_test_tree_op(commitment, epoch, leaf_id));
        }
        oplog
    })
}

/// Strategy for generating AttestedOp instances
///
/// Generates individual tree operations for property testing.
///
/// # Example
///
/// ```rust
/// use aura_testkit::strategies::arb_attested_op;
/// use proptest::prelude::*;
///
/// proptest! {
///     #[test]
///     fn test_op_property(op in arb_attested_op()) {
///         assert_eq!(op.op.version, 1);
///     }
/// }
/// ```
pub fn arb_attested_op() -> impl Strategy<Value = AttestedOp> {
    (prop::array::uniform32(any::<u8>()), 1u64..=10, 1u64..=100)
        .prop_map(|(commitment, epoch, leaf_id)| create_test_tree_op(commitment, epoch, leaf_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #[test]
        fn test_arb_device_id_deterministic(seed in 0u64..100) {
            // Same seed should produce same device ID
            let strategy = arb_device_id();
            // We can't easily test determinism with proptest, but we can test validity
            proptest!(|(id in strategy)| {
                assert_ne!(id.to_string(), "");
            });
        }

        #[test]
        fn test_arb_account_id_valid(id in arb_account_id()) {
            assert_ne!(id.to_string(), "");
        }

        #[test]
        fn test_arb_session_id_valid(id in arb_session_id()) {
            assert_ne!(id.to_string(), "");
        }

        #[test]
        fn test_arb_key_pair_valid((sk, vk) in arb_key_pair()) {
            assert_eq!(sk.verifying_key(), vk);
        }

        #[test]
        fn test_arb_timestamp_range(ts in arb_timestamp()) {
            assert!(ts >= 1_600_000_000);
            assert!(ts <= 1_800_000_000);
        }

        #[test]
        fn test_arb_non_empty_bytes(data in arb_non_empty_bytes(1, 100)) {
            assert!(!data.is_empty());
            assert!(data.len() <= 100);
        }

        #[test]
        fn test_arb_threshold_config((threshold, total) in arb_threshold_config()) {
            assert!(threshold <= total);
            assert!(threshold > 0);
            assert!(total > 0);
        }
    }
}
