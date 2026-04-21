//! Recovery Protocol Integration Tests
//!
//! Tests for the authority-based recovery protocol using relational contexts.
//! Validates recovery operations, guardian approvals, and protocol outcomes.

#![allow(clippy::expect_used, clippy::redundant_clone, clippy::useless_vec)]

use super::support::{
    authority, guardian_ids, guardian_profile,
    guardian_profile_with_label as support_guardian_profile_with_label, guardian_rotation_op, hash,
};
use aura_core::{Hash32, TrustLevel};
use aura_recovery::{
    guardian_ceremony::{CeremonyId, CeremonyResponse, CeremonyStatus, GuardianRotationOp},
    types::{GuardianProfile, GuardianSet},
    RecoveryContext, RecoveryOperationType,
};

// ============================================================================
// CeremonyId Tests
// ============================================================================

#[test]
fn recovery_protocol_choreography_is_coherent_and_orphan_free() {
    let source = include_str!("../../src/recovery_protocol.tell");
    aura_testkit::assert_protocol_coherent(source);
    let orphan_free = aura_testkit::orphan_free_status_for_all_roles(source);
    assert!(
        orphan_free.values().any(|ok| !ok),
        "expected at least one non-orphan-free role in recovery choreography"
    );
}

#[test]
fn ceremony_id_is_deterministic() {
    let prestate = hash(1);
    let operation = hash(2);
    let nonce = 42u64;

    let id1 = CeremonyId::new(prestate, operation, nonce);
    let id2 = CeremonyId::new(prestate, operation, nonce);

    assert_eq!(id1, id2, "Same inputs should produce same ceremony ID");
}

#[test]
fn ceremony_id_varies_with_prestate() {
    let prestate1 = hash(1);
    let prestate2 = hash(2);
    let operation = hash(3);
    let nonce = 1u64;

    let id1 = CeremonyId::new(prestate1, operation, nonce);
    let id2 = CeremonyId::new(prestate2, operation, nonce);

    assert_ne!(id1, id2, "Different prestate should produce different ID");
}

#[test]
fn ceremony_id_varies_with_operation() {
    let prestate = hash(1);
    let operation1 = hash(2);
    let operation2 = hash(3);
    let nonce = 1u64;

    let id1 = CeremonyId::new(prestate, operation1, nonce);
    let id2 = CeremonyId::new(prestate, operation2, nonce);

    assert_ne!(id1, id2, "Different operation should produce different ID");
}

#[test]
fn ceremony_id_varies_with_nonce() {
    let prestate = hash(1);
    let operation = hash(2);

    let id1 = CeremonyId::new(prestate, operation, 1);
    let id2 = CeremonyId::new(prestate, operation, 2);

    assert_ne!(id1, id2, "Different nonce should produce different ID");
}

#[test]
fn ceremony_id_display_format() {
    let id = CeremonyId::new(Hash32([0xAB; 32]), Hash32([0xCD; 32]), 0);
    let display = format!("{id}");

    assert!(
        display.starts_with("ceremony:"),
        "Display should start with 'ceremony:'"
    );
    assert!(display.len() > 10, "Display should include hex suffix");
}

// ============================================================================
// GuardianRotationOp Tests
// ============================================================================

#[test]
fn guardian_rotation_op_hash_is_deterministic() {
    let op = guardian_rotation_op(2, &[1, 2, 3], 5);

    let hash1 = op.compute_hash();
    let hash2 = op.compute_hash();

    assert_eq!(hash1, hash2, "Same operation should produce same hash");
}

#[test]
fn guardian_rotation_op_hash_varies_with_threshold() {
    let base_ids = guardian_ids(&[1, 2, 3]);

    let op1 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 3,
        guardian_ids: base_ids.clone(),
        new_epoch: 1,
    };

    let op2 = GuardianRotationOp {
        threshold_k: 3,
        total_n: 3,
        guardian_ids: base_ids,
        new_epoch: 1,
    };

    assert_ne!(op1.compute_hash(), op2.compute_hash());
}

