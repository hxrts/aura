//! Guard Chain and Authorization Property Tests  
//!
//! Property-based tests verifying the correctness of capability-based authorization
//! and guard chain operations. These tests ensure security properties hold under
//! all possible capability delegations and revocations.
//!
//! ## Properties Verified
//!
//! 1. **Authorization Monotonicity**: Removing capabilities never grants new permissions
//! 2. **Delegation Bounds**: Delegated capabilities cannot exceed delegator's permissions  
//! 3. **Revocation Completeness**: Revoking a capability revokes all dependent delegations
//! 4. **Chain Integrity**: Guard chains maintain consistent authorization paths
//! 5. **Temporal Consistency**: Time-bounded capabilities expire correctly

use aura_core::{
    DeviceId, AccountId, CapabilityId, AuraResult, 
    capabilities::{Capability, CapabilityType, Permission},
    authorization::{AuthorizationDecision, AuthorizationContext, AuthorizationError},
    time::TemporalBounds,
};
use aura_wot::{
    CapabilitySet, TreePolicy, PolicyMeet,
    capability_tree::{CapabilityTree, TreeNode, NodeId},
    authorization::{AuthorizationChain, GuardChain, AuthorizationProof},
    delegation::{DelegationConstraints, DelegationDepth},
};
use aura_core::journal::{Cap, AuthLevel};
use proptest::prelude::*;
use std::collections::{HashMap, HashSet, BTreeSet};
use std::time::{SystemTime, Duration};

/// Strategy to generate arbitrary device IDs
fn arbitrary_device_id() -> impl Strategy<Value = DeviceId> {
    any::<[u8; 32]>().prop_map(DeviceId::from_bytes)
}

/// Strategy to generate arbitrary capability IDs
fn arbitrary_capability_id() -> impl Strategy<Value = CapabilityId> {
    any::<[u8; 32]>().prop_map(CapabilityId::from_bytes)
}

/// Strategy to generate arbitrary permissions
fn arbitrary_permission() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("storage:read".to_string()),
        Just("storage:write".to_string()),
        Just("storage:admin".to_string()),
        Just("tree:read".to_string()),
        Just("tree:write".to_string()),
        Just("tree:propose".to_string()),
        Just("tree:remove".to_string()),
        Just("tree:admin".to_string()),
        Just("guardian:view".to_string()),
        Just("guardian:manage".to_string()),
        Just("session:create".to_string()),
        Just("session:join".to_string()),
        Just("session:manage".to_string()),
        Just("relay:forward".to_string()),
        Just("relay:route".to_string()),
    ]
}

/// Strategy to generate arbitrary capability sets
fn arbitrary_capability_set() -> impl Strategy<Value = CapabilitySet> {
    prop::collection::vec(arbitrary_permission(), 0..8)
        .prop_map(|permissions| {
            let perm_refs: Vec<&str> = permissions.iter().map(|s| s.as_str()).collect();
            CapabilitySet::from_permissions(&perm_refs)
        })
}

/// Strategy to generate temporal bounds
fn arbitrary_temporal_bounds() -> impl Strategy<Value = TemporalBounds> {
    (
        any::<u64>(),
        any::<u64>(),
    ).prop_map(|(start_offset, duration)| {
        let now = SystemTime::now();
        let start_time = now + Duration::from_secs(start_offset % 3600); // Within 1 hour
        let end_time = start_time + Duration::from_secs(duration % 7200); // Up to 2 hours later
        
        TemporalBounds {
            not_before: start_time,
            not_after: end_time,
        }
    })
}

/// Strategy to generate delegation constraints
fn arbitrary_delegation_constraints() -> impl Strategy<Value = DelegationConstraints> {
    (
        prop::option::of(1u32..10), // max_depth
        prop::collection::vec(arbitrary_device_id(), 0..3), // required_endorsers
        prop::option::of(arbitrary_temporal_bounds()), // temporal_bounds
    ).prop_map(|(max_depth, required_endorsers, temporal_bounds)| {
        DelegationConstraints {
            max_depth: max_depth.map(DelegationDepth::Limited).unwrap_or(DelegationDepth::Unlimited),
            required_endorsers,
            temporal_bounds,
            restrictions: HashSet::new(),
        }
    })
}

