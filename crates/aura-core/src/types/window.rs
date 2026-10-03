//! Pure, domain-separated half-open interval arithmetic.
//!
//! An interval proves arithmetic validity only. Admission, checkpoint provenance,
//! progression and persistence remain with the domain's authoritative owner.
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
}
/// Sealed coordinate domains; physical time and receipt generations cannot mix.
pub trait WindowDomain: sealed::Sealed {}
/// Local physical milliseconds; does not authorize a clock observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalMillis;
/// Receipt nonce generation; carries no physical time information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiptGeneration;
impl sealed::Sealed for PhysicalMillis {}
impl sealed::Sealed for ReceiptGeneration {}
impl WindowDomain for PhysicalMillis {}
impl WindowDomain for ReceiptGeneration {}

/// A coordinate in one explicit window domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPosition<D: WindowDomain> {
    value: u64,
    domain: PhantomData<D>,
}
impl<D: WindowDomain> WindowPosition<D> {
    pub const fn new(value: u64) -> Self {
        Self {
            value,
            domain: PhantomData,
        }
    }
    pub const fn value(&self) -> u64 {
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WindowIntervalError {
    #[error("window endpoint is not representable in u64")]
    EndpointOverflow,
    #[error("window end precedes its start")]
    ReversedBounds,
}

/// Validated `[start, end)` coordinates, with a representable exclusive endpoint.
/// Empty intervals are valid arithmetic; physical timeout policy rejects them.
///
/// ```compile_fail
/// use aura_core::types::window::{WindowInterval, WindowPosition, PhysicalMillis, ReceiptGeneration};
/// let window = WindowInterval::<PhysicalMillis>::new(WindowPosition::new(10), 5).expect("valid bounds");
/// window.contains(WindowPosition::<ReceiptGeneration>::new(11));
/// ```
///
/// ```compile_fail
/// use aura_core::types::window::{WindowInterval, WindowPosition, PhysicalMillis, ReceiptGeneration};
/// let window = WindowInterval::<ReceiptGeneration>::new(WindowPosition::new(10), 5).expect("valid bounds");
/// window.contains(WindowPosition::<PhysicalMillis>::new(11));
/// ```
/// Generation allowances extending beyond `u64::MAX` are rejected, never
/// truncated or wrapped. Representing a window including the maximal generation
/// would require a wider coordinate contract; callers must not silently clamp it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowInterval<D: WindowDomain> {
    start: u64,
    end: u64,
    domain: PhantomData<D>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntervalSnapshot {
    start: u64,
    end: u64,
}
impl<D: WindowDomain> WindowInterval<D> {
    pub fn new(start: WindowPosition<D>, extent: u64) -> Result<Self, WindowIntervalError> {
        let end = start
            .value
            .checked_add(extent)
            .ok_or(WindowIntervalError::EndpointOverflow)?;
        Self::from_bounds(start, WindowPosition::new(end))
    }
    pub fn from_bounds(
        start: WindowPosition<D>,
        end: WindowPosition<D>,
    ) -> Result<Self, WindowIntervalError> {
        if end.value < start.value {
            return Err(WindowIntervalError::ReversedBounds);
        }
        Ok(Self {
            start: start.value,
            end: end.value,
            domain: PhantomData,
        })
    }
    pub fn start(&self) -> WindowPosition<D> {
        WindowPosition::new(self.start)
    }
    pub fn end(&self) -> WindowPosition<D> {
        WindowPosition::new(self.end)
    }
    pub fn extent(&self) -> u64 {
        self.end - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
    pub fn contains(&self, position: WindowPosition<D>) -> bool {
        position.value >= self.start && position.value < self.end
    }
}
impl<D: WindowDomain> Serialize for WindowInterval<D> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        IntervalSnapshot {
            start: self.start,
            end: self.end,
        }
        .serialize(serializer)
    }
}
impl<'de, D: WindowDomain> Deserialize<'de> for WindowInterval<D> {
    fn deserialize<T: serde::Deserializer<'de>>(deserializer: T) -> Result<Self, T::Error> {
        let snapshot = IntervalSnapshot::deserialize(deserializer)?;
        Self::from_bounds(
            WindowPosition::new(snapshot.start),
            WindowPosition::new(snapshot.end),
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    fn interval<D: WindowDomain>(start: u64, extent: u64) -> WindowInterval<D> {
        WindowInterval::new(WindowPosition::new(start), extent).expect("valid fixture bounds")
    }
    fn boundary_law<D: WindowDomain>() {
        let window = interval::<D>(10, 5);
        assert!(!window.contains(WindowPosition::new(9)));
        assert!(window.contains(WindowPosition::new(10)));
        assert!(window.contains(WindowPosition::new(14)));
        assert!(!window.contains(WindowPosition::new(15)));
        let empty = interval::<D>(10, 0);
        assert!(empty.is_empty());
        assert!(!empty.contains(WindowPosition::new(10)));
    }
    #[test]
    fn both_domains_obey_half_open_and_empty_laws() {
        boundary_law::<PhysicalMillis>();
        boundary_law::<ReceiptGeneration>();
    }
    #[test]
    fn generation_endpoint_overflow_is_rejected_without_wrap_or_truncation() {
        let last = interval::<ReceiptGeneration>(u64::MAX - 1, 1);
        assert!(last.contains(WindowPosition::new(u64::MAX - 1)));
        assert!(!last.contains(WindowPosition::new(u64::MAX)));
        assert_eq!(
            WindowInterval::<ReceiptGeneration>::new(WindowPosition::new(u64::MAX), 1),
            Err(WindowIntervalError::EndpointOverflow)
        );
        assert_eq!(
            WindowInterval::<PhysicalMillis>::new(WindowPosition::new(u64::MAX), 1),
            Err(WindowIntervalError::EndpointOverflow)
        );
    }
    #[test]
    fn serde_restore_validates_bounds_and_preserves_exact_interval() {
        let original = interval::<PhysicalMillis>(100, 20);
        let bytes = serde_json::to_vec(&original).expect("serialize pure interval");
        let restored: WindowInterval<PhysicalMillis> =
            serde_json::from_slice(&bytes).expect("restore valid bounds");
        assert_eq!(restored, original);
        assert!(
            serde_json::from_str::<WindowInterval<PhysicalMillis>>(r#"{"start":20,"end":10}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<WindowInterval<ReceiptGeneration>>(r#"{"start":20}"#).is_err()
        );
        assert!(serde_json::from_str::<WindowInterval<ReceiptGeneration>>(
            r#"{"start":20,"end":21,"epoch":9}"#
        )
        .is_err());
    }
    #[test]
    fn membership_matches_checked_distance_for_both_domains() {
        for start in [0, 1, u64::MAX - 16] {
            for extent in 0..=16 {
                let physical = interval::<PhysicalMillis>(start, extent);
                let generation = interval::<ReceiptGeneration>(start, extent);
                for candidate in [0, start, start.saturating_add(1), u64::MAX] {
                    let expected = candidate >= start && candidate - start < extent;
                    assert_eq!(physical.contains(WindowPosition::new(candidate)), expected);
                    assert_eq!(
                        generation.contains(WindowPosition::new(candidate)),
                        expected
                    );
                }
            }
        }
    }
}
