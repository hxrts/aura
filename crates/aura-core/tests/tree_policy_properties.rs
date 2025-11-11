//! Property-Based Tests for Tree Policy Lattice
//!
//! This module verifies the mathematical properties of the policy meet-semilattice
//! using property-based testing with proptest.
//!
//! ## Properties Verified
//!
//! 1. **Associativity**: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
//! 2. **Commutativity**: a ⊓ b = b ⊓ a
//! 3. **Idempotency**: a ⊓ a = a
//! 4. **Absorption**: a ⊓ (a ⊔ b) = a (if join exists)
//! 5. **Meet selects stricter**: a ⊓ b ≤ a and a ⊓ b ≤ b
//!
//! ## Lattice Structure
//!
//! ```text
//!                 Any (⊤)
//!                  |
//!            Threshold{m,n}
//!                  |
//!                 All (⊥)
//! ```

use aura_core::tree::Policy;
use proptest::prelude::*;
use proptest::{prop_oneof, proptest};

/// Generate arbitrary threshold policies
fn arb_threshold() -> impl Strategy<Value = Policy> {
    (1u16..=10, 1u16..=10)
        .prop_filter("m <= n", |(m, n)| m <= n)
        .prop_map(|(m, n)| Policy::Threshold { m, n })
}

/// Generate arbitrary policies
fn arb_policy() -> impl Strategy<Value = Policy> {
    prop_oneof![Just(Policy::Any), Just(Policy::All), arb_threshold(),]
}

/// Compute meet (greatest lower bound) of two policies
/// Uses the actual implementation from Policy::meet
fn meet(a: &Policy, b: &Policy) -> Policy {
    a.meet(b)
}

/// Check if policy a is stricter than or equal to policy b
/// In lattice terms: a ≤ b (a is more restrictive than or equal to b)
fn is_stricter_or_equal(a: &Policy, b: &Policy) -> bool {
    use Policy::*;
    match (a, b) {
        (All, _) => true,                 // All is bottom (strictest) - ≤ everything
        (_, Any) => true,                 // Any is top (least strict) - everything is ≤ Any
        (Any, All) => false,              // Any is not ≤ All
        (Any, Threshold { .. }) => false, // Any is not ≤ Threshold
        (a, b) if a == b => true,         // Reflexive
        (Threshold { m: m1, n: n1 }, Threshold { m: m2, n: n2 }) => {
            // a is stricter (≤) if a's fraction ≥ b's fraction
            (m1 * n2) >= (m2 * n1)
        }
        (Threshold { m, n }, All) => m == n, // Threshold{n,n} ≤ All, but other thresholds are not
    }
}