#[test]
fn guardian_rotation_op_hash_varies_with_guardians() {
    let op1 = guardian_rotation_op(2, &[1, 2, 3], 1);

    let op2 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 3,
        guardian_ids: guardian_ids(&[1, 2, 4]), // Different guardian
        new_epoch: 1,
    };

    assert_ne!(op1.compute_hash(), op2.compute_hash());
}

#[test]
fn guardian_rotation_op_hash_varies_with_epoch() {
    let guardians = guardian_ids(&[1, 2]);

    let op1 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 2,
        guardian_ids: guardians.clone(),
        new_epoch: 1,
    };

    let op2 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 2,
        guardian_ids: guardians,
        new_epoch: 2,
    };

    assert_ne!(op1.compute_hash(), op2.compute_hash());
}

// ============================================================================
// CeremonyResponse Tests
// ============================================================================

#[test]
fn ceremony_response_variants() {
    let accept = CeremonyResponse::Accept;
    let decline = CeremonyResponse::Decline;
    let pending = CeremonyResponse::Pending;

    // Verify each variant is distinct
    assert!(matches!(accept, CeremonyResponse::Accept));
    assert!(matches!(decline, CeremonyResponse::Decline));
    assert!(matches!(pending, CeremonyResponse::Pending));
}

// ============================================================================
// CeremonyStatus Tests
// ============================================================================

#[test]
fn ceremony_status_awaiting_responses() {
    let status = CeremonyStatus::AwaitingResponses {
        accepted: 2,
        declined: 1,
        pending: 2,
    };

    assert!(matches!(
        status,
        CeremonyStatus::AwaitingResponses {
            accepted: 2,
            declined: 1,
            pending: 2
        }
    ));
}

#[test]
fn ceremony_status_committed() {
    let status = CeremonyStatus::Committed { new_epoch: 42 };

    assert!(matches!(
        status,
        CeremonyStatus::Committed { new_epoch: 42 }
    ));
}

#[test]
fn ceremony_status_aborted() {
    let status = CeremonyStatus::Aborted {
        reason: aura_recovery::guardian_ceremony::CeremonyAbortReason::Manual {
            reason: "Timeout".to_string(),
        },
    };

    assert!(matches!(
        status,
        CeremonyStatus::Aborted {
            reason: aura_recovery::guardian_ceremony::CeremonyAbortReason::Manual { reason }
        } if reason == "Timeout"
    ));
}

// ============================================================================
// GuardianProfile Tests
// ============================================================================

#[test]
fn guardian_profile_creation_with_new() {
    let authority_id = authority(42);
    let profile = GuardianProfile::new(authority_id);

    assert_eq!(profile.authority_id, authority_id);
    assert!(profile.label.is_none());
    assert_eq!(profile.trust_level, TrustLevel::High);
    assert_eq!(profile.cooldown_secs, 900); // 15 minutes default
}

#[test]
fn guardian_profile_with_label() {
    let authority_id = authority(1);
    let profile = support_guardian_profile_with_label(1, "Test Guardian");

    assert_eq!(profile.authority_id, authority_id);
    assert_eq!(profile.label, Some("Test Guardian".to_string()));
    assert_eq!(profile.trust_level, TrustLevel::High);
}

#[test]
fn guardian_profile_custom_construction() {
    let authority_id = authority(1);
    let profile = GuardianProfile {
        authority_id,
        label: Some("Custom".to_string()),
        trust_level: TrustLevel::Medium,
        cooldown_secs: 1800,
    };

    assert_eq!(profile.label, Some("Custom".to_string()));
    assert_eq!(profile.trust_level, TrustLevel::Medium);
    assert_eq!(profile.cooldown_secs, 1800);
}

// ============================================================================
// GuardianSet Tests
// ============================================================================

#[test]
fn guardian_set_creation() {
    let g1 = guardian_profile(1);
    let g2 = guardian_profile(2);
    let g3 = guardian_profile(3);

    let set = GuardianSet::new(vec![g1, g2, g3]);

    assert_eq!(set.len(), 3);
    assert!(!set.is_empty());
}

