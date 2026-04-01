//! Pure time comparison functions for Aeneas translation
//!
//! This module exposes `TimeStamp::compare` as free functions for potential
//! Aeneas translation. The Lean model in `verification/lean/Aura/TimeSystem.lean`
//! proves reflexivity, transitivity, and privacy (OrderClock hides physical time).

use crate::time::{OrderingPolicy, TimeOrdering, TimeStamp};

/// Pure timestamp comparison function.
///
/// Thin wrapper around `TimeStamp::compare()` exposed as a free function
/// for easier Aeneas targeting.
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
    use crate::time::{LogicalTime, OrderTime, PhysicalTime, VectorClock};

    #[test]
    fn test_reflexive_physical() {
        let t = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        assert_eq!(
            timestamp_compare(&t, &t, OrderingPolicy::Native),
            TimeOrdering::Concurrent
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

    #[test]
    fn test_transitive_physical() {
        let a = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 100,
            uncertainty: None,
        });
        let b = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 200,
            uncertainty: None,
        });
        let c = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 300,
            uncertainty: None,
        });

        assert_eq!(
            timestamp_compare(&a, &b, OrderingPolicy::Native),
            TimeOrdering::Before
        );
        assert_eq!(
            timestamp_compare(&b, &c, OrderingPolicy::Native),
            TimeOrdering::Before
        );
        assert_eq!(
            timestamp_compare(&a, &c, OrderingPolicy::Native),
            TimeOrdering::Before
        );
    }

    #[test]
    fn test_transitive_order_clock() {
        let a = TimeStamp::OrderClock(OrderTime([1u8; 32]));
        let b = TimeStamp::OrderClock(OrderTime([2u8; 32]));
        let c = TimeStamp::OrderClock(OrderTime([3u8; 32]));

        assert_eq!(
            timestamp_compare(&a, &b, OrderingPolicy::Native),
            TimeOrdering::Before
        );
        assert_eq!(
            timestamp_compare(&b, &c, OrderingPolicy::Native),
            TimeOrdering::Before
        );
        assert_eq!(
            timestamp_compare(&a, &c, OrderingPolicy::Native),
            TimeOrdering::Before
        );
    }

    #[test]
    fn test_cross_domain_incomparable() {
        let physical = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let order = TimeStamp::OrderClock(OrderTime([1u8; 32]));

        assert_eq!(
            timestamp_compare(&physical, &order, OrderingPolicy::Native),
            TimeOrdering::Incomparable
        );
    }

    #[test]
    fn test_deterministic_tiebreak_policy() {
        use crate::types::identifiers::DeviceId;

        let device = DeviceId::new_from_entropy([1u8; 32]);
        let mut v1 = VectorClock::new();
        v1.insert(device, 1);

        let device2 = DeviceId::new_from_entropy([2u8; 32]);
        let mut v2 = VectorClock::new();
        v2.insert(device2, 1);

        let t1 = TimeStamp::LogicalClock(LogicalTime {
            vector: v1,
            lamport: 1,
        });
        let t2 = TimeStamp::LogicalClock(LogicalTime {
            vector: v2,
            lamport: 1,
        });

        assert_eq!(
            timestamp_compare(&t1, &t2, OrderingPolicy::Native),
            TimeOrdering::Incomparable
        );

        assert_eq!(
            timestamp_compare(&t1, &t2, OrderingPolicy::DeterministicTieBreak),
            TimeOrdering::Concurrent
        );
    }
}
