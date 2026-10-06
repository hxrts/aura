//! Generic order-independent reduction rules over `CausalMetadata`.
//!
//! docs/105_journal.md requires reduction to be a deterministic function of
//! the fact set. Facts that can be reversed or overwritten (bans, overrides,
//! grants, ...) arrive late and out of order through sync, so their reducers
//! must not depend on arrival order. This module provides the shared rules:
//!
//! - [`observed_remove_live`]: tagged observed-remove set. A reversal removes
//!   exactly the add tags its writer observed (`CausalMetadata::revokes`).
//! - [`register_survivors`]: multi-value register. A write replaces the writes
//!   its writer observed (`CausalMetadata::supersedes`) and any write causally
//!   before it; survivors are truly concurrent and callers resolve them by an
//!   explicit policy, then [`causal_cmp`].
//! - [`causal_cmp`]: total order consistent with causality, for history
//!   display and deterministic tie-breaks.
//! - [`for_each_permutation`] / [`assert_permutation_invariant`]: test harness
//!   proving a reducer gives identical state for every arrival order.
//!
//! Physical time never orders these facts.

use aura_core::time::{
    CausalClock, CausalMetadata, CausalTag, OrderingPolicy, TimeStamp, VectorClock,
};
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// A fact carrying causal metadata, with its content-derived tag.
pub trait CausalFact {
    /// The fact's tag, derived from its canonical content.
    fn causal_tag(&self) -> CausalTag;
    /// The fact's causal metadata.
    fn causal_metadata(&self) -> &CausalMetadata;
}

/// Total order consistent with happens-before: causal comparison
/// (`TimeStamp::sort_compare` with `OrderingPolicy::DeterministicTieBreak`),
/// with concurrent facts broken by causal depth, Lamport scalar, then tag.
#[must_use]
pub fn causal_cmp<F: CausalFact>(a: &F, b: &F) -> Ordering {
    let (left, right) = (&a.causal_metadata().clock, &b.causal_metadata().clock);
    let causal = if left.happens_before(right) {
        Ordering::Less
    } else if right.happens_before(left) {
        Ordering::Greater
    } else {
        TimeStamp::LogicalClock(left.to_logical()).sort_compare(
            &TimeStamp::LogicalClock(right.to_logical()),
            OrderingPolicy::DeterministicTieBreak,
        )
    };
    causal
        .then_with(|| left.depth().cmp(&right.depth()))
        .then_with(|| left.lamport.cmp(&right.lamport))
        .then_with(|| a.causal_tag().cmp(&b.causal_tag()))
}

/// Live add tags of a tagged observed-remove set: every add `(key, tag)` not
/// revoked by a reversal `(key, revokes)` of the same key. Independent of
/// input order.
#[must_use]
pub fn observed_remove_live<K: Eq>(
    adds: &[(K, CausalTag)],
    reversals: &[(K, &[CausalTag])],
) -> BTreeSet<CausalTag> {
    adds.iter()
        .filter(|(key, tag)| {
            !reversals
                .iter()
                .any(|(reversed, revokes)| reversed == key && revokes.contains(tag))
        })
        .map(|(_, tag)| *tag)
        .collect()
}

/// Surviving writes of a multi-value register: writes neither superseded by
/// an observing write nor causally before another write. Independent of
/// input order.
#[must_use]
pub fn register_survivors<F: CausalFact>(writes: &[&F]) -> BTreeSet<CausalTag> {
    writes
        .iter()
        .filter(|write| {
            let (tag, meta) = (write.causal_tag(), write.causal_metadata());
            !writes.iter().any(|other| {
                let other_meta = other.causal_metadata();
                other.causal_tag() != tag
                    && (other_meta.supersedes.contains(&tag)
                        || meta.clock.happens_before(&other_meta.clock))
            })
        })
        .map(|write| write.causal_tag())
        .collect()
}

/// Element-wise maximum of the given clocks: what a writer has observed.
#[must_use]
pub fn merged_vector<'a>(clocks: impl IntoIterator<Item = &'a CausalClock>) -> VectorClock {
    let mut merged = VectorClock::new();
    for clock in clocks {
        for (device, counter) in &clock.vector {
            if merged.get(device).map_or(true, |current| counter > current) {
                merged.insert(*device, *counter);
            }
        }
    }
    merged
}

