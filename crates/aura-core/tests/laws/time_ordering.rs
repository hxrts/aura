//! Property tests for time ordering and vector clock invariants.

use aura_core::time::{OrderingPolicy, PhysicalTime, TimeOrdering, TimeStamp, VectorClock};
use aura_core::DeviceId;
use proptest::prelude::*;

proptest! {
    #[test]
    fn physical_time_compare_matches_order(a in any::<u64>(), b in any::<u64>()) {
        let t1 = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: a, uncertainty: None });
        let t2 = TimeStamp::PhysicalClock(PhysicalTime { ts_ms: b, uncertainty: None });

        let ordering = t1.compare(&t2, OrderingPolicy::Native);
        if a < b {
            prop_assert_eq!(ordering, TimeOrdering::Before);
        } else if a > b {
            prop_assert_eq!(ordering, TimeOrdering::After);
        } else {
            prop_assert_eq!(ordering, TimeOrdering::Concurrent);
        }
    }

    #[test]
    fn vector_clock_increment_is_monotonic(counter in 0u64..u64::MAX) {
        let device = DeviceId::new_from_entropy([42u8; 32]);
        let mut clock = VectorClock::single(device, counter);
        let before = clock.clone();

        clock.increment(device).unwrap();
        let before_count = before.get(&device).copied().unwrap();
        let after_count = clock.get(&device).copied().unwrap();

        prop_assert_eq!(after_count, before_count + 1);
        prop_assert_eq!(before.partial_cmp(&clock), Some(std::cmp::Ordering::Less));
    }
}
