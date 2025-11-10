//! Proptest strategies for capability generation

use crate::{Capability, CapabilitySet, Policy};
use proptest::prelude::*;
use std::collections::BTreeSet;

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
        "[a-z]{1,20}".prop_map(|s| Capability::Delegate { target_pattern: s }),
    ]
}

/// Generate arbitrary capability sets
pub fn arb_capability_set() -> impl Strategy<Value = CapabilitySet> {
    prop::collection::btree_set(arb_capability(), 0..10).prop_map(CapabilitySet::new)
}

/// Generate arbitrary policies
pub fn arb_policy() -> impl Strategy<Value = Policy> {
    (arb_capability_set(), "[a-z]{1,20}").prop_map(|(caps, context)| Policy::new(caps, context))
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
            assert!(set.capabilities().len() <= 10);
        }
    }
}