/// Call `visit` with every permutation of `items` (Heap's algorithm). Test
/// harness for order-independent reducers; `items` should stay small.
pub fn for_each_permutation<T: Clone>(items: &[T], mut visit: impl FnMut(&[T])) {
    let mut items = items.to_vec();
    let n = items.len();
    let mut counters = vec![0usize; n];
    visit(&items);
    let mut i = 0;
    while i < n {
        if counters[i] < i {
            if i % 2 == 0 {
                items.swap(0, i);
            } else {
                items.swap(counters[i], i);
            }
            visit(&items);
            counters[i] += 1;
            i = 0;
        } else {
            counters[i] = 0;
            i += 1;
        }
    }
}

/// Assert `reduce` yields identical state for every arrival order of `items`.
/// Returns that state.
///
/// # Panics
/// Panics with the differing order when two permutations disagree.
pub fn assert_permutation_invariant<T, S>(items: &[T], reduce: impl Fn(&[T]) -> S) -> S
where
    T: Clone + std::fmt::Debug,
    S: PartialEq + std::fmt::Debug,
{
    let expected = reduce(items);
    for_each_permutation(items, |order| {
        let actual = reduce(order);
        assert_eq!(
            actual, expected,
            "reduction depends on arrival order: {order:?} vs {items:?}"
        );
    });
    expected
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::identifiers::DeviceId;

    fn tag(byte: u8) -> CausalTag {
        CausalTag([byte; 32])
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Write {
        tag: CausalTag,
        meta: CausalMetadata,
    }

    impl CausalFact for Write {
        fn causal_tag(&self) -> CausalTag {
            self.tag
        }
        fn causal_metadata(&self) -> &CausalMetadata {
            &self.meta
        }
    }

    fn meta(tag_byte: u8, device: u8, counter: u64, supersedes: &[CausalTag]) -> Write {
        Write {
            tag: tag(tag_byte),
            meta: CausalMetadata {
                revokes: Vec::new(),
                supersedes: supersedes.to_vec(),
                clock: CausalClock {
                    lamport: counter,
                    vector: vec![(DeviceId(uuid::Uuid::from_bytes([device; 16])), counter)],
                },
            },
        }
    }

    #[test]
    fn permutation_harness_visits_every_order() {
        let mut seen = BTreeSet::new();
        for_each_permutation(&[1, 2, 3, 4], |order| {
            seen.insert(order.to_vec());
        });
        assert_eq!(seen.len(), 24);
    }

    #[test]
    fn observed_remove_keeps_unobserved_concurrent_add() {
        let revokes = [tag(1)];
        let adds = [('x', tag(1)), ('x', tag(2)), ('y', tag(3))];
        let reversals = [('x', &revokes[..]), ('y', &revokes[..])];
        let live = observed_remove_live(&adds, &reversals);
        assert_eq!(live, BTreeSet::from([tag(2), tag(3)]));
    }

    #[test]
    fn register_survivors_are_order_independent() {
        let writes = vec![
            meta(1, 1, 1, &[]),
            meta(2, 1, 2, &[tag(1)]),
            meta(3, 2, 1, &[]),
        ];
        let survivors = assert_permutation_invariant(&writes, |order| {
            register_survivors(&order.iter().collect::<Vec<_>>())
        });
        assert_eq!(survivors, BTreeSet::from([tag(2), tag(3)]));
    }

    #[test]
    fn causal_cmp_follows_happens_before_then_breaks_ties() {
        let early = meta(9, 1, 1, &[]);
        let late = meta(1, 1, 3, &[]);
        let concurrent = meta(5, 2, 1, &[]);
        let mut all = [late, concurrent, early];
        all.sort_by(causal_cmp);
        let tags: Vec<_> = all.iter().map(|m| m.tag).collect();
        assert_eq!(tags, vec![tag(5), tag(9), tag(1)]);
    }
}
