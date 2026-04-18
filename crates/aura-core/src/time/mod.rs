//! Unified time semantics for Aura
//!
//! Provides semantic time representations (logical, order-only, physical, range)
//! and explicit ordering policies. Provenance/attestation is modeled via an
//! orthogonal wrapper (`ProvenancedTime`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

mod ordering;
pub mod pure;
pub mod timeout;
mod vector_clock;

use crate::{
    crypto::Ed25519Signature,
    types::identifiers::{AuthorityId, DeviceId},
};

/// Physical clock representation with optional uncertainty.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PhysicalTime {
    pub ts_ms: u64,
    pub uncertainty: Option<u64>,
}

impl PhysicalTime {
    /// Create an exact physical timestamp with no uncertainty sidecar.
    pub const fn exact(ts_ms: u64) -> Self {
        Self {
            ts_ms,
            uncertainty: None,
        }
    }
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
    earliest_ms: u64,
    latest_ms: u64,
    confidence: TimeConfidence,
}

impl RangeTime {
    /// Create a validated range (earliest <= latest).
    pub fn new(
        earliest_ms: u64,
        latest_ms: u64,
        confidence: TimeConfidence,
    ) -> Result<Self, crate::AuraError> {
        if earliest_ms > latest_ms {
            return Err(crate::AuraError::invalid(format!(
                "RangeTime invalid: earliest_ms ({earliest_ms}) > latest_ms ({latest_ms})"
            )));
        }
        Ok(Self {
            earliest_ms,
            latest_ms,
            confidence,
        })
    }

    pub fn earliest_ms(&self) -> u64 {
        self.earliest_ms
    }

    pub fn latest_ms(&self) -> u64 {
        self.latest_ms
    }

    pub fn confidence(&self) -> &TimeConfidence {
        &self.confidence
    }
}

/// Vector clock: device -> counter (causal domain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorClock {
    /// Single device optimization — common case for many authorities.
    Single { device: DeviceId, counter: u64 },
    /// Multiple devices — fallback to BTreeMap.
    Multiple(BTreeMap<DeviceId, u64>),
}

/// Scalar clock for tie-breaking.
pub type ScalarClock = u64;

/// Signature attached to attested time proofs (Ed25519 bytes).
pub type Signature = Ed25519Signature;

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
    LogicalClock(LogicalTime),
    OrderClock(OrderTime),
    PhysicalClock(PhysicalTime),
    Range(RangeTime),
}

/// Domain selector for time requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TimeDomain {
    LogicalClock,
    OrderClock,
    PhysicalClock,
    Range,
}

impl TimeDomain {
    pub fn as_str(&self) -> &'static str {
        match self {
            TimeDomain::LogicalClock => "logical",
            TimeDomain::OrderClock => "order",
            TimeDomain::PhysicalClock => "physical",
            TimeDomain::Range => "range",
        }
    }
}

impl fmt::Display for TimeDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Domain-scoped index for time ordering within a single time domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeIndex {
    domain: TimeDomain,
    value: u64,
}

impl TimeIndex {
    pub fn new(domain: TimeDomain, value: u64) -> Self {
        Self { domain, value }
    }

    pub fn domain(&self) -> TimeDomain {
        self.domain
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    /// Compare indices only when they share a domain.
    pub fn cmp_same_domain(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.domain == other.domain {
            Some(self.value.cmp(&other.value))
        } else {
            None
        }
    }

    /// Deterministic tie-break key across domains (explicit use only).
    pub fn tie_break_key(&self) -> (TimeDomain, u64) {
        (self.domain, self.value)
    }
}

impl fmt::Display for TimeIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.domain, self.value)
    }
}

/// Optional trust/provenance wrapper for time claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenancedTime {
    pub stamp: TimeStamp,
    pub proofs: Vec<TimeProof>,
    pub origin: Option<AuthorityId>,
}

/// Optional, policy-gated metadata sidecar (omitted by default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimeMetadata {
    pub created_at: Option<PhysicalTime>,
    pub precision: Option<TimeConfidence>,
    pub confidence: Option<TimeConfidence>,
    pub authority: Option<AuthorityId>,
}

/// Time ordering result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeOrdering {
    Before,
    After,
    Concurrent,
    Overlapping,
    Incomparable,
}

/// Ordering policy for tie-break decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingPolicy {
    Native,
    DeterministicTieBreak,
}

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
        let device_a = DeviceId::new_from_entropy([86u8; 32]);
        let device_b = DeviceId::new_from_entropy([87u8; 32]);

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
        assert_eq!(
            t3.compare(&t4, OrderingPolicy::Native),
            TimeOrdering::Incomparable
        );
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
        let r1 = TimeStamp::Range(RangeTime::new(0, 10, TimeConfidence::High).unwrap());
        let r2 = TimeStamp::Range(RangeTime::new(11, 20, TimeConfidence::High).unwrap());
        let r3 = TimeStamp::Range(RangeTime::new(5, 15, TimeConfidence::High).unwrap());

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
    fn range_time_rejects_inverted_bounds() {
        assert!(RangeTime::new(10, 5, TimeConfidence::High).is_err());
    }

    #[test]
    fn vector_clock_increment_detects_overflow() {
        let device = DeviceId::new_from_entropy([9u8; 32]);
        let mut vc = VectorClock::single(device, u64::MAX);
        assert!(vc.increment(device).is_err());
    }

    #[test]
    fn to_index_ms_is_domain_scoped() {
        let physical = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let logical = TimeStamp::LogicalClock(LogicalTime {
            vector: VectorClock::new(),
            lamport: 500,
        });
        let order = TimeStamp::OrderClock(OrderTime([1u8; 32]));
        let range = TimeStamp::Range(RangeTime::new(900, 1100, TimeConfidence::High).unwrap());

        assert_eq!(physical.to_index_ms().domain(), TimeDomain::PhysicalClock);
        assert_eq!(physical.to_index_ms().value(), 1000);
        assert_eq!(logical.to_index_ms().domain(), TimeDomain::LogicalClock);
        assert_eq!(logical.to_index_ms().value(), 500);
        assert_eq!(range.to_index_ms().domain(), TimeDomain::Range);
        assert_eq!(range.to_index_ms().value(), 1100);
        assert_eq!(order.to_index_ms().domain(), TimeDomain::OrderClock);
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

        assert_eq!(
            physical1.sort_compare(&physical2, OrderingPolicy::Native),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            physical1.sort_compare(&logical, OrderingPolicy::Native),
            std::cmp::Ordering::Equal
        );

        let det = physical1.sort_compare(&logical, OrderingPolicy::DeterministicTieBreak);
        assert_eq!(
            det,
            physical1
                .to_index_ms()
                .tie_break_key()
                .cmp(&logical.to_index_ms().tie_break_key())
        );
    }
}
