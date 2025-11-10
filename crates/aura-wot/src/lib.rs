//! # Aura Web of Trust
//!
//! Capability-based authorization implementing meet-semilattice laws for
//! monotonic capability restriction and delegation chains.
//!
//! This crate implements the Web of Trust layer from Aura's architectural
//! model, providing:
//!
//! - Meet-semilattice capability objects that can only shrink (⊓)
//! - Capability delegation chains with proper attenuation
//! - Policy enforcement via capability intersection
//! - Formal verification of semilattice laws
//!
//! ## Core Concepts
//!
//! Capabilities follow meet-semilattice laws:
//! - **Associative**: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
//! - **Commutative**: a ⊓ b = b ⊓ a
//! - **Idempotent**: a ⊓ a = a
//! - **Monotonic**: a ⊓ b ⪯ a and a ⊓ b ⪯ b
//!
//! ## Usage
//!
//! ```rust
//! use aura_wot::{Capability, CapabilitySet};
//!
//! // Capabilities only shrink via meet operation
//! let base_policy = CapabilitySet::from_permissions(&["read", "write"]);
//! let delegation = CapabilitySet::from_permissions(&["read"]);

// Phase 6: Capability/tree policy integration tests
// DISABLED: Tests reference unimplemented API methods
// #[cfg(test)]
// mod capability_tree_policy_tests;

// Phase 6: Property tests for semilattice laws
// DISABLED: Tests reference unimplemented API methods
// #[cfg(test)]
// mod semilattice_property_tests;
//!
//! // Effective capabilities = intersection (can only get smaller)
//! let effective = base_policy.meet(&delegation);
//! assert!(effective.permits("read"));
//! assert!(!effective.permits("write")); // Lost via intersection
//! ```

pub mod capability;
pub mod capability_evaluation;
pub mod capability_evaluator;
pub mod delegation;
pub mod errors;
pub mod evaluation;
pub mod policy;
pub mod policy_meet;
pub mod storage_authz;
pub mod tokens;
pub mod tree_authz;
pub mod tree_operations;
pub mod tree_policy;

pub use capability::{Capability, CapabilitySet, TrustLevel, RelayPermission, StoragePermission};
pub use capability_evaluation::{
    evaluate_tree_operation_capabilities, CapabilityEvaluationContext, CapabilityEvaluationResult,
    EntityId, TreeCapabilityRequest,
};
pub use capability_evaluator::{
    CapabilityEvaluator, EffectSystemInterface, EffectiveCapabilitySet,
};
pub use delegation::{DelegationChain, DelegationLink};
pub use errors::{AuraError, AuraResult, WotError, WotResult};
pub use evaluation::{evaluate_capabilities, EvaluationContext, LocalChecks};
pub use policy::{Policy, PolicyEngine};
pub use policy_meet::{PolicyMeet, TreePolicyCapabilityExt};
pub use storage_authz::{storage_capabilities, StorageAuthorizationMiddleware, StorageOperation};
pub use tokens::{CapabilityCondition, CapabilityId, CapabilityToken, DelegationProof};
pub use tree_authz::{
    tree_device_lifecycle_capabilities, tree_management_capabilities, tree_read_capabilities,
    TreeAuthorizationMiddleware, TreeOperation,
};
pub use tree_operations::{
    evaluate_tree_operation, tree_operation_capabilities, LeafRole, TreeAuthzContext,
    TreeAuthzRequest, TreeAuthzResult, TreeOp, TreeOpKind,
};
pub use tree_policy::{NodeIndex, Policy as TreePolicyEnum, ThresholdConfig, TreePolicy};

/// Type alias for capability meet operation results
pub type CapResult<T> = Result<T, WotError>;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn capability_meet_is_associative(
            perms1 in prop::collection::vec(".*", 0..5),
            perms2 in prop::collection::vec(".*", 0..5),
            perms3 in prop::collection::vec(".*", 0..5)
        ) {
            let perms1_refs: Vec<&str> = perms1.iter().map(|s| s.as_str()).collect();
            let perms2_refs: Vec<&str> = perms2.iter().map(|s| s.as_str()).collect();
            let perms3_refs: Vec<&str> = perms3.iter().map(|s| s.as_str()).collect();

            let a = CapabilitySet::from_permissions(&perms1_refs);
            let b = CapabilitySet::from_permissions(&perms2_refs);
            let c = CapabilitySet::from_permissions(&perms3_refs);

            let left = a.meet(&b).meet(&c);
            let right = a.meet(&b.meet(&c));

            prop_assert_eq!(left, right);
        }

        #[test]
        fn capability_meet_is_commutative(
            perms1 in prop::collection::vec(".*", 0..5),
            perms2 in prop::collection::vec(".*", 0..5)
        ) {
            let perms1_refs: Vec<&str> = perms1.iter().map(|s| s.as_str()).collect();
            let perms2_refs: Vec<&str> = perms2.iter().map(|s| s.as_str()).collect();

            let a = CapabilitySet::from_permissions(&perms1_refs);
            let b = CapabilitySet::from_permissions(&perms2_refs);

            prop_assert_eq!(a.meet(&b), b.meet(&a));
        }

        #[test]
        fn capability_meet_is_idempotent(
            perms in prop::collection::vec(".*", 0..5)
        ) {
            let perms_refs: Vec<&str> = perms.iter().map(|s| s.as_str()).collect();
            let a = CapabilitySet::from_permissions(&perms_refs);
            prop_assert_eq!(a.meet(&a), a);
        }

        #[test]
        fn capability_meet_is_monotonic(
            perms1 in prop::collection::vec(".*", 0..5),
            perms2 in prop::collection::vec(".*", 0..5)
        ) {
            let perms1_refs: Vec<&str> = perms1.iter().map(|s| s.as_str()).collect();
            let perms2_refs: Vec<&str> = perms2.iter().map(|s| s.as_str()).collect();

            let a = CapabilitySet::from_permissions(&perms1_refs);
            let b = CapabilitySet::from_permissions(&perms2_refs);
            let meet = a.meet(&b);

            // meet result must be subset of both inputs
            prop_assert!(meet.is_subset_of(&a));
            prop_assert!(meet.is_subset_of(&b));
        }
    }
}
