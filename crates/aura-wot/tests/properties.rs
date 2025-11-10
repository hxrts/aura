//! Property tests verifying meet-semilattice laws for capabilities

use super::strategies::*;
use crate::{Capability, CapabilitySet, Policy};
use aura_core::semilattice::{MeetSemiLattice, Top};
use proptest::prelude::*;

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
        for cap in result.capabilities() {
            prop_assert!(
                a.capabilities().contains(cap) && b.capabilities().contains(cap),
                "Meet result contains capability not in both inputs: {:?}", cap
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

    /// Test policy meet operations
    #[test]
    fn test_policy_meet_laws(a in arb_policy(), b in arb_policy()) {
        // Policies should also satisfy meet semilattice laws
        let ab = a.meet(&b);
        let ba = b.meet(&a);
        prop_assert_eq!(ab, ba, "Policy meet not commutative");

        // Meet result should be more restrictive than inputs
        prop_assert!(
            ab.effective_capabilities().capabilities().len() <=
            a.effective_capabilities().capabilities().len().min(
                b.effective_capabilities().capabilities().len()
            ),
            "Policy meet should be more restrictive"
        );
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use std::collections::BTreeSet;

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

        let set1 = CapabilitySet::new([read_a.clone(), read_b.clone()].into());
        let set2 = CapabilitySet::new([read_a.clone(), write_a.clone()].into());

        let intersection = set1.meet(&set2);

        // Should only contain capabilities present in both sets
        assert!(intersection.capabilities().contains(&read_a));
        assert!(!intersection.capabilities().contains(&read_b));
        assert!(!intersection.capabilities().contains(&write_a));
    }

    #[test]
    fn test_empty_capability_set_meet() {
        let empty = CapabilitySet::new(BTreeSet::new());
        let read_cap = CapabilitySet::new(
            [Capability::Read {
                resource_pattern: "test".to_string(),
            }]
            .into(),
        );

        let result = empty.meet(&read_cap);
        assert!(
            result.capabilities().is_empty(),
            "Meet with empty should be empty"
        );

        let result2 = read_cap.meet(&empty);
        assert!(
            result2.capabilities().is_empty(),
            "Meet with empty should be empty"
        );
    }
}
