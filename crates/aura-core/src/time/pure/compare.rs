//! Pure timestamp comparison function - thin wrapper for Aeneas translation
//!
//! This module exposes the actual `TimeStamp::compare` implementation as a
//! free function for potential Aeneas translation.
//!
//! # Actual Implementation
//!
//! The real comparison logic is in `super::TimeStamp::compare()`. This module
//! provides a thin wrapper that exposes it as a free function.
//!
//! # Lean Correspondence
//!
//! The Lean model in `verification/lean/Aura/TimeSystem.lean`:
//!
//! ```lean
//! def compare (policy : Policy) (a b : TimeStamp) : Ordering :=
//!   if policy.ignorePhysical then
//!     compareNat a.logical b.logical
//!   else
//!     match compareNat a.logical b.logical with
//!     | .lt => .lt
//!     | .gt => .gt
//!     | .eq => compareNat a.orderClock b.orderClock
//! ```
//!
//! # Proven Properties
//!
//! - `compare_refl`: Reflexivity
//! - `compare_trans`: Transitivity for lt
//! - `physical_hidden`: Privacy when ignorePhysical=true

use crate::time::{OrderingPolicy, TimeOrdering, TimeStamp};

/// Pure timestamp comparison function.
///
/// This is a thin wrapper around `TimeStamp::compare()` exposed as a free
/// function for easier Aeneas targeting.
///
/// # Lean Correspondence
///
/// Maps to the Lean `compare` function. The Lean model proves:
/// - **Reflexivity**: `compare policy t t = Ordering.eq`
/// - **Transitivity**: `compare policy a b = .lt → compare policy b c = .lt → compare policy a c = .lt`
/// - **Privacy**: Physical time is hidden when policy ignores it
///
/// # Arguments
///
/// * `a` - First timestamp
/// * `b` - Second timestamp
/// * `policy` - Ordering policy (Native or DeterministicTieBreak)
///
/// # Returns
///
/// The ordering relationship between the timestamps.
#[inline]
pub fn timestamp_compare(a: &TimeStamp, b: &TimeStamp, policy: OrderingPolicy) -> TimeOrdering {
    a.compare(b, policy)
}

/// Sort comparison wrapper.
///
/// Wrapper around `TimeStamp::sort_compare()` for use in sorting operations.
#[inline]
pub fn timestamp_sort_compare(
    a: &TimeStamp,
    b: &TimeStamp,
    policy: OrderingPolicy,
) -> std::cmp::Ordering {
    a.sort_compare(b, policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{OrderTime, PhysicalTime, LogicalTime, VectorClock};

    // ==========================================================
    // Reflexivity tests (compare_refl in Lean)
    // ==========================================================

    #[test]
    fn test_reflexive_physical() {
        let t = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        assert_eq!(
            timestamp_compare(&t, &t, OrderingPolicy::Native),
            TimeOrdering::Concurrent // Same time = concurrent
        );
    }

    #[test]
    fn test_reflexive_order_clock() {
        let t = TimeStamp::OrderClock(OrderTime([42u8; 32]));
        assert_eq!(
            timestamp_compare(&t, &t, OrderingPolicy::Native),
            TimeOrdering::Concurrent
        );
    }

    // ==========================================================
    // Transitivity tests (compare_trans in Lean)
    // ==========================================================

    #[test]
    fn test_transitive_physical() {
        let a = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: 100, uncertainty: None });
        let b = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: 200, uncertainty: None });
        let c = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: 300, uncertainty: None });

        assert_eq!(timestamp_compare(&a, &b, OrderingPolicy::Native), TimeOrdering::Before);
        assert_eq!(timestamp_compare(&b, &c, OrderingPolicy::Native), TimeOrdering::Before);
        assert_eq!(timestamp_compare(&a, &c, OrderingPolicy::Native), TimeOrdering::Before);
    }

    #[test]
    fn test_transitive_order_clock() {
        let a = TimeStamp::OrderClock(OrderTime([1u8; 32]));
        let b = TimeStamp::OrderClock(OrderTime([2u8; 32]));
        let c = TimeStamp::OrderClock(OrderTime([3u8; 32]));

        assert_eq!(timestamp_compare(&a, &b, OrderingPolicy::Native), TimeOrdering::Before);
        assert_eq!(timestamp_compare(&b, &c, OrderingPolicy::Native), TimeOrdering::Before);
        assert_eq!(timestamp_compare(&a, &c, OrderingPolicy::Native), TimeOrdering::Before);
    }

    // ==========================================================
    // Cross-domain tests
    // ==========================================================

    #[test]
    fn test_cross_domain_incomparable() {
        let physical = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: 1000, uncertainty: None });
        let order = TimeStamp::OrderClock(OrderTime([1u8; 32]));

        assert_eq!(
            timestamp_compare(&physical, &order, OrderingPolicy::Native),
            TimeOrdering::Incomparable
        );
    }

    // ==========================================================
    // Policy tests
    // ==========================================================

    #[test]
    fn test_deterministic_tiebreak_policy() {
        use crate::types::identifiers::DeviceId;

        let device = DeviceId::new_from_entropy([1u8; 32]);
        let mut v1 = VectorClock::new();
        v1.insert(device, 1);

        let device2 = DeviceId::new_from_entropy([2u8; 32]);
        let mut v2 = VectorClock::new();
        v2.insert(device2, 1);

        let t1 = TimeStamp::LogicalClock(LogicalTime { vector: v1, lamport: 1 });
        let t2 = TimeStamp::LogicalClock(LogicalTime { vector: v2, lamport: 1 });

        // Native policy returns Incomparable for concurrent vector clocks
        assert_eq!(
            timestamp_compare(&t1, &t2, OrderingPolicy::Native),
            TimeOrdering::Incomparable
        );

        // DeterministicTieBreak returns Concurrent
        assert_eq!(
            timestamp_compare(&t1, &t2, OrderingPolicy::DeterministicTieBreak),
            TimeOrdering::Concurrent
        );
    }
}
