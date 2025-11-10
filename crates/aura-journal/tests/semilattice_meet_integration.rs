//! Integration tests for domain-specific meet semi-lattice CRDTs
//!
//! This module validates the behavior of application-specific meet CRDTs
//! in realistic usage scenarios, ensuring they satisfy both algebraic laws
//! and domain-specific requirements.

use aura_core::semilattice::{MeetSemiLattice, MvState, Top};
use aura_journal::semilattice::meet_types::*;
use proptest::prelude::*;
use std::collections::BTreeSet;

// === Test Strategies for Domain Types ===

/// Strategy for generating CapabilitySet values
fn capability_set_strategy() -> impl Strategy<Value = CapabilitySet> {
    (
        prop::collection::btree_set("[a-z]{1,5}", 0..5), // read_permissions
        prop::collection::btree_set("[a-z]{1,5}", 0..3), // write_permissions
        prop::collection::btree_set("[a-z]{1,5}", 0..2), // admin_permissions
        prop::option::of(1000u64..9999u64),              // expiry_time
        prop::option::of(10u64..1000u64),                // max_operations
    )
        .prop_map(|(read, write, admin, expiry, ops)| CapabilitySet {
            read_permissions: read,
            write_permissions: write,
            admin_permissions: admin,
            expiry_time: expiry,
            max_operations: ops,
        })
}

/// Strategy for generating TimeWindow values
fn time_window_strategy() -> impl Strategy<Value = TimeWindow> {
    (
        1000u64..9999u64,
        10000u64..99999u64,
        prop::option::of(-12i32..12i32),
    )
        .prop_map(|(start, end, tz)| TimeWindow {
            start,
            end: start + (end - start), // Ensure end > start
            timezone_offset: tz,
        })
}

/// Strategy for generating SecurityPolicy values
fn security_policy_strategy() -> impl Strategy<Value = SecurityPolicy> {
    (
        prop::collection::btree_set("(password|2fa|biometric|token)", 0..3),
        0u8..10u8,
        prop::collection::btree_set(r"\*\.[a-z]{3,8}\.com", 1..4),
        prop::option::of(300u64..7200u64),
        prop::collection::btree_set("(tpm|secure_boot|attestation)", 0..3),
    )
        .prop_map(|(auth, level, origins, duration, caps)| SecurityPolicy {
            required_auth_methods: auth,
            min_security_level: level,
            allowed_origins: origins,
            max_session_duration: duration,
            required_device_caps: caps,
        })
}

// === Property Tests for CapabilitySet ===

proptest! {
    /// Test CapabilitySet meet commutativity
    #[test]
    fn test_capability_set_commutativity(
        a in capability_set_strategy(),
        b in capability_set_strategy()
    ) {
        prop_assert_eq!(a.meet(&b), b.meet(&a));
    }

    /// Test CapabilitySet meet associativity
    #[test]
    fn test_capability_set_associativity(
        a in capability_set_strategy(),
        b in capability_set_strategy(),
        c in capability_set_strategy()
    ) {
        let left = a.meet(&b).meet(&c);
        let right = a.meet(&b.meet(&c));
        prop_assert_eq!(left, right);
    }

    /// Test CapabilitySet meet idempotence
    #[test]
    fn test_capability_set_idempotence(a in capability_set_strategy()) {
        prop_assert_eq!(a.meet(&a), a);
    }

    /// Test CapabilitySet becomes more restrictive after meet
    #[test]
    fn test_capability_set_restriction(
        a in capability_set_strategy(),
        b in capability_set_strategy()
    ) {
        let result = a.meet(&b);

        // Result should have intersection of permissions (more restrictive)
        prop_assert!(result.read_permissions.is_subset(&a.read_permissions));
        prop_assert!(result.read_permissions.is_subset(&b.read_permissions));
        prop_assert!(result.write_permissions.is_subset(&a.write_permissions));
        prop_assert!(result.write_permissions.is_subset(&b.write_permissions));
        prop_assert!(result.admin_permissions.is_subset(&a.admin_permissions));
        prop_assert!(result.admin_permissions.is_subset(&b.admin_permissions));

        // Expiry should be earlier (more restrictive)
        match (a.expiry_time, b.expiry_time, result.expiry_time) {
            (Some(a_exp), Some(b_exp), Some(r_exp)) => {
                prop_assert_eq!(r_exp, a_exp.min(b_exp));
            }
            (Some(a_exp), None, Some(r_exp)) => {
                prop_assert_eq!(r_exp, a_exp);
            }
            (None, Some(b_exp), Some(r_exp)) => {
                prop_assert_eq!(r_exp, b_exp);
            }
            (None, None, None) => {} // All good
            _ => prop_assert!(false, "Unexpected expiry time combination")
        }
    }
}

