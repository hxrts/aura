//! Property-based testing for meet semi-lattice algebraic laws
//!
//! This module validates that meet semi-lattice implementations satisfy
//! the required algebraic properties through property-based testing.

#[cfg(test)]
mod tests {
    use aura_core::semilattice::{MeetSemiLattice, MvState, Top};
    use proptest::collection;
    use proptest::prelude::*;
    use proptest::proptest;
    use std::collections::BTreeSet;

    // === Test Strategies ===

    /// Strategy for generating arbitrary BTreeSet<String> values
    fn btree_string_set_strategy() -> impl Strategy<Value = BTreeSet<String>> {
        collection::btree_set(
            "[a-z]{1,5}", // Generate simple strings
            0..10,        // Sets with 0-10 elements
        )
    }

    /// Strategy for generating u64 values
    fn u64_strategy() -> impl Strategy<Value = u64> {
        any::<u64>()
    }

    // === Property Tests for u64 ===

    proptest! {
        /// Test meet commutativity for u64: a ∧ b = b ∧ a
        #[test]
        fn test_u64_meet_commutativity(a in u64_strategy(), b in u64_strategy()) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        /// Test meet associativity for u64: (a ∧ b) ∧ c = a ∧ (b ∧ c)
        #[test]
        fn test_u64_meet_associativity(
            a in u64_strategy(),
            b in u64_strategy(),
            c in u64_strategy()
        ) {
            let left = a.meet(&b).meet(&c);
            let right = a.meet(&b.meet(&c));
            prop_assert_eq!(left, right);
        }

        /// Test meet idempotence for u64: a ∧ a = a
        #[test]
        fn test_u64_meet_idempotence(a in u64_strategy()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        /// Test meet identity for u64: a ∧ ⊤ = a
        #[test]
        fn test_u64_meet_identity(a in u64_strategy()) {
            let top = <u64 as Top>::top();
            prop_assert_eq!(a.meet(&top), a);
        }

        /// Test meet ordering for u64: (a ∧ b) ≤ a and (a ∧ b) ≤ b
        #[test]
        fn test_u64_meet_ordering(a in u64_strategy(), b in u64_strategy()) {
            let meet_result = a.meet(&b);
            prop_assert!(meet_result <= a);
            prop_assert!(meet_result <= b);
        }
    }

    // === Property Tests for BTreeSet<String> ===

    proptest! {
        /// Test meet commutativity for BTreeSet: a ∧ b = b ∧ a
        #[test]
        fn test_btreeset_meet_commutativity(
            a in btree_string_set_strategy(),
            b in btree_string_set_strategy()
        ) {
            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        /// Test meet associativity for BTreeSet: (a ∧ b) ∧ c = a ∧ (b ∧ c)
        #[test]
        fn test_btreeset_meet_associativity(
            a in btree_string_set_strategy(),
            b in btree_string_set_strategy(),
            c in btree_string_set_strategy()
        ) {
            let left = a.meet(&b).meet(&c);
            let right = a.meet(&b.meet(&c));
            prop_assert_eq!(left, right);
        }

        /// Test meet idempotence for BTreeSet: a ∧ a = a
        #[test]
        fn test_btreeset_meet_idempotence(a in btree_string_set_strategy()) {
            prop_assert_eq!(a.meet(&a), a);
        }

        // Note: BTreeSet does not have a top element (no universal set)
        // so we don't test the identity law a ∧ ⊤ = a for BTreeSet

        /// Test meet is subset for BTreeSet: (a ∧ b) ⊆ a and (a ∧ b) ⊆ b
        #[test]
        fn test_btreeset_meet_subset(
            a in btree_string_set_strategy(),
            b in btree_string_set_strategy()
        ) {
            let meet_result = a.meet(&b);
            prop_assert!(meet_result.is_subset(&a));
            prop_assert!(meet_result.is_subset(&b));
        }

        /// Test meet is intersection for BTreeSet
        #[test]
        fn test_btreeset_meet_is_intersection(
            a in btree_string_set_strategy(),
            b in btree_string_set_strategy()
        ) {
            let meet_result = a.meet(&b);
            let intersection = a.intersection(&b).cloned().collect::<BTreeSet<String>>();
            prop_assert_eq!(meet_result, intersection);
        }
    }

    // === Combined Property Tests ===

    proptest! {
        /// Test monotonicity: if a ⊆ b, then a ∧ c ⊆ b ∧ c
        #[test]
        fn test_btreeset_meet_monotonicity(
            a in btree_string_set_strategy(),
            b in btree_string_set_strategy(),
            c in btree_string_set_strategy()
        ) {
            // Ensure a ⊆ b by making a the intersection of a and b
            let a_subset = a.intersection(&b).cloned().collect::<BTreeSet<String>>();

            let left_meet = a_subset.meet(&c);
            let right_meet = b.meet(&c);

            prop_assert!(left_meet.is_subset(&right_meet));
        }

        /// Test absorption properties: a ∧ (a ∨ b) = a
        /// Note: This requires both meet and join, testing with supersets
        #[test]
        fn test_btreeset_absorption(
            a in btree_string_set_strategy(),
            b in btree_string_set_strategy()
        ) {
            // Create union (join) of a and b
            let union = a.union(&b).cloned().collect::<BTreeSet<String>>();

            // Meet with original set should give back original
            let result = a.meet(&union);
            prop_assert_eq!(result, a);
        }
    }

    // === Regression Tests ===

    #[test]
    fn test_u64_meet_specific_cases() {
        // Test specific known cases
        assert_eq!(42u64.meet(&17u64), 17u64);
        assert_eq!(100u64.meet(&100u64), 100u64);
        assert_eq!(u64::MAX.meet(&0u64), 0u64);
        assert_eq!(u64::MAX.meet(&u64::MAX), u64::MAX);
    }

    #[test]
    fn test_btreeset_meet_specific_cases() {
        let set1: BTreeSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let set2: BTreeSet<String> = ["b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let expected: BTreeSet<String> = ["b", "c"].iter().map(|s| s.to_string()).collect();

        assert_eq!(set1.meet(&set2), expected);

        // Test with empty set
        let empty: BTreeSet<String> = BTreeSet::new();
        assert_eq!(set1.meet(&empty), empty);

        // Test with identical sets
        assert_eq!(set1.meet(&set1), set1);
    }

    #[test]
    fn test_top_element_properties() {
        // Test that top element behaves correctly for u64
        let top_u64 = <u64 as Top>::top();

        // Top should be the greatest element (most permissive)
        assert_eq!(top_u64, u64::MAX);

        // Any element meet with top should equal the element (identity)
        assert_eq!(42u64.meet(&top_u64), 42u64);

        // Note: BTreeSet does not have a top element (no universal set)
        // so we don't test top properties for BTreeSet
    }

    // === Integration Tests with Domain Types ===

    #[test]
    fn test_mv_state_trait_implementations() {
        // Test that our implementations satisfy the MvState trait
        fn assert_mv_state<T: MvState>() {}

        assert_mv_state::<u64>();
        assert_mv_state::<BTreeSet<String>>();
    }

    // === Performance Tests ===

    #[test]
    fn test_meet_performance() {
        // Test that meet operations are reasonably fast
        let large_set1: BTreeSet<String> = (0..1000).map(|i| format!("item_{i}")).collect();
        let large_set2: BTreeSet<String> = (500..1500).map(|i| format!("item_{i}")).collect();

        // Performance test - just ensure it completes without timing specifics
        let _result = large_set1.meet(&large_set2);
        // Test passes if no panic/timeout occurs
    }

    // === Error Cases and Edge Cases ===

    #[test]
    fn test_edge_cases() {
        // Test with boundary values
        assert_eq!(0u64.meet(&u64::MAX), 0u64);
        assert_eq!(u64::MAX.meet(&0u64), 0u64);

        // Test with empty sets
        let empty1: BTreeSet<String> = BTreeSet::new();
        let empty2: BTreeSet<String> = BTreeSet::new();
        assert_eq!(empty1.meet(&empty2), BTreeSet::new());

        // Test with single element sets
        let single1: BTreeSet<String> = ["test"].iter().map(|s| s.to_string()).collect();
        let single2: BTreeSet<String> = ["test"].iter().map(|s| s.to_string()).collect();
        assert_eq!(single1.meet(&single2), single1);

        let single3: BTreeSet<String> = ["other"].iter().map(|s| s.to_string()).collect();
        assert_eq!(single1.meet(&single3), BTreeSet::new());
    }
}

// === Utility Functions for Testing ===

#[cfg(test)]
mod test_utils {
    use aura_core::semilattice::MeetSemiLattice;
    use std::collections::BTreeSet;

    /// Helper to verify core meet semi-lattice laws (commutativity, associativity, idempotence)
    ///
    /// These are the fundamental laws that all meet semilattices must satisfy.
    /// Does not test identity law since not all types have a top element.
    pub fn verify_meet_laws<T>(a: &T, b: &T, c: &T) -> bool
    where
        T: MeetSemiLattice + PartialEq + Clone,
    {
        // Commutativity: a ∧ b = b ∧ a
        let commutative = a.meet(b) == b.meet(a);

        // Associativity: (a ∧ b) ∧ c = a ∧ (b ∧ c)
        let left_assoc = a.meet(b).meet(c);
        let right_assoc = a.meet(&b.meet(c));
        let associative = left_assoc == right_assoc;

        // Idempotence: a ∧ a = a
        let idempotent = a.meet(a) == *a;

        commutative && associative && idempotent
    }

    /// Helper to verify meet semi-lattice laws including identity for u64
    ///
    /// u64 has a proper top element (u64::MAX) so we can test the identity law.
    pub fn verify_meet_laws_with_top_u64(a: u64, b: u64, c: u64) -> bool {
        // All core laws
        let core = verify_meet_laws(&a, &b, &c);

        // Identity: a ∧ ⊤ = a (only valid for types with top element like u64)
        let identity = a.meet(&u64::MAX) == a;

        core && identity
    }

    /// Helper to check if result of meet is properly bounded for u64
    /// For u64, meet is min, so result should be <= both inputs
    pub fn verify_meet_bounds_u64(a: u64, b: u64, result: u64) -> bool {
        result <= a && result <= b
    }

    /// Helper to check if result of meet is properly bounded for BTreeSet
    /// For BTreeSet, meet is intersection, so result should be subset of both inputs
    pub fn verify_meet_bounds_btreeset<T: Ord>(
        a: &BTreeSet<T>,
        b: &BTreeSet<T>,
        result: &BTreeSet<T>,
    ) -> bool {
        result.is_subset(a) && result.is_subset(b)
    }
}

#[cfg(test)]
mod helper_tests {
    use super::test_utils::*;
    use aura_core::semilattice::MeetSemiLattice;
    use proptest::prelude::*;

    proptest! {
        /// Test that verify_meet_laws correctly validates u64 meet operations (with top)
        #[test]
        fn test_verify_meet_laws_u64(
            a in any::<u64>(),
            b in any::<u64>(),
            c in any::<u64>()
        ) {
            // All u64 values should satisfy meet laws including identity
            prop_assert!(verify_meet_laws_with_top_u64(a, b, c));
        }

        /// Test that verify_meet_laws correctly validates BTreeSet meet operations
        #[test]
        fn test_verify_meet_laws_btreeset(
            a in prop::collection::btree_set("[a-z]{1,3}", 0..5),
            b in prop::collection::btree_set("[a-z]{1,3}", 0..5),
            c in prop::collection::btree_set("[a-z]{1,3}", 0..5)
        ) {
            // All BTreeSet values should satisfy core meet laws (no top element)
            prop_assert!(verify_meet_laws(&a, &b, &c));
        }

        /// Test that verify_meet_bounds_u64 correctly validates u64 meet bounds
        #[test]
        fn test_verify_meet_bounds_u64(a in any::<u64>(), b in any::<u64>()) {
            let result = a.meet(&b);
            // Meet result should always be bounded by inputs
            prop_assert!(verify_meet_bounds_u64(a, b, result));
        }

        /// Test that verify_meet_bounds_btreeset correctly validates BTreeSet meet bounds
        #[test]
        fn test_verify_meet_bounds_btreeset(
            a in prop::collection::btree_set("[a-z]{1,3}", 0..5),
            b in prop::collection::btree_set("[a-z]{1,3}", 0..5)
        ) {
            let result = a.meet(&b);
            // Meet result (intersection) should be subset of both inputs
            prop_assert!(verify_meet_bounds_btreeset(&a, &b, &result));
        }
    }
}
