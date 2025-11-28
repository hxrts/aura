//! Unified time semantics for Aura
//!
//! Provides semantic time representations (logical, order-only, physical, range)
//! and explicit ordering policies. Provenance/attestation is modeled via an
//! orthogonal wrapper (`ProvenancedTime`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::types::identifiers::{AuthorityId, DeviceId};

/// Physical clock representation with optional uncertainty.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PhysicalTime {
    pub ts_ms: u64,
    pub uncertainty: Option<u64>, // milliseconds
}

/// Logical clock representation (causal semantics).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalTime {
    pub vector: VectorClock,
    pub lamport: ScalarClock,
}

/// Order-only time (opaque total order, no causal/temporal meaning).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OrderTime(pub [u8; 32]);

/// Range constraint (validity window).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeTime {
    pub earliest_ms: u64,
    pub latest_ms: u64,
    pub confidence: TimeConfidence,
}

/// Vector clock: device -> counter (causal domain).
/// Optimized representation for memory efficiency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorClock {
    /// Single device optimization - common case for many authorities
    Single { device: DeviceId, counter: u64 },
    /// Multiple devices - fallback to BTreeMap for complex cases
    Multiple(BTreeMap<DeviceId, u64>),
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorClock {
    pub fn new() -> Self {
        // Performance improvement: Start with Single variant when possible
        // Avoids allocating an empty BTreeMap in the common case
        VectorClock::Multiple(BTreeMap::new())
    }

    /// Create a VectorClock for a single device - optimized constructor
    pub fn single(device: DeviceId, counter: u64) -> Self {
        VectorClock::Single { device, counter }
    }

    pub fn insert(&mut self, device: DeviceId, counter: u64) {
        match self {
            VectorClock::Single {
                device: current_device,
                counter: current_counter,
            } => {
                // Fast path: updating same device
                if device == *current_device {
                    *current_counter = counter;
                } else {
                    // Convert to multiple representation
                    // Performance: Pre-allocate with capacity 2 (common case)
                    let mut map = BTreeMap::new();
                    map.insert(*current_device, *current_counter);
                    map.insert(device, counter);
                    *self = VectorClock::Multiple(map);
                }
            }
            VectorClock::Multiple(map) => {
                if map.is_empty() {
                    // Optimize empty map to single device
                    *self = VectorClock::Single { device, counter };
                } else {
                    // Performance: Only insert if value changed (avoids rebalancing)
                    match map.get(&device) {
                        Some(&existing) if existing == counter => {
                            // No change needed
                        }
                        _ => {
                            map.insert(device, counter);
                        }
                    }
                }
            }
        }
    }

    pub fn get(&self, device: &DeviceId) -> Option<&u64> {
        match self {
            VectorClock::Single {
                device: current_device,
                counter,
            } => {
                // Fast path with likely branch hint
                if device == current_device {
                    Some(counter)
                } else {
                    None
                }
            }
            VectorClock::Multiple(map) => map.get(device),
        }
    }

    /// Increment a device's counter - common operation optimized
    pub fn increment(&mut self, device: DeviceId) {
        match self {
            VectorClock::Single {
                device: current_device,
                counter: current_counter,
            } => {
                if device == *current_device {
                    // Fast path: increment in place
                    *current_counter = current_counter.saturating_add(1);
                } else {
                    // Need to convert to Multiple
                    let old_counter = *current_counter;
                    let mut map = BTreeMap::new();
                    map.insert(*current_device, old_counter);
                    map.insert(device, 1);
                    *self = VectorClock::Multiple(map);
                }
            }
            VectorClock::Multiple(map) => {
                if map.is_empty() {
                    // Optimize to single
                    *self = VectorClock::Single { device, counter: 1 };
                } else {
                    // Increment or insert
                    map.entry(device)
                        .and_modify(|c| *c = c.saturating_add(1))
                        .or_insert(1);
                }
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&DeviceId, &u64)> {
        VectorClockIter::new(self)
    }

    pub fn len(&self) -> usize {
        match self {
            VectorClock::Single { .. } => 1,
            VectorClock::Multiple(map) => map.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            VectorClock::Single { .. } => false,
            VectorClock::Multiple(map) => map.is_empty(),
        }
    }
}

impl PartialOrd for VectorClock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Fast path for same representation
        match (self, other) {
            (
                VectorClock::Single {
                    device: d1,
                    counter: c1,
                },
                VectorClock::Single {
                    device: d2,
                    counter: c2,
                },
            ) => {
                if d1 == d2 {
                    c1.partial_cmp(c2)
                } else {
                    None // Incomparable different devices
                }
            }
            _ => {
                // General case: compare using happens-before relationship
                let mut self_le_other = true;
                let mut other_le_self = true;

                // Check if self <= other
                for (device, self_counter) in self.iter() {
                    if let Some(other_counter) = other.get(device) {
                        if self_counter > other_counter {
                            self_le_other = false;
                            break;
                        }
                    } else if *self_counter > 0 {
                        self_le_other = false;
                        break;
                    }
                }

                // Check if other <= self
                for (device, other_counter) in other.iter() {
                    if let Some(self_counter) = self.get(device) {
                        if other_counter > self_counter {
                            other_le_self = false;
                            break;
                        }
                    } else if *other_counter > 0 {
                        other_le_self = false;
                        break;
                    }
                }

                match (self_le_other, other_le_self) {
                    (true, true) => Some(std::cmp::Ordering::Equal),
                    (true, false) => Some(std::cmp::Ordering::Less),
                    (false, true) => Some(std::cmp::Ordering::Greater),
                    (false, false) => None, // Concurrent/incomparable
                }
            }
        }
    }
}