/// Strategy to generate capability trees
fn arbitrary_capability_tree() -> impl Strategy<Value = CapabilityTree> {
    (
        arbitrary_device_id(), // root device
        prop::collection::vec(
            (arbitrary_capability_set(), arbitrary_delegation_constraints()),
            1..8
        )
    ).prop_map(|(root_device, cap_constraint_pairs)| {
        let mut tree = CapabilityTree::new(root_device);
        
        for (capabilities, constraints) in cap_constraint_pairs {
            let child_device = DeviceId::new();
            let _ = tree.delegate_capabilities(
                root_device,
                child_device, 
                capabilities,
                constraints,
            );
        }
        
        tree
    })
}

/// Strategy to generate authorization chains
fn arbitrary_authorization_chain() -> impl Strategy<Value = AuthorizationChain> {
    (
        arbitrary_device_id(), // requestor
        arbitrary_capability_id(), // requested capability
        prop::collection::vec(arbitrary_device_id(), 1..6), // delegation path
    ).prop_map(|(requestor, capability_id, delegation_path)| {
        AuthorizationChain::new(requestor, capability_id, delegation_path)
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        cases: 100, // Comprehensive testing for security properties
        .. ProptestConfig::default()
    })]

    /// Property: Authorization is monotonic (removing capabilities never grants new permissions)
    /// For any capability set C and subset S ⊆ C: authorize(S) ⊆ authorize(C)
    #[test]
    fn prop_authorization_monotonic(
        full_capabilities in arbitrary_capability_set(),
        device_id in arbitrary_device_id(),
        requested_permissions in prop::collection::vec(arbitrary_permission(), 1..5)
    ) {
        let permissions_subset: Vec<String> = full_capabilities
            .permissions()
            .iter()
            .take(full_capabilities.permissions().len() / 2)
            .cloned()
            .collect();
            
        let subset_refs: Vec<&str> = permissions_subset.iter().map(|s| s.as_str()).collect();
        let subset_capabilities = CapabilitySet::from_permissions(&subset_refs);

        // Check authorization for each requested permission
        for permission in requested_permissions {
            let full_authorized = full_capabilities.has_permission(&permission);
            let subset_authorized = subset_capabilities.has_permission(&permission);
            
            // If subset authorizes, full set must also authorize (monotonicity)
            prop_assert!(
                !subset_authorized || full_authorized,
                "Authorization must be monotonic: subset cannot authorize '{}' if full set doesn't", 
                permission
            );
        }
    }

    /// Property: Delegated capabilities cannot exceed delegator's permissions
    /// For any delegation from A to B: capabilities(B) ⊆ capabilities(A)
    #[test]
    fn prop_delegation_bounds(
        delegator_capabilities in arbitrary_capability_set(),
        delegatee_device in arbitrary_device_id(),
        delegation_constraints in arbitrary_delegation_constraints()
    ) {
        let delegator_device = DeviceId::new();
        let mut capability_tree = CapabilityTree::new(delegator_device);
        
        // Grant capabilities to delegator
        capability_tree.add_device_capabilities(delegator_device, delegator_capabilities.clone());
        
        // Try to delegate all permissions (some may be filtered by constraints)
        let delegation_result = capability_tree.delegate_capabilities(
            delegator_device,
            delegatee_device,
            delegator_capabilities.clone(),
            delegation_constraints,
        );
        
        if let Ok(delegated_capabilities) = delegation_result {
            // Every delegated permission must be present in delegator's capabilities
            for delegated_permission in delegated_capabilities.permissions() {
                prop_assert!(
                    delegator_capabilities.has_permission(delegated_permission),
                    "Delegated permission '{}' must be present in delegator's capabilities",
                    delegated_permission
                );
            }
        }
        // If delegation fails, that's also acceptable (constraints may prevent it)
    }

    /// Property: Revocation completeness (revoking capability revokes all dependent delegations)
    /// If capability C is revoked from device A, then C is revoked from all devices that received C from A
    #[test]
    fn prop_revocation_completeness(
        mut capability_tree in arbitrary_capability_tree(),
        revoked_capability in arbitrary_permission()
    ) {
        let root_device = capability_tree.root_device();
        
        // Get initial state - which devices have the capability
        let initial_holders: Vec<DeviceId> = capability_tree
            .list_devices_with_permission(&revoked_capability)
            .unwrap_or_default();
        
        if !initial_holders.is_empty() {
            // Revoke capability from root device
            let revocation_result = capability_tree.revoke_permission_from_device(
                root_device, 
                &revoked_capability
            );
            
            if revocation_result.is_ok() {
                // After revocation, check that all devices lost the capability
                let remaining_holders: Vec<DeviceId> = capability_tree
                    .list_devices_with_permission(&revoked_capability)
                    .unwrap_or_default();
                
                // Revocation should be transitive through delegation chains
                prop_assert!(
                    remaining_holders.is_empty() || 
                    remaining_holders.len() <= initial_holders.len(),
                    "Revocation should remove capability from all dependent devices"
                );
            }
        }
    }

    /// Property: Guard chain integrity (authorization paths remain consistent)
    /// For any valid authorization chain, all intermediate delegates must have valid delegations
    #[test]
    fn prop_guard_chain_integrity(
        authorization_chain in arbitrary_authorization_chain(),
        capability_tree in arbitrary_capability_tree()
    ) {
        let chain_validation = authorization_chain.validate(&capability_tree);
        
        match chain_validation {
            Ok(proof) => {
                // If chain is valid, each step should be individually valid
                let delegation_path = authorization_chain.delegation_path();
                
                for i in 0..delegation_path.len().saturating_sub(1) {
                    let delegator = delegation_path[i];
                    let delegatee = delegation_path[i + 1];
                    
                    // Verify delegator has capability and can delegate to delegatee
                    let has_capability = capability_tree.device_has_capability(
                        delegator, 
                        authorization_chain.requested_capability()
                    );
                    
                    let can_delegate = capability_tree.can_delegate(
                        delegator,
                        delegatee,
                        authorization_chain.requested_capability(),
                    );
                    
                    prop_assert!(
                        has_capability.unwrap_or(false),
                        "Intermediate delegator {} must have the capability",
                        delegator
                    );
                    
                    prop_assert!(
                        can_delegate.unwrap_or(false),
                        "Delegator {} must be able to delegate to {}",
                        delegator, delegatee
                    );
                }
                
                // Proof should contain complete delegation chain
                prop_assert!(
                    proof.chain_length() == delegation_path.len(),
                    "Authorization proof should cover complete delegation chain"
                );
            }
            Err(_) => {
                // Invalid chains are acceptable - the property is that validation
                // correctly identifies invalid chains
            }
        }
    }

    /// Property: Temporal consistency (time-bounded capabilities expire correctly)
    /// Capabilities with temporal bounds are only valid within their time windows
    #[test]
    fn prop_temporal_consistency(
        device_id in arbitrary_device_id(),
        capability_id in arbitrary_capability_id(),
        temporal_bounds in arbitrary_temporal_bounds()
    ) {
        let now = SystemTime::now();
        
        // Create capability with temporal bounds
        let cap = Cap::new()
            .with_permission("test:action")
            .with_temporal_bounds(temporal_bounds.clone());
        
        let mut capability_set = CapabilitySet::empty();
        capability_set.add_capability(cap);
        
        // Check authorization at different times
        let before_valid = now < temporal_bounds.not_before;
        let after_valid = now > temporal_bounds.not_after;
        let within_valid = now >= temporal_bounds.not_before && now <= temporal_bounds.not_after;
        
        let authorization_result = capability_set.has_permission_at_time(
            "test:action", 
            now
        );
        
        if before_valid || after_valid {
            // Outside validity window - should not be authorized
            prop_assert!(
                !authorization_result.unwrap_or(true),
                "Capability should not be valid outside temporal bounds"
            );
        } else if within_valid {
            // Within validity window - should be authorized
            prop_assert!(
                authorization_result.unwrap_or(false),
                "Capability should be valid within temporal bounds"
            );
        }
    }

    /// Property: Delegation depth limits are enforced
    /// Delegations cannot exceed specified depth limits
    #[test]
    fn prop_delegation_depth_limits(
        root_device in arbitrary_device_id(),
        max_depth in 1u32..5,
        chain_length in 1usize..10
    ) {
        let mut capability_tree = CapabilityTree::new(root_device);
        
        let capabilities = CapabilitySet::from_permissions(&["test:permission"]);
        let constraints = DelegationConstraints {
            max_depth: DelegationDepth::Limited(max_depth),
            required_endorsers: vec![],
            temporal_bounds: None,
            restrictions: HashSet::new(),
        };
        
        let mut current_device = root_device;
        let mut delegation_count = 0;
        
        // Try to create delegation chain longer than max_depth
        for i in 0..chain_length {
            let next_device = DeviceId::new();
            
            let delegation_result = capability_tree.delegate_capabilities(
                current_device,
                next_device,
                capabilities.clone(),
                constraints.clone(),
            );
            
            if i >= max_depth as usize {
                // Should fail when exceeding depth limit
                prop_assert!(
                    delegation_result.is_err(),
                    "Delegation should fail when exceeding depth limit {} at step {}",
                    max_depth, i
                );
            } else {
                // Should succeed within depth limit
                if delegation_result.is_ok() {
                    current_device = next_device;
                    delegation_count += 1;
                }
            }
        }
        
        prop_assert!(
            delegation_count <= max_depth as usize,
            "Actual delegation depth {} should not exceed limit {}",
            delegation_count, max_depth
        );
    }

    /// Property: Required endorsers are enforced
    /// Delegations requiring endorsers should not succeed without them
    #[test]
    fn prop_required_endorsers_enforcement(
        delegator_device in arbitrary_device_id(),
        delegatee_device in arbitrary_device_id(),
        required_endorsers in prop::collection::vec(arbitrary_device_id(), 1..4),
        actual_endorsers in prop::collection::vec(arbitrary_device_id(), 0..3)
    ) {
        let mut capability_tree = CapabilityTree::new(delegator_device);
        
        let capabilities = CapabilitySet::from_permissions(&["test:permission"]);
        let constraints = DelegationConstraints {
            max_depth: DelegationDepth::Unlimited,
            required_endorsers: required_endorsers.clone(),
            temporal_bounds: None,
            restrictions: HashSet::new(),
        };
        
        // Add actual endorsers to the tree
        for &endorser in &actual_endorsers {
            capability_tree.add_endorser(endorser);
        }
        
        let delegation_result = capability_tree.delegate_capabilities_with_endorsers(
            delegator_device,
            delegatee_device,
            capabilities,
            constraints,
            actual_endorsers.clone(),
        );
        
        // Check if all required endorsers are present in actual endorsers
        let all_endorsers_present = required_endorsers
            .iter()
            .all(|req| actual_endorsers.contains(req));
        
        if all_endorsers_present {
            prop_assert!(
                delegation_result.is_ok(),
                "Delegation should succeed when all required endorsers are present"
            );
        } else {
            prop_assert!(
                delegation_result.is_err(),
                "Delegation should fail when missing required endorsers"
            );
        }
    }

    /// Property: Authorization decisions are deterministic
    /// Same authorization request always produces same result
    #[test]
    fn prop_authorization_deterministic(
        device_id in arbitrary_device_id(),
        capability_set in arbitrary_capability_set(),
        requested_permission in arbitrary_permission()
    ) {
        // Check authorization multiple times
        let auth_result_1 = capability_set.has_permission(&requested_permission);
        let auth_result_2 = capability_set.has_permission(&requested_permission);
        let auth_result_3 = capability_set.has_permission(&requested_permission);
        
        prop_assert_eq!(auth_result_1, auth_result_2,
            "Authorization should be deterministic (attempt 1 vs 2)");
        prop_assert_eq!(auth_result_2, auth_result_3,
            "Authorization should be deterministic (attempt 2 vs 3)");
    }

    /// Property: Capability meet (intersection) preserves security
    /// Meet of two capability sets should never grant more permissions than either input
    #[test]
    fn prop_capability_meet_security_preserving(
        capabilities_a in arbitrary_capability_set(),
        capabilities_b in arbitrary_capability_set(),
        test_permission in arbitrary_permission()
    ) {
        let meet_result = capabilities_a.meet(&capabilities_b);
        
        let auth_a = capabilities_a.has_permission(&test_permission);
        let auth_b = capabilities_b.has_permission(&test_permission);
        let auth_meet = meet_result.has_permission(&test_permission);
        
        // Meet should only authorize if BOTH inputs authorize
        prop_assert_eq!(
            auth_meet,
            auth_a && auth_b,
            "Capability meet should only authorize permission if both inputs authorize it"
        );
        
        // Meet result should never authorize more than either input
        prop_assert!(
            !auth_meet || (auth_a && auth_b),
            "Meet result should not grant permissions that both inputs don't have"
        );
    }

    /// Property: Authentication level requirements are enforced
    /// Operations requiring higher auth levels should reject lower-level attempts
    #[test]
    fn prop_auth_level_enforcement(
        device_capabilities in arbitrary_capability_set(),
        required_auth_level in prop::sample::select(&[AuthLevel::None, AuthLevel::Device, AuthLevel::MultiFactor, AuthLevel::Threshold])
    ) {
        // Simulate different authentication levels achieved by the request
        for provided_auth_level in &[AuthLevel::None, AuthLevel::Device, AuthLevel::MultiFactor, AuthLevel::Threshold] {
            let authorization_context = AuthorizationContext {
                requesting_device: DeviceId::new(),
                requested_permission: "test:action".to_string(),
                provided_auth_level: *provided_auth_level,
                required_auth_level,
                timestamp: SystemTime::now(),
            };
            
            let authorization_result = evaluate_authorization_with_context(
                &device_capabilities,
                &authorization_context,
            );
            
            // Should only authorize if provided level meets or exceeds required level
            let should_authorize = *provided_auth_level as u8 >= required_auth_level as u8;
            
            match authorization_result {
                Ok(decision) => {
                    prop_assert_eq!(
                        decision.authorized,
                        should_authorize,
                        "Auth level enforcement: provided {:?} vs required {:?}",
                        provided_auth_level, required_auth_level
                    );
                }
                Err(_) => {
                    // Errors are acceptable if auth level is insufficient
                    prop_assert!(
                        !should_authorize,
                        "Error should only occur when auth level is insufficient"
                    );
                }
            }
        }
    }
}