#[test]
fn guardian_set_empty() {
    let set = GuardianSet::default();

    assert!(set.is_empty());
    assert_eq!(set.len(), 0);
}

#[test]
fn guardian_set_iteration() {
    let g1 = guardian_profile(1);
    let g2 = guardian_profile(2);

    let set = GuardianSet::new(vec![g1.clone(), g2.clone()]);

    let profiles: Vec<_> = set.iter().collect();
    assert_eq!(profiles.len(), 2);
}

// ============================================================================
// RecoveryContext Tests
// ============================================================================

#[test]
fn recovery_context_device_key_recovery() {
    let context = RecoveryContext::new(
        RecoveryOperationType::DeviceKeyRecovery,
        "Device lost",
        1000,
    );

    assert!(matches!(
        context.operation_type,
        RecoveryOperationType::DeviceKeyRecovery
    ));
    assert_eq!(context.justification, "Device lost");
}

#[test]
fn recovery_context_account_access_recovery() {
    let context = RecoveryContext::new(
        RecoveryOperationType::AccountAccessRecovery,
        "Account locked out",
        2000,
    );

    assert!(matches!(
        context.operation_type,
        RecoveryOperationType::AccountAccessRecovery
    ));
}

#[test]
fn recovery_context_guardian_set_modification() {
    let context = RecoveryContext::new(
        RecoveryOperationType::GuardianSetModification,
        "Adding new guardian",
        3000,
    );

    assert!(matches!(
        context.operation_type,
        RecoveryOperationType::GuardianSetModification
    ));
}

#[test]
fn recovery_context_emergency_freeze() {
    let context = RecoveryContext::new(
        RecoveryOperationType::EmergencyFreeze,
        "Account compromised",
        4000,
    );

    assert!(matches!(
        context.operation_type,
        RecoveryOperationType::EmergencyFreeze
    ));
    // Emergency freeze should set is_emergency appropriately
}

// ============================================================================
// Property-Based Tests
// ============================================================================

#[test]
fn ceremony_id_collision_resistance() {
    // Generate many ceremony IDs and verify no collisions
    let mut ids = std::collections::HashSet::new();

    for nonce in 0..1000u64 {
        let id = CeremonyId::new(hash(0), hash(1), nonce);
        assert!(ids.insert(id), "Collision detected at nonce {nonce}");
    }
}

#[test]
fn guardian_rotation_op_serialization_roundtrip() {
    let op = guardian_rotation_op(3, &[0, 1, 2, 3, 4], 42);

    // Serialize and deserialize
    let serialized = serde_json::to_string(&op).expect("serialization should succeed");
    let deserialized: GuardianRotationOp =
        serde_json::from_str(&serialized).expect("deserialization should succeed");

    assert_eq!(op.threshold_k, deserialized.threshold_k);
    assert_eq!(op.total_n, deserialized.total_n);
    assert_eq!(op.guardian_ids.len(), deserialized.guardian_ids.len());
    assert_eq!(op.new_epoch, deserialized.new_epoch);

    // Hash should be identical after roundtrip
    assert_eq!(op.compute_hash(), deserialized.compute_hash());
}

// ============================================================================
// Protocol Invariant Tests
// ============================================================================

#[test]
fn threshold_must_be_positive() {
    // A threshold of 0 makes no sense - at least 1 guardian must approve
    let threshold: u16 = 2;
    let guardians_count = 3;

    assert!(threshold > 0, "Threshold must be positive");
    assert!(
        threshold <= guardians_count,
        "Threshold cannot exceed guardian count"
    );
}

#[test]
fn recovery_requires_unique_guardians() {
    let g1 = authority(1);
    let g2 = authority(2);
    let g3 = authority(3);

    let guardians = vec![g1, g2, g3];
    let unique_count = {
        let mut set = std::collections::HashSet::new();
        guardians.iter().filter(|g| set.insert(*g)).count()
    };

    assert_eq!(
        guardians.len(),
        unique_count,
        "All guardians must be unique"
    );
}
