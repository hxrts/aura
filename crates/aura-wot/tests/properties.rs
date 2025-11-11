//! Property tests verifying meet-semilattice laws for capabilities

use aura_core::semilattice::Top;
use aura_wot::{Capability, CapabilitySet};
use proptest::prelude::*;

/// Generate arbitrary capabilities for testing
pub fn arb_capability() -> impl Strategy<Value = Capability> {
    prop_oneof![
        "[a-z]{1,20}".prop_map(|s| Capability::Read {
            resource_pattern: s
        }),
        "[a-z]{1,20}".prop_map(|s| Capability::Write {
            resource_pattern: s
        }),
        "[a-z]{1,20}".prop_map(|s| Capability::Execute { operation: s }),
        (1u32..10).prop_map(|depth| Capability::Delegate { max_depth: depth }),
        Just(Capability::All),
        Just(Capability::None),
    ]
}

/// Generate arbitrary capability sets
pub fn arb_capability_set() -> impl Strategy<Value = CapabilitySet> {
    prop::collection::btree_set(arb_capability(), 0..10).prop_map(CapabilitySet::from_capabilities)
}

proptest! {
    /// Test idempotency: a ⊓ a = a
    #[test]
    fn test_meet_idempotency(set in arb_capability_set()) {
        let result = set.meet(&set);
        prop_assert_eq!(result, set);
    }

    /// Test commutativity: a ⊓ b = b ⊓ a
    #[test]
    fn test_meet_commutativity(a in arb_capability_set(), b in arb_capability_set()) {
        let ab = a.meet(&b);
        let ba = b.meet(&a);
        prop_assert_eq!(ab, ba);
    }

    /// Test associativity: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
    #[test]
    fn test_meet_associativity(
        a in arb_capability_set(),
        b in arb_capability_set(),
        c in arb_capability_set()
    ) {
        let ab_c = a.meet(&b).meet(&c);
        let a_bc = a.meet(&b.meet(&c));
        prop_assert_eq!(ab_c, a_bc);
    }

    /// Test monotonicity: meet never increases authority
    #[test]
    fn test_meet_monotonicity(a in arb_capability_set(), b in arb_capability_set()) {
        let result = a.meet(&b);

        // Result should be subset of both inputs (more restrictive)
        // For capability semilattice, All implies every capability
        for cap in result.capabilities() {
            let in_a = a.capabilities().any(|c| c == cap) || a.capabilities().any(|c| *c == Capability::All);
            let in_b = b.capabilities().any(|c| c == cap) || b.capabilities().any(|c| *c == Capability::All);

            prop_assert!(
                in_a && in_b,
                "Meet result contains capability not logically present in both inputs: {:?}", cap
            );
        }
    }

    /// Test top element: a ⊓ ⊤ = a
    #[test]
    fn test_meet_top_identity(set in arb_capability_set()) {
        let top = CapabilitySet::top();
        let result = set.meet(&top);
        prop_assert_eq!(result, set);
    }

    // Commented out due to missing arb_policy function and API changes
    // /// Test policy meet operations
    // #[test]
    // fn test_policy_meet_laws(a in arb_policy(), b in arb_policy()) {
    //     // Policies should also satisfy meet semilattice laws
    //     let ab = a.meet(&b);
    //     let ba = b.meet(&a);
    //     prop_assert_eq!(ab, ba, "Policy meet not commutative");
    // }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_capability_meet_examples() {
        let read_a = Capability::Read {
            resource_pattern: "a".to_string(),
        };
        let read_b = Capability::Read {
            resource_pattern: "b".to_string(),
        };
        let write_a = Capability::Write {
            resource_pattern: "a".to_string(),
        };

        let set1 = CapabilitySet::from_capabilities([read_a.clone(), read_b.clone()].into());
        let set2 = CapabilitySet::from_capabilities([read_a.clone(), write_a.clone()].into());

        let intersection = set1.meet(&set2);

        // Should only contain capabilities present in both sets
        assert!(intersection.capabilities().any(|cap| cap == &read_a));
        assert!(!intersection.capabilities().any(|cap| cap == &read_b));
        assert!(!intersection.capabilities().any(|cap| cap == &write_a));
    }

    #[test]
    fn test_empty_capability_set_meet() {
        let empty = CapabilitySet::empty();
        let read_cap = CapabilitySet::from_permissions(&["read:test"]);

        let result = empty.meet(&read_cap);
        assert!(
            result.capabilities().count() == 0,
            "Meet with empty should be empty"
        );

        let result2 = read_cap.meet(&empty);
        assert!(
            result2.capabilities().count() == 0,
            "Meet with empty should be empty"
        );
    }
}