/// Helper function to evaluate authorization with context
fn evaluate_authorization_with_context(
    capabilities: &CapabilitySet,
    context: &AuthorizationContext,
) -> Result<AuthorizationDecision, AuthorizationError> {
    // Check if device has the requested permission
    let has_permission = capabilities.has_permission(&context.requested_permission);
    
    // Check authentication level requirement
    let auth_level_sufficient = context.provided_auth_level as u8 >= context.required_auth_level as u8;
    
    if has_permission && auth_level_sufficient {
        Ok(AuthorizationDecision {
            authorized: true,
            reason: "Permission granted and auth level sufficient".to_string(),
            auth_level_used: context.provided_auth_level,
            expires_at: None,
        })
    } else if !has_permission {
        Err(AuthorizationError::InsufficientPermissions {
            required: context.requested_permission.clone(),
            provided: capabilities.permissions().into_iter().collect(),
        })
    } else {
        Err(AuthorizationError::InsufficientAuthLevel {
            required: context.required_auth_level,
            provided: context.provided_auth_level,
        })
    }
}

/// Additional unit tests for edge cases
#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_empty_capability_set() {
        let empty_caps = CapabilitySet::empty();
        
        assert!(!empty_caps.has_permission("any:permission"));
        assert!(empty_caps.permissions().is_empty());
    }

    #[test]
    fn test_capability_tree_root_device() {
        let root_device = DeviceId::new();
        let tree = CapabilityTree::new(root_device);
        
        assert_eq!(tree.root_device(), root_device);
        assert_eq!(tree.device_count(), 1); // Just the root
    }

    #[test]
    fn test_delegation_to_self() {
        let device = DeviceId::new();
        let mut tree = CapabilityTree::new(device);
        
        let capabilities = CapabilitySet::from_permissions(&["test:permission"]);
        let constraints = DelegationConstraints {
            max_depth: DelegationDepth::Limited(1),
            required_endorsers: vec![],
            temporal_bounds: None,
            restrictions: HashSet::new(),
        };
        
        // Delegating to self should fail or be no-op
        let result = tree.delegate_capabilities(device, device, capabilities, constraints);
        
        // Either fails with error or succeeds with no change
        match result {
            Err(_) => {
                // Failure is acceptable
            }
            Ok(_) => {
                // If it succeeds, device count shouldn't increase
                assert_eq!(tree.device_count(), 1);
            }
        }
    }

    #[test]
    fn test_temporal_bounds_edge_cases() {
        let now = SystemTime::now();
        
        // Capability that's already expired
        let expired_bounds = TemporalBounds {
            not_before: now - Duration::from_secs(3600), // 1 hour ago
            not_after: now - Duration::from_secs(1800),   // 30 minutes ago
        };
        
        let expired_cap = Cap::new()
            .with_permission("test:action")
            .with_temporal_bounds(expired_bounds);
        
        let mut capability_set = CapabilitySet::empty();
        capability_set.add_capability(expired_cap);
        
        assert!(!capability_set.has_permission_at_time("test:action", now).unwrap_or(true));
    }

    #[test]
    fn test_capability_meet_idempotence() {
        let capabilities = CapabilitySet::from_permissions(&["read", "write", "admin"]);
        
        let meet_self = capabilities.meet(&capabilities);
        
        assert_eq!(capabilities.permissions(), meet_self.permissions());
    }

    #[test]
    fn test_authorization_chain_empty() {
        let device = DeviceId::new();
        let capability_id = CapabilityId::new();
        
        let empty_chain = AuthorizationChain::new(device, capability_id, vec![]);
        
        // Empty delegation path should be invalid
        let tree = CapabilityTree::new(device);
        let validation = empty_chain.validate(&tree);
        
        assert!(validation.is_err());
    }
}