// === Property Tests for TimeWindow ===

proptest! {
    /// Test TimeWindow meet commutativity
    #[test]
    fn test_time_window_commutativity(
        a in time_window_strategy(),
        b in time_window_strategy()
    ) {
        prop_assert_eq!(a.meet(&b), b.meet(&a));
    }

    /// Test TimeWindow intersection property
    #[test]
    fn test_time_window_intersection(
        a in time_window_strategy(),
        b in time_window_strategy()
    ) {
        let result = a.meet(&b);

        // Result should be intersection: latest start, earliest end
        prop_assert_eq!(result.start, a.start.max(b.start));
        prop_assert_eq!(result.end, a.end.min(b.end));
    }

    /// Test TimeWindow validity preservation
    #[test]
    fn test_time_window_validity(
        a in time_window_strategy(),
        b in time_window_strategy()
    ) {
        let result = a.meet(&b);

        // If both inputs are valid, result validity depends on overlap
        if a.is_valid() && b.is_valid() {
            if a.overlaps(&b) {
                prop_assert!(result.is_valid(), "Overlapping windows should produce valid intersection");
            }
            // Non-overlapping windows may produce invalid intersections (start > end)
        }
    }
}

// === Property Tests for SecurityPolicy ===

proptest! {
    /// Test SecurityPolicy meet increases restrictions
    #[test]
    fn test_security_policy_restriction(
        a in security_policy_strategy(),
        b in security_policy_strategy()
    ) {
        let result = a.meet(&b);

        // Required auth methods should be union (more restrictive)
        prop_assert!(a.required_auth_methods.is_subset(&result.required_auth_methods));
        prop_assert!(b.required_auth_methods.is_subset(&result.required_auth_methods));

        // Security level should be max (more restrictive)
        prop_assert_eq!(result.min_security_level, a.min_security_level.max(b.min_security_level));

        // Allowed origins should be intersection (more restrictive)
        prop_assert!(result.allowed_origins.is_subset(&a.allowed_origins));
        prop_assert!(result.allowed_origins.is_subset(&b.allowed_origins));

        // Session duration should be minimum (more restrictive)
        match (a.max_session_duration, b.max_session_duration, result.max_session_duration) {
            (Some(a_dur), Some(b_dur), Some(r_dur)) => {
                prop_assert_eq!(r_dur, a_dur.min(b_dur));
            }
            (Some(a_dur), None, Some(r_dur)) => {
                prop_assert_eq!(r_dur, a_dur);
            }
            (None, Some(b_dur), Some(r_dur)) => {
                prop_assert_eq!(r_dur, b_dur);
            }
            (None, None, None) => {} // All good
            _ => prop_assert!(false, "Unexpected duration combination")
        }
    }
}

// === Scenario-Based Tests ===

