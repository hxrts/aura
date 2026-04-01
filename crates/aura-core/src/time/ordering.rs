//! TimeStamp comparison and sorting logic.

use super::{OrderingPolicy, TimeDomain, TimeIndex, TimeOrdering, TimeStamp};

impl TimeStamp {
    /// Convert TimeStamp to a domain-scoped index.
    ///
    /// This does not define a total order across domains. Callers must
    /// explicitly choose a tie-break policy when comparing different domains.
    pub fn to_index_ms(&self) -> TimeIndex {
        match self {
            TimeStamp::PhysicalClock(p) => TimeIndex::new(TimeDomain::PhysicalClock, p.ts_ms),
            TimeStamp::LogicalClock(l) => TimeIndex::new(TimeDomain::LogicalClock, l.lamport),
            TimeStamp::OrderClock(o) => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&o.0[..8]);
                TimeIndex::new(TimeDomain::OrderClock, u64::from_be_bytes(buf))
            }
            TimeStamp::Range(r) => TimeIndex::new(TimeDomain::Range, r.latest_ms()),
        }
    }

    /// Compare two timestamps for sorting with proper ordering semantics.
    pub fn sort_compare(&self, other: &TimeStamp, policy: OrderingPolicy) -> std::cmp::Ordering {
        match (self, other) {
            (TimeStamp::PhysicalClock(a), TimeStamp::PhysicalClock(b)) => a.ts_ms.cmp(&b.ts_ms),
            (TimeStamp::OrderClock(a), TimeStamp::OrderClock(b)) => a.0.cmp(&b.0),
            (TimeStamp::Range(a), TimeStamp::Range(b)) => {
                if a.latest_ms() < b.earliest_ms() {
                    std::cmp::Ordering::Less
                } else if b.latest_ms() < a.earliest_ms() {
                    std::cmp::Ordering::Greater
                } else {
                    a.latest_ms().cmp(&b.latest_ms())
                }
            }
            (TimeStamp::LogicalClock(_), TimeStamp::LogicalClock(_)) => {
                match self.compare(other, policy) {
                    TimeOrdering::Before => std::cmp::Ordering::Less,
                    TimeOrdering::After => std::cmp::Ordering::Greater,
                    TimeOrdering::Concurrent | TimeOrdering::Overlapping => {
                        std::cmp::Ordering::Equal
                    }
                    TimeOrdering::Incomparable => match policy {
                        OrderingPolicy::Native => std::cmp::Ordering::Equal,
                        OrderingPolicy::DeterministicTieBreak => {
                            self.to_index_ms().value().cmp(&other.to_index_ms().value())
                        }
                    },
                }
            }
            _ => match policy {
                OrderingPolicy::Native => std::cmp::Ordering::Equal,
                OrderingPolicy::DeterministicTieBreak => {
                    let self_index = self.to_index_ms();
                    let other_index = other.to_index_ms();
                    self_index.tie_break_key().cmp(&other_index.tie_break_key())
                }
            },
        }
    }

    /// Sort a collection of timestamps with domain-specific optimizations.
    pub fn sort_collection_optimized(
        timestamps: &mut [TimeStamp],
        policy: OrderingPolicy,
        stable: bool,
    ) {
        if let Some(first) = timestamps.first() {
            let all_same_variant = timestamps
                .iter()
                .all(|ts| std::mem::discriminant(ts) == std::mem::discriminant(first));

            if all_same_variant {
                match first {
                    TimeStamp::PhysicalClock(_) => {
                        let cmp = |a: &TimeStamp, b: &TimeStamp| {
                            if let (TimeStamp::PhysicalClock(x), TimeStamp::PhysicalClock(y)) =
                                (a, b)
                            {
                                x.ts_ms.cmp(&y.ts_ms)
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        };
                        if stable {
                            timestamps.sort_by(cmp);
                        } else {
                            timestamps.sort_unstable_by(cmp);
                        }
                    }
                    _ => {
                        if stable {
                            timestamps.sort_by(|a, b| a.sort_compare(b, policy));
                        } else {
                            timestamps.sort_unstable_by(|a, b| a.sort_compare(b, policy));
                        }
                    }
                }
            } else if stable {
                timestamps.sort_by(|a, b| a.sort_compare(b, policy));
            } else {
                timestamps.sort_unstable_by(|a, b| a.sort_compare(b, policy));
            }
        }
    }

    /// Compare two timestamps using the specified policy.
    pub fn compare(&self, other: &TimeStamp, policy: OrderingPolicy) -> TimeOrdering {
        match (self, other) {
            (TimeStamp::PhysicalClock(a), TimeStamp::PhysicalClock(b)) => {
                if a.ts_ms < b.ts_ms {
                    TimeOrdering::Before
                } else if a.ts_ms > b.ts_ms {
                    TimeOrdering::After
                } else {
                    TimeOrdering::Concurrent
                }
            }
            (TimeStamp::LogicalClock(a), TimeStamp::LogicalClock(b)) => {
                match a.vector.partial_cmp(&b.vector) {
                    Some(std::cmp::Ordering::Less) => TimeOrdering::Before,
                    Some(std::cmp::Ordering::Greater) => TimeOrdering::After,
                    Some(std::cmp::Ordering::Equal) => TimeOrdering::Concurrent,
                    None => match policy {
                        OrderingPolicy::Native => TimeOrdering::Incomparable,
                        OrderingPolicy::DeterministicTieBreak => TimeOrdering::Concurrent,
                    },
                }
            }
            (TimeStamp::Range(a), TimeStamp::Range(b)) => {
                if a.latest_ms() < b.earliest_ms() {
                    TimeOrdering::Before
                } else if b.latest_ms() < a.earliest_ms() {
                    TimeOrdering::After
                } else {
                    TimeOrdering::Overlapping
                }
            }
            (TimeStamp::OrderClock(a), TimeStamp::OrderClock(b)) => match a.0.cmp(&b.0) {
                std::cmp::Ordering::Less => TimeOrdering::Before,
                std::cmp::Ordering::Greater => TimeOrdering::After,
                std::cmp::Ordering::Equal => TimeOrdering::Concurrent,
            },
            _ => TimeOrdering::Incomparable,
        }
    }
}