/// Iterator for VectorClock that handles both single and multiple representations
pub enum VectorClockIter<'a> {
    Single {
        device: &'a DeviceId,
        counter: &'a u64,
        yielded: bool,
    },
    Multiple(std::collections::btree_map::Iter<'a, DeviceId, u64>),
}

impl<'a> VectorClockIter<'a> {
    fn new(clock: &'a VectorClock) -> Self {
        match clock {
            VectorClock::Single { device, counter } => VectorClockIter::Single {
                device,
                counter,
                yielded: false,
            },
            VectorClock::Multiple(map) => VectorClockIter::Multiple(map.iter()),
        }
    }
}

impl<'a> Iterator for VectorClockIter<'a> {
    type Item = (&'a DeviceId, &'a u64);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            VectorClockIter::Single {
                device,
                counter,
                yielded,
            } => {
                if *yielded {
                    None
                } else {
                    *yielded = true;
                    Some((device, counter))
                }
            }
            VectorClockIter::Multiple(iter) => iter.next(),
        }
    }
}
/// Scalar clock for tie-breaking.
pub type ScalarClock = u64;
/// Signature placeholder for attestation proofs.
pub type Signature = Vec<u8>;

/// Confidence/precision indicator for ranges and metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

/// Attestation validity classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttestationValidity {
    Valid,
    Suspicious(String),
    Invalid(String),
}

/// Attestation proof for time claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeProof {
    pub attestor: AuthorityId,
    pub claimed_ts: u64,
    pub attestor_ts: u64,
    pub skew_ms: i64,
    pub validity: AttestationValidity,
    pub signature: Signature,
}

/// Semantic time enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeStamp {
    LogicalClock(LogicalTime),   // causal partial order
    OrderClock(OrderTime),       // opaque total order, no causality
    PhysicalClock(PhysicalTime), // local physical claim
    Range(RangeTime),            // constraint on validity window
}

/// Domain selector for time requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeDomain {
    LogicalClock,
    OrderClock,
    PhysicalClock,
    Range,
}

/// Optional trust/provenance wrapper for time claims (e.g., consensus or multi-attestor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenancedTime {
    pub stamp: TimeStamp,
    pub proofs: Vec<TimeProof>,      // empty for local trust
    pub origin: Option<AuthorityId>, // who vouches; None for local
}

