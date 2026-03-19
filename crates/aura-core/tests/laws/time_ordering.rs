//! Property tests for time ordering invariants across all four clock domains.
//!
//! If time ordering is wrong, causal consistency breaks (LogicalClock),
//! privacy-preserving ordering leaks information (OrderClock), validity
//! windows admit expired data (Range), and wall-clock comparisons invert
//! (PhysicalClock).

use aura_core::time::{
    LogicalTime, OrderTime, OrderingPolicy, PhysicalTime, RangeTime, ScalarClock, TimeConfidence,
    TimeOrdering, TimeStamp, VectorClock,
};
use aura_core::DeviceId;
use proptest::prelude::*;

// ============================================================================
// PhysicalClock: total order over wall-clock milliseconds
// ============================================================================

proptest! {
    /// PhysicalClock compare must agree with u64 ordering.
    #[test]
    fn physical_clock_compare_matches_u64_order(a in any::<u64>(), b in any::<u64>()) {
        let t1 = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: a, uncertainty: None });
        let t2 = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: b, uncertainty: None });

        let ordering = t1.compare(&t2, OrderingPolicy::Native);
        match a.cmp(&b) {
            std::cmp::Ordering::Less => prop_assert_eq!(ordering, TimeOrdering::Before),
            std::cmp::Ordering::Greater => prop_assert_eq!(ordering, TimeOrdering::After),
            std::cmp::Ordering::Equal => prop_assert_eq!(ordering, TimeOrdering::Concurrent),
        }
    }

    /// PhysicalClock ordering is antisymmetric: if a < b then b > a.
    #[test]
    fn physical_clock_antisymmetric(a in any::<u64>(), b in any::<u64>()) {
        let t1 = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: a, uncertainty: None });
        let t2 = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: b, uncertainty: None });

        let fwd = t1.compare(&t2, OrderingPolicy::Native);
        let rev = t2.compare(&t1, OrderingPolicy::Native);

        match fwd {
            TimeOrdering::Before => prop_assert_eq!(rev, TimeOrdering::After),
            TimeOrdering::After => prop_assert_eq!(rev, TimeOrdering::Before),
            TimeOrdering::Concurrent => prop_assert_eq!(rev, TimeOrdering::Concurrent),
            _ => {} // shouldn't happen for physical clocks
        }
    }
}

// ============================================================================
// OrderClock: total order over opaque 32-byte tokens — no timing leakage
// ============================================================================

proptest! {
    /// OrderClock ordering is a total order over byte arrays.
    #[test]
    fn order_clock_total_order(a in any::<[u8; 32]>(), b in any::<[u8; 32]>()) {
        let t1 = TimeStamp::OrderClock(OrderTime(a));
        let t2 = TimeStamp::OrderClock(OrderTime(b));

        let ordering = t1.compare(&t2, OrderingPolicy::Native);
        match a.cmp(&b) {
            std::cmp::Ordering::Less => prop_assert_eq!(ordering, TimeOrdering::Before),
            std::cmp::Ordering::Greater => prop_assert_eq!(ordering, TimeOrdering::After),
            std::cmp::Ordering::Equal => prop_assert_eq!(ordering, TimeOrdering::Concurrent),
        }
    }

    /// OrderClock is antisymmetric.
    #[test]
    fn order_clock_antisymmetric(a in any::<[u8; 32]>(), b in any::<[u8; 32]>()) {
        let t1 = TimeStamp::OrderClock(OrderTime(a));
        let t2 = TimeStamp::OrderClock(OrderTime(b));

        let fwd = t1.compare(&t2, OrderingPolicy::Native);
        let rev = t2.compare(&t1, OrderingPolicy::Native);

        match fwd {
            TimeOrdering::Before => prop_assert_eq!(rev, TimeOrdering::After),
            TimeOrdering::After => prop_assert_eq!(rev, TimeOrdering::Before),
            TimeOrdering::Concurrent => prop_assert_eq!(rev, TimeOrdering::Concurrent),
            _ => {}
        }
    }
}

// ============================================================================
// VectorClock: partial order — concurrent operations are valid
// ============================================================================

proptest! {
    /// Incrementing a vector clock always produces a strictly greater clock.
    #[test]
    fn vector_clock_increment_is_monotonic(counter in 0u64..u64::MAX) {
        let device = DeviceId::new_from_entropy([42u8; 32]);
        let mut clock = VectorClock::single(device, counter);
        let before = clock.clone();

        clock.increment(device).unwrap();
        prop_assert_eq!(before.partial_cmp(&clock), Some(std::cmp::Ordering::Less));
    }
}

// ============================================================================
// RangeTime: interval overlap semantics
// ============================================================================

proptest! {
    /// Non-overlapping ranges compare strictly: if a.latest < b.earliest
    /// then a is Before b. Tests the core validity-window property.
    #[test]
    fn range_non_overlapping_is_strict(
        a_start in 0u64..1_000_000,
        a_len in 1u64..1_000,
        gap in 1u64..1_000,
        b_len in 1u64..1_000,
    ) {
        let a_end = a_start.saturating_add(a_len);
        let b_start = a_end.saturating_add(gap);
        let b_end = b_start.saturating_add(b_len);

        let r1 = TimeStamp::Range(RangeTime::new(a_start, a_end, TimeConfidence::High).unwrap());
        let r2 = TimeStamp::Range(RangeTime::new(b_start, b_end, TimeConfidence::High).unwrap());

        prop_assert_eq!(r1.compare(&r2, OrderingPolicy::Native), TimeOrdering::Before);
        prop_assert_eq!(r2.compare(&r1, OrderingPolicy::Native), TimeOrdering::After);
    }

    /// Overlapping ranges return Overlapping, not Before or After.
    #[test]
    fn range_overlapping_is_symmetric(
        start in 0u64..1_000_000,
        len in 2u64..1_000,
    ) {
        let mid = start.saturating_add(len / 2);
        let end = start.saturating_add(len);

        let r1 = TimeStamp::Range(RangeTime::new(start, mid.saturating_add(1), TimeConfidence::High).unwrap());
        let r2 = TimeStamp::Range(RangeTime::new(mid, end, TimeConfidence::High).unwrap());

        // Both directions should agree they overlap
        prop_assert_eq!(r1.compare(&r2, OrderingPolicy::Native), TimeOrdering::Overlapping);
        prop_assert_eq!(r2.compare(&r1, OrderingPolicy::Native), TimeOrdering::Overlapping);
    }
}

// ============================================================================
// Cross-domain: different domains are always incomparable
// ============================================================================

/// Comparing timestamps from different domains must return Incomparable.
/// This is a security property: physical time must not be confused with
/// logical ordering, and order clocks must not leak into causal comparisons.
#[test]
fn cross_domain_always_incomparable() {
    let physical = TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1000,
        uncertainty: None,
    });
    let logical = TimeStamp::LogicalClock(LogicalTime {
        vector: VectorClock::default(),
        lamport: ScalarClock::default(),
    });
    let order = TimeStamp::OrderClock(OrderTime([1u8; 32]));
    let range = TimeStamp::Range(RangeTime::new(0, 100, TimeConfidence::High).unwrap());

    let all = [&physical, &logical, &order, &range];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            if i != j {
                assert_eq!(
                    a.compare(b, OrderingPolicy::Native),
                    TimeOrdering::Incomparable,
                    "cross-domain comparison {i} vs {j} must be Incomparable"
                );
            }
        }
    }
}
