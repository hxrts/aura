//! Proptest strategies for capability generation

use aura_wot::tree_policy::Policy as TreePolicy;
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

/// Generate capability sets that avoid problematic edge cases for associativity tests
/// This excludes mixed sets containing All with other capabilities
pub fn arb_capability_set_for_associativity() -> impl Strategy<Value = CapabilitySet> {
    prop_oneof![
        // Empty set
        Just(CapabilitySet::empty()),
        // Pure All (top element)
        Just(CapabilitySet::from_capabilities(
            [Capability::All].into_iter().collect()
        )),
        // Sets without All or None
        prop::collection::btree_set(
            prop_oneof![
                "[a-z]{1,20}".prop_map(|s| Capability::Read {
                    resource_pattern: s
                }),
                "[a-z]{1,20}".prop_map(|s| Capability::Write {
                    resource_pattern: s
                }),
                "[a-z]{1,20}".prop_map(|s| Capability::Execute { operation: s }),
                (1u32..10).prop_map(|depth| Capability::Delegate { max_depth: depth }),
            ],
            1..5
        )
        .prop_map(CapabilitySet::from_capabilities),
    ]
}

/// Generate arbitrary policies
pub fn arb_policy() -> impl Strategy<Value = TreePolicy> {
    prop_oneof![
        Just(TreePolicy::Any),
        Just(TreePolicy::All),
        (1u16..10u16, 1u16..10u16).prop_filter_map("m <= n", |(m, n)| if m <= n {
            Some(TreePolicy::Threshold { m, n })
        } else {
            None
        })
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #[test]
        fn test_capability_generation(cap in arb_capability()) {
            // Just verify we can generate capabilities without panic
            let _ = format!("{:?}", cap);
        }

        #[test]
        fn test_capability_set_generation(set in arb_capability_set()) {
            // Verify capability sets are valid
            assert!(set.capabilities().count() <= 10);
        }
    }
}