#[test]
fn test_capability_intersection_scenario() {
    // Scenario: Device capabilities intersected with session capabilities
    // Fixed: Use exact string matches since meet does string intersection, not pattern matching
    let device_caps = CapabilitySet {
        read_permissions: ["files/docs", "files/tmp", "logs/access"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        write_permissions: ["files/tmp", "files/uploads"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        admin_permissions: BTreeSet::new(),
        expiry_time: None, // Device caps don't expire
        max_operations: None,
    };

    let session_caps = CapabilitySet {
        read_permissions: ["files/docs", "files/config"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        write_permissions: ["files/tmp"].iter().map(|s| s.to_string()).collect(),
        admin_permissions: BTreeSet::new(),
        expiry_time: Some(3600), // Session expires in 1 hour
        max_operations: Some(1000),
    };

    let effective = device_caps.meet(&session_caps);

    // Effective capabilities should be intersection (strings that appear in both sets)
    assert_eq!(effective.read_permissions.len(), 1);
    assert!(effective.read_permissions.contains("files/docs"));
    assert_eq!(effective.write_permissions.len(), 1);
    assert!(effective.write_permissions.contains("files/tmp"));
    assert_eq!(effective.expiry_time, Some(3600));
    assert_eq!(effective.max_operations, Some(1000));
}

#[test]
fn test_time_window_coordination_scenario() {
    // Scenario: Finding common availability window among team members
    let alice_availability = TimeWindow::new(900, 1700); // 9 AM - 5 PM
    let bob_availability = TimeWindow::new(1000, 1800); // 10 AM - 6 PM
    let charlie_availability = TimeWindow::new(800, 1500); // 8 AM - 3 PM

    let common_window = alice_availability
        .meet(&bob_availability)
        .meet(&charlie_availability);

    // Common window should be 10 AM - 3 PM (1000-1500)
    assert_eq!(common_window.start, 1000);
    assert_eq!(common_window.end, 1500);
    assert!(common_window.is_valid());
    assert_eq!(common_window.duration(), 500);
}

#[test]
fn test_security_policy_composition_scenario() {
    // Scenario: Combining organization policy with project policy
    let org_policy = SecurityPolicy {
        required_auth_methods: ["password"].iter().map(|s| s.to_string()).collect(),
        min_security_level: 3,
        allowed_origins: ["*.company.com"].iter().map(|s| s.to_string()).collect(),
        max_session_duration: Some(8 * 3600), // 8 hours
        required_device_caps: ["tpm"].iter().map(|s| s.to_string()).collect(),
    };

    let project_policy = SecurityPolicy {
        required_auth_methods: ["2fa"].iter().map(|s| s.to_string()).collect(),
        min_security_level: 5,
        allowed_origins: ["*.company.com", "trusted.partner.org"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        max_session_duration: Some(4 * 3600), // 4 hours
        required_device_caps: ["secure_boot"].iter().map(|s| s.to_string()).collect(),
    };

    let combined = org_policy.meet(&project_policy);

    // Combined policy should be more restrictive
    assert_eq!(combined.required_auth_methods.len(), 2); // Both password AND 2fa
    assert!(combined.required_auth_methods.contains("password"));
    assert!(combined.required_auth_methods.contains("2fa"));
    assert_eq!(combined.min_security_level, 5); // Higher requirement
    assert_eq!(combined.allowed_origins.len(), 1); // Only company.com
    assert!(combined.allowed_origins.contains("*.company.com"));
    assert_eq!(combined.max_session_duration, Some(4 * 3600)); // Shorter duration
    assert_eq!(combined.required_device_caps.len(), 2); // Both capabilities required
}

#[test]
fn test_consensus_constraint_intersection_scenario() {
    // Scenario: Participants with different consensus requirements
    let participant_1 = ConsensusConstraint {
        min_participants: 3,
        max_participants: 10,
        threshold_ratio: (2, 3), // 67%
        max_timeout: 5000,
        required_capabilities: ["sign"].iter().map(|s| s.to_string()).collect(),
    };

    let participant_2 = ConsensusConstraint {
        min_participants: 5,
        max_participants: 8,
        threshold_ratio: (3, 4), // 75%
        max_timeout: 3000,
        required_capabilities: ["verify"].iter().map(|s| s.to_string()).collect(),
    };

    let consensus = participant_1.meet(&participant_2);

    assert!(consensus.is_valid());
    assert_eq!(consensus.min_participants, 5); // Higher minimum
    assert_eq!(consensus.max_participants, 8); // Lower maximum
    assert_eq!(consensus.threshold_ratio, (3, 4)); // Higher threshold
    assert_eq!(consensus.max_timeout, 3000); // Shorter timeout
    assert_eq!(consensus.required_capabilities.len(), 2); // Both capabilities

    // Test threshold calculation
    assert_eq!(consensus.required_threshold(6), 5); // ceil(6 * 3/4) = 5
    assert_eq!(consensus.required_threshold(8), 6); // ceil(8 * 3/4) = 6
}

// === Edge Case Tests ===

#[test]
fn test_empty_capability_sets() {
    let empty1 = CapabilitySet {
        read_permissions: BTreeSet::new(),
        write_permissions: BTreeSet::new(),
        admin_permissions: BTreeSet::new(),
        expiry_time: None,
        max_operations: None,
    };

    let empty2 = CapabilitySet {
        read_permissions: BTreeSet::new(),
        write_permissions: BTreeSet::new(),
        admin_permissions: BTreeSet::new(),
        expiry_time: None,
        max_operations: None,
    };

    let result = empty1.meet(&empty2);
    assert_eq!(result, empty1);
}

#[test]
fn test_non_overlapping_time_windows() {
    let window1 = TimeWindow::new(1000, 2000);
    let window2 = TimeWindow::new(3000, 4000);

    let result = window1.meet(&window2);

    // Non-overlapping windows should produce invalid intersection
    assert!(!result.is_valid());
    assert_eq!(result.start, 3000); // max(1000, 3000)
    assert_eq!(result.end, 2000); // min(2000, 4000)
}

#[test]
fn test_contradictory_security_policies() {
    let policy1 = SecurityPolicy {
        required_auth_methods: BTreeSet::new(),
        min_security_level: 0,
        allowed_origins: ["site1.com"].iter().map(|s| s.to_string()).collect(),
        max_session_duration: None,
        required_device_caps: BTreeSet::new(),
    };

    let policy2 = SecurityPolicy {
        required_auth_methods: BTreeSet::new(),
        min_security_level: 0,
        allowed_origins: ["site2.com"].iter().map(|s| s.to_string()).collect(),
        max_session_duration: None,
        required_device_caps: BTreeSet::new(),
    };

    let combined = policy1.meet(&policy2);

    // No common origins - should result in empty allowed origins
    assert!(combined.allowed_origins.is_empty());
}

// === Top Element Tests ===

#[test]
fn test_top_element_behavior() {
    let cap_top = CapabilitySet::top();
    let time_top = TimeWindow::top();
    let security_top = SecurityPolicy::top();
    let consensus_top = ConsensusConstraint::top();

    // Top elements should be most permissive
    assert!(cap_top.read_permissions.contains("*"));
    assert!(cap_top.write_permissions.contains("*"));
    assert!(cap_top.admin_permissions.contains("*"));
    assert_eq!(cap_top.expiry_time, None);
    assert_eq!(cap_top.max_operations, None);

    assert_eq!(time_top.start, 0);
    assert_eq!(time_top.end, u64::MAX);

    assert!(security_top.required_auth_methods.is_empty());
    assert_eq!(security_top.min_security_level, 0);
    assert!(security_top.allowed_origins.contains("*"));
    assert_eq!(security_top.max_session_duration, None);

    assert_eq!(consensus_top.min_participants, 1);
    assert_eq!(consensus_top.max_participants, u32::MAX);
    assert_eq!(consensus_top.threshold_ratio, (1, 1));
    assert_eq!(consensus_top.max_timeout, u64::MAX);
}

// === Performance and Scale Tests ===

#[test]
fn test_large_capability_sets() {
    let large_cap1 = CapabilitySet {
        read_permissions: (0..1000).map(|i| format!("resource_{}", i)).collect(),
        write_permissions: (0..500).map(|i| format!("resource_{}", i)).collect(),
        admin_permissions: (0..100).map(|i| format!("resource_{}", i)).collect(),
        expiry_time: Some(9999),
        max_operations: Some(10000),
    };

    let large_cap2 = CapabilitySet {
        read_permissions: (500..1500).map(|i| format!("resource_{}", i)).collect(),
        write_permissions: (200..700).map(|i| format!("resource_{}", i)).collect(),
        admin_permissions: (50..150).map(|i| format!("resource_{}", i)).collect(),
        expiry_time: Some(8888),
        max_operations: Some(5000),
    };

    let start = std::time::Instant::now();
    let result = large_cap1.meet(&large_cap2);
    let duration = start.elapsed();

    // Should complete in reasonable time
    assert!(
        duration.as_millis() < 100,
        "Large capability meet took too long: {:?}",
        duration
    );

    // Verify correctness of large intersection
    assert_eq!(result.read_permissions.len(), 500); // intersection size
    assert_eq!(result.write_permissions.len(), 300); // intersection size
    assert_eq!(result.admin_permissions.len(), 50); // intersection size
    assert_eq!(result.expiry_time, Some(8888)); // min expiry
    assert_eq!(result.max_operations, Some(5000)); // min operations
}

// === CRDT Type Existence Tests ===

#[test]
fn test_crdt_types_exist() {
    use aura_journal::semilattice::{DeviceRegistry, IntentPool, JournalMap};

    let journal = JournalMap::new();
    assert_eq!(journal.num_ops(), 0);
    assert_eq!(journal.num_intents(), 0);

    let intent_pool = IntentPool::new();
    assert_eq!(intent_pool.len(), 0);

    let device_registry = DeviceRegistry::new();
    assert_eq!(device_registry.len(), 0);
}