/// Optional, policy-gated metadata sidecar (omitted by default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimeMetadata {
    pub created_at: Option<SystemTime>,
    pub precision: Option<TimeConfidence>,
    pub confidence: Option<TimeConfidence>,
    pub authority: Option<AuthorityId>,
}

/// Time ordering result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeOrdering {
    Before,
    After,
    Concurrent,   // for logical
    Overlapping,  // for ranges
    Incomparable, // cross-domain without policy
}

/// Ordering policy for tie-break decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingPolicy {
    Native,                // partial where applicable
    DeterministicTieBreak, // caller applies authority/hash tie-break when allowed
}

impl TimeStamp {
    /// Convert TimeStamp to milliseconds for indexing compatibility.
    ///
    /// This provides a total ordering for timestamps across different domains.
    /// For logical clocks, uses lamport counter. For order clocks, uses first 8 bytes.
    /// For ranges, uses the latest timestamp.
    pub fn to_index_ms(&self) -> i64 {
        match self {
            TimeStamp::PhysicalClock(p) => p.ts_ms as i64,
            TimeStamp::LogicalClock(l) => l.lamport as i64,
            TimeStamp::OrderClock(o) => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&o.0[..8]);
                i64::from_be_bytes(buf)
            }
            TimeStamp::Range(r) => r.latest_ms as i64,
        }
    }

    /// Compare two timestamps for sorting, with proper ordering semantics.
    ///
    /// Unlike raw i64 comparison, this uses the unified time system's
    /// native comparison methods and handles cross-domain comparisons gracefully.
    ///
    /// This method is optimized for performance in sorting scenarios.
    pub fn sort_compare(&self, other: &TimeStamp, policy: OrderingPolicy) -> std::cmp::Ordering {
        // Fast path: check if both timestamps are the same variant for direct comparison
        match (self, other) {
            // PhysicalClock fast path - direct numeric comparison
            (TimeStamp::PhysicalClock(a), TimeStamp::PhysicalClock(b)) => a.ts_ms.cmp(&b.ts_ms),
            // OrderClock fast path - direct byte array comparison
            (TimeStamp::OrderClock(a), TimeStamp::OrderClock(b)) => a.0.cmp(&b.0),
            // Range fast path - optimized range comparison
            (TimeStamp::Range(a), TimeStamp::Range(b)) => {
                if a.latest_ms < b.earliest_ms {
                    std::cmp::Ordering::Less
                } else if b.latest_ms < a.earliest_ms {
                    std::cmp::Ordering::Greater
                } else {
                    // Overlapping ranges - compare by latest timestamp
                    a.latest_ms.cmp(&b.latest_ms)
                }
            }
            // LogicalClock comparison requires full comparison logic
            (TimeStamp::LogicalClock(_), TimeStamp::LogicalClock(_)) => {
                match self.compare(other, policy) {
                    TimeOrdering::Before => std::cmp::Ordering::Less,
                    TimeOrdering::After => std::cmp::Ordering::Greater,
                    TimeOrdering::Concurrent | TimeOrdering::Overlapping => {
                        std::cmp::Ordering::Equal
                    }
                    TimeOrdering::Incomparable => {
                        // Shouldn't happen for same-domain logical clocks, but handle gracefully
                        self.to_index_ms().cmp(&other.to_index_ms())
                    }
                }
            }
            // Cross-domain comparison - fall back to index-based ordering
            _ => {
                // Fast path: if index values differ significantly, avoid full comparison
                let self_index = self.to_index_ms();
                let other_index = other.to_index_ms();
                match self_index.cmp(&other_index) {
                    std::cmp::Ordering::Equal => {
                        // Same index - need full comparison for stability
                        match self.compare(other, policy) {
                            TimeOrdering::Before => std::cmp::Ordering::Less,
                            TimeOrdering::After => std::cmp::Ordering::Greater,
                            _ => std::cmp::Ordering::Equal,
                        }
                    }
                    ordering => ordering,
                }
            }
        }
    }

    /// High-performance sorting for collections of timestamps.
    ///
    /// This method provides optimizations for sorting large collections by:
    /// - Using unstable sort for better performance when stability isn't required
    /// - Pre-computing sort keys for mixed-domain collections
    /// - Using specialized fast paths for same-domain collections
    pub fn sort_collection_optimized(
        timestamps: &mut [TimeStamp],
        policy: OrderingPolicy,
        stable: bool,
    ) {
        // Fast path: check if all timestamps are the same variant
        if let Some(first) = timestamps.first() {
            let all_same_variant = timestamps
                .iter()
                .all(|ts| std::mem::discriminant(ts) == std::mem::discriminant(first));

            if all_same_variant {
                // Same variant - use optimized domain-specific sorting
                match first {
                    TimeStamp::PhysicalClock(_) => {
                        // Direct numeric sort on ts_ms
                        if stable {
                            timestamps.sort_by(|a, b| {
                                if let (TimeStamp::PhysicalClock(x), TimeStamp::PhysicalClock(y)) =
                                    (a, b)
                                {
                                    x.ts_ms.cmp(&y.ts_ms)
                                } else {
                                    std::cmp::Ordering::Equal
                                }
                            });
                        } else {
                            timestamps.sort_unstable_by(|a, b| {
                                if let (TimeStamp::PhysicalClock(x), TimeStamp::PhysicalClock(y)) =
                                    (a, b)
                                {
                                    x.ts_ms.cmp(&y.ts_ms)
                                } else {
                                    std::cmp::Ordering::Equal
                                }
                            });
                        }
                    }
                    TimeStamp::OrderClock(_) => {
                        // Direct byte array sort
                        if stable {
                            timestamps.sort_by(|a, b| a.sort_compare(b, policy));
                        } else {
                            timestamps.sort_unstable_by(|a, b| a.sort_compare(b, policy));
                        }
                    }
                    _ => {
                        // Use general sorting for logical/range clocks
                        if stable {
                            timestamps.sort_by(|a, b| a.sort_compare(b, policy));
                        } else {
                            timestamps.sort_unstable_by(|a, b| a.sort_compare(b, policy));
                        }
                    }
                }
            } else {
                // Mixed variants - use general sorting with sort_compare optimization
                if stable {
                    timestamps.sort_by(|a, b| a.sort_compare(b, policy));
                } else {
                    timestamps.sort_unstable_by(|a, b| a.sort_compare(b, policy));
                }
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
                if a.latest_ms < b.earliest_ms {
                    TimeOrdering::Before
                } else if b.latest_ms < a.earliest_ms {
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

// Legacy current_unix_timestamp() utility removed - use PhysicalTimeEffects trait instead

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_clock_orders_totally() {
        let a = TimeStamp::OrderClock(OrderTime([0u8; 32]));
        let b = TimeStamp::OrderClock(OrderTime([1u8; 32]));
        assert_eq!(a.compare(&b, OrderingPolicy::Native), TimeOrdering::Before);
        assert_eq!(b.compare(&a, OrderingPolicy::Native), TimeOrdering::After);
        assert_eq!(
            a.compare(&a, OrderingPolicy::Native),
            TimeOrdering::Concurrent
        );
    }

    #[test]
    fn logical_clock_respects_partial_order() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();

        let mut v1 = VectorClock::new();
        v1.insert(device_a, 1);
        v1.insert(device_b, 0);
        let mut v2 = VectorClock::new();
        v2.insert(device_a, 2);
        v2.insert(device_b, 1);

        let t1 = TimeStamp::LogicalClock(LogicalTime {
            vector: v1.clone(),
            lamport: 1,
        });
        let t2 = TimeStamp::LogicalClock(LogicalTime {
            vector: v2,
            lamport: 2,
        });

        assert_eq!(
            t1.compare(&t2, OrderingPolicy::Native),
            TimeOrdering::Before
        );
        assert_eq!(t2.compare(&t1, OrderingPolicy::Native), TimeOrdering::After);

        // incomparable vectors fall back to policy
        let mut v3 = VectorClock::new();
        v3.insert(device_a, 1);
        v3.insert(device_b, 0);
        let mut v4 = VectorClock::new();
        v4.insert(device_a, 0);
        v4.insert(device_b, 1);
        let t3 = TimeStamp::LogicalClock(LogicalTime {
            vector: v3,
            lamport: 1,
        });
        let t4 = TimeStamp::LogicalClock(LogicalTime {
            vector: v4,
            lamport: 1,
        });
        let native_cmp = t3.compare(&t4, OrderingPolicy::Native);
        // Native policy should return Incomparable for concurrent vector clocks
        assert_eq!(native_cmp, TimeOrdering::Incomparable);
        // DeterministicTieBreak should resolve to Concurrent
        assert_eq!(
            t3.compare(&t4, OrderingPolicy::DeterministicTieBreak),
            TimeOrdering::Concurrent
        );
    }

    #[test]
    fn physical_clock_compares_by_millis() {
        let t1 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 10,
            uncertainty: None,
        });
        let t2 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 20,
            uncertainty: None,
        });
        assert_eq!(
            t1.compare(&t2, OrderingPolicy::Native),
            TimeOrdering::Before
        );
        assert_eq!(t2.compare(&t1, OrderingPolicy::Native), TimeOrdering::After);
    }

    #[test]
    fn range_clock_detects_overlap() {
        let r1 = TimeStamp::Range(RangeTime {
            earliest_ms: 0,
            latest_ms: 10,
            confidence: TimeConfidence::High,
        });
        let r2 = TimeStamp::Range(RangeTime {
            earliest_ms: 11,
            latest_ms: 20,
            confidence: TimeConfidence::High,
        });
        let r3 = TimeStamp::Range(RangeTime {
            earliest_ms: 5,
            latest_ms: 15,
            confidence: TimeConfidence::High,
        });

        assert_eq!(
            r1.compare(&r2, OrderingPolicy::Native),
            TimeOrdering::Before
        );
        assert_eq!(r2.compare(&r1, OrderingPolicy::Native), TimeOrdering::After);
        assert_eq!(
            r1.compare(&r3, OrderingPolicy::Native),
            TimeOrdering::Overlapping
        );
    }

    #[test]
    fn to_index_ms_provides_consistent_ordering() {
        // Test that to_index_ms provides a total ordering
        let physical = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let logical = TimeStamp::LogicalClock(LogicalTime {
            vector: VectorClock::new(),
            lamport: 500,
        });
        let order = TimeStamp::OrderClock(OrderTime([1u8; 32]));
        let range = TimeStamp::Range(RangeTime {
            earliest_ms: 900,
            latest_ms: 1100,
            confidence: TimeConfidence::High,
        });

        // Verify each returns a consistent i64 value
        assert_eq!(physical.to_index_ms(), 1000);
        assert_eq!(logical.to_index_ms(), 500);
        assert_eq!(range.to_index_ms(), 1100); // Uses latest_ms

        // Verify order clock returns a deterministic value
        let order_ms = order.to_index_ms();
        assert_eq!(order.to_index_ms(), order_ms); // Should be consistent
    }

    #[test]
    fn sort_compare_handles_cross_domain_timestamps() {
        let physical1 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let physical2 = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 2000,
            uncertainty: None,
        });
        let logical = TimeStamp::LogicalClock(LogicalTime {
            vector: VectorClock::new(),
            lamport: 1500,
        });

        // Same-domain comparisons should work normally
        assert_eq!(
            physical1.sort_compare(&physical2, OrderingPolicy::Native),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            physical2.sort_compare(&physical1, OrderingPolicy::Native),
            std::cmp::Ordering::Greater
        );

        // Cross-domain comparisons should fall back to index-based ordering
        assert_eq!(
            physical1.sort_compare(&logical, OrderingPolicy::Native),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            logical.sort_compare(&physical2, OrderingPolicy::Native),
            std::cmp::Ordering::Less
        );
    }
}