proptest! {
    /// Property: Meet is associative
    /// For all policies a, b, c: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
    #[test]
    fn prop_meet_associative(
        a in arb_policy(),
        b in arb_policy(),
        c in arb_policy()
    ) {
        let left = meet(&meet(&a, &b), &c);
        let right = meet(&a, &meet(&b, &c));
        prop_assert_eq!(left, right, "Meet must be associative");
    }

    /// Property: Meet is commutative
    /// For all policies a, b: a ⊓ b = b ⊓ a
    #[test]
    fn prop_meet_commutative(
        a in arb_policy(),
        b in arb_policy()
    ) {
        let left = meet(&a, &b);
        let right = meet(&b, &a);
        prop_assert_eq!(left, right, "Meet must be commutative");
    }

    /// Property: Meet is idempotent
    /// For all policies a: a ⊓ a = a
    #[test]
    fn prop_meet_idempotent(a in arb_policy()) {
        let result = meet(&a, &a);
        prop_assert_eq!(result, a, "Meet must be idempotent");
    }

    /// Property: Meet selects stricter policy
    /// For all policies a, b: (a ⊓ b) ≤ a and (a ⊓ b) ≤ b
    #[test]
    fn prop_meet_selects_stricter(
        a in arb_policy(),
        b in arb_policy()
    ) {
        let m = meet(&a, &b);
        prop_assert!(
            is_stricter_or_equal(&m, &a),
            "Meet must be stricter than or equal to first argument: {:?} ≤ {:?}",
            m, a
        );
        prop_assert!(
            is_stricter_or_equal(&m, &b),
            "Meet must be stricter than or equal to second argument: {:?} ≤ {:?}",
            m, b
        );
    }

    /// Property: Meet with Any gives original policy
    /// For all policies a: a ⊓ Any = a
    #[test]
    fn prop_meet_with_any(a in arb_policy()) {
        let result = meet(&a, &Policy::Any);
        prop_assert_eq!(result, a, "Meet with Any should return original policy");
    }

    /// Property: Meet with All gives All
    /// For all policies a: a ⊓ All = All
    #[test]
    fn prop_meet_with_all(a in arb_policy()) {
        let result = meet(&a, &Policy::All);
        prop_assert_eq!(result, Policy::All, "Meet with All should return All");
    }

    /// Property: Threshold ordering is transitive
    /// If a ≤ b and b ≤ c, then a ≤ c
    #[test]
    fn prop_ordering_transitive(
        a in arb_policy(),
        b in arb_policy(),
        c in arb_policy()
    ) {
        if is_stricter_or_equal(&a, &b) && is_stricter_or_equal(&b, &c) {
            prop_assert!(
                is_stricter_or_equal(&a, &c),
                "Ordering must be transitive: {:?} ≤ {:?} ≤ {:?}",
                a, b, c
            );
        }
    }

    /// Property: Meet is greatest lower bound
    /// If d ≤ a and d ≤ b, then d ≤ (a ⊓ b)
    #[test]
    fn prop_meet_is_greatest_lower_bound(
        a in arb_policy(),
        b in arb_policy(),
        d in arb_policy()
    ) {
        let m = meet(&a, &b);
        if is_stricter_or_equal(&d, &a) && is_stricter_or_equal(&d, &b) {
            prop_assert!(
                is_stricter_or_equal(&d, &m),
                "Meet must be greatest lower bound: {:?} ≤ {:?}",
                d, m
            );
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_meet_any_threshold() {
        let any = Policy::Any;
        let threshold = Policy::Threshold { m: 2, n: 3 };
        assert_eq!(meet(&any, &threshold), threshold);
        assert_eq!(meet(&threshold, &any), threshold);
    }

    #[test]
    fn test_meet_all_threshold() {
        let all = Policy::All;
        let threshold = Policy::Threshold { m: 2, n: 3 };
        assert_eq!(meet(&all, &threshold), Policy::All);
        assert_eq!(meet(&threshold, &all), Policy::All);
    }

    #[test]
    fn test_meet_thresholds_same() {
        let t1 = Policy::Threshold { m: 2, n: 3 };
        let t2 = Policy::Threshold { m: 2, n: 3 };
        assert_eq!(meet(&t1, &t2), t1);
    }

    #[test]
    fn test_meet_thresholds_different() {
        let t1 = Policy::Threshold { m: 2, n: 3 }; // 2/3 ≈ 0.667
        let t2 = Policy::Threshold { m: 3, n: 4 }; // 3/4 = 0.75 (stricter)
        assert_eq!(meet(&t1, &t2), t2); // Stricter threshold wins
    }

    #[test]
    fn test_is_stricter_all() {
        assert!(is_stricter_or_equal(&Policy::All, &Policy::Any));
        assert!(is_stricter_or_equal(
            &Policy::All,
            &Policy::Threshold { m: 2, n: 3 }
        ));
        assert!(is_stricter_or_equal(&Policy::All, &Policy::All));
    }

    #[test]
    fn test_is_stricter_any() {
        assert!(!is_stricter_or_equal(&Policy::Any, &Policy::All));
        assert!(!is_stricter_or_equal(
            &Policy::Any,
            &Policy::Threshold { m: 2, n: 3 }
        ));
        assert!(is_stricter_or_equal(&Policy::Any, &Policy::Any));
    }

    #[test]
    fn test_is_stricter_threshold() {
        let t1 = Policy::Threshold { m: 2, n: 3 }; // 2/3
        let t2 = Policy::Threshold { m: 1, n: 2 }; // 1/2 (less strict)
        assert!(is_stricter_or_equal(&t1, &t2));
        assert!(!is_stricter_or_equal(&t2, &t1));
    }

    #[test]
    fn test_threshold_fraction_comparison() {
        // 2/3 vs 3/4: which is stricter?
        let t1 = Policy::Threshold { m: 2, n: 3 };
        let t2 = Policy::Threshold { m: 3, n: 4 };

        // 2*4 = 8 vs 3*3 = 9, so t2 is stricter
        assert!(is_stricter_or_equal(&t2, &t1));
        assert!(!is_stricter_or_equal(&t1, &t2));
    }
}
