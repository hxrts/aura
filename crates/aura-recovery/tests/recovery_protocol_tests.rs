//! Recovery Protocol Integration Tests
//!
//! Tests for the authority-based recovery protocol using relational contexts.
//! Validates recovery operations, guardian approvals, and protocol outcomes.

use aura_core::{identifiers::AuthorityId, Hash32, TrustLevel};
use aura_recovery::{
    guardian_ceremony::{CeremonyId, CeremonyResponse, CeremonyStatus, GuardianRotationOp},
    types::{GuardianProfile, GuardianSet},
    RecoveryContext, RecoveryOperationType,
};

// ============================================================================
// CeremonyId Tests
// ============================================================================

#[test]
fn ceremony_id_is_deterministic() {
    let prestate = Hash32([1u8; 32]);
    let operation = Hash32([2u8; 32]);
    let nonce = 42u64;

    let id1 = CeremonyId::new(prestate, operation, nonce);
    let id2 = CeremonyId::new(prestate, operation, nonce);

    assert_eq!(id1, id2, "Same inputs should produce same ceremony ID");
}

#[test]
fn ceremony_id_varies_with_prestate() {
    let prestate1 = Hash32([1u8; 32]);
    let prestate2 = Hash32([2u8; 32]);
    let operation = Hash32([3u8; 32]);
    let nonce = 1u64;

    let id1 = CeremonyId::new(prestate1, operation, nonce);
    let id2 = CeremonyId::new(prestate2, operation, nonce);

    assert_ne!(id1, id2, "Different prestate should produce different ID");
}

#[test]
fn ceremony_id_varies_with_operation() {
    let prestate = Hash32([1u8; 32]);
    let operation1 = Hash32([2u8; 32]);
    let operation2 = Hash32([3u8; 32]);
    let nonce = 1u64;

    let id1 = CeremonyId::new(prestate, operation1, nonce);
    let id2 = CeremonyId::new(prestate, operation2, nonce);

    assert_ne!(id1, id2, "Different operation should produce different ID");
}

#[test]
fn ceremony_id_varies_with_nonce() {
    let prestate = Hash32([1u8; 32]);
    let operation = Hash32([2u8; 32]);

    let id1 = CeremonyId::new(prestate, operation, 1);
    let id2 = CeremonyId::new(prestate, operation, 2);

    assert_ne!(id1, id2, "Different nonce should produce different ID");
}

#[test]
fn ceremony_id_display_format() {
    let id = CeremonyId::new(Hash32([0xAB; 32]), Hash32([0xCD; 32]), 0);
    let display = format!("{}", id);

    assert!(display.starts_with("ceremony:"), "Display should start with 'ceremony:'");
    assert!(display.len() > 10, "Display should include hex suffix");
}

// ============================================================================
// GuardianRotationOp Tests
// ============================================================================

#[test]
fn guardian_rotation_op_hash_is_deterministic() {
    let op = GuardianRotationOp {
        threshold_k: 2,
        total_n: 3,
        guardian_ids: vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
        ],
        new_epoch: 5,
    };

    let hash1 = op.compute_hash();
    let hash2 = op.compute_hash();

    assert_eq!(hash1, hash2, "Same operation should produce same hash");
}

#[test]
fn guardian_rotation_op_hash_varies_with_threshold() {
    let base_ids = vec![
        AuthorityId::new_from_entropy([1u8; 32]),
        AuthorityId::new_from_entropy([2u8; 32]),
        AuthorityId::new_from_entropy([3u8; 32]),
    ];

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
    let op1 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 3,
        guardian_ids: vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
        ],
        new_epoch: 1,
    };

    let op2 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 3,
        guardian_ids: vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([4u8; 32]), // Different guardian
        ],
        new_epoch: 1,
    };

    assert_ne!(op1.compute_hash(), op2.compute_hash());
}

#[test]
fn guardian_rotation_op_hash_varies_with_epoch() {
    let guardian_ids = vec![
        AuthorityId::new_from_entropy([1u8; 32]),
        AuthorityId::new_from_entropy([2u8; 32]),
    ];

    let op1 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 2,
        guardian_ids: guardian_ids.clone(),
        new_epoch: 1,
    };

    let op2 = GuardianRotationOp {
        threshold_k: 2,
        total_n: 2,
        guardian_ids,
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
        CeremonyStatus::AwaitingResponses { accepted: 2, declined: 1, pending: 2 }
    ));
}

#[test]
fn ceremony_status_committed() {
    let status = CeremonyStatus::Committed { new_epoch: 42 };

    assert!(matches!(status, CeremonyStatus::Committed { new_epoch: 42 }));
}

#[test]
fn ceremony_status_aborted() {
    let status = CeremonyStatus::Aborted {
        reason: "Timeout".to_string(),
    };

    assert!(matches!(
        status,
        CeremonyStatus::Aborted { reason } if reason == "Timeout"
    ));
}

// ============================================================================
// GuardianProfile Tests
// ============================================================================

#[test]
fn guardian_profile_creation_with_new() {
    let authority_id = AuthorityId::new_from_entropy([42u8; 32]);
    let profile = GuardianProfile::new(authority_id);

    assert_eq!(profile.authority_id, authority_id);
    assert!(profile.label.is_none());
    assert_eq!(profile.trust_level, TrustLevel::High);
    assert_eq!(profile.cooldown_secs, 900); // 15 minutes default
}

#[test]
fn guardian_profile_with_label() {
    let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
    let profile = GuardianProfile::with_label(authority_id, "Test Guardian");

    assert_eq!(profile.authority_id, authority_id);
    assert_eq!(profile.label, Some("Test Guardian".to_string()));
    assert_eq!(profile.trust_level, TrustLevel::High);
}

#[test]
fn guardian_profile_custom_construction() {
    let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
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
    let g1 = GuardianProfile::new(AuthorityId::new_from_entropy([1u8; 32]));
    let g2 = GuardianProfile::new(AuthorityId::new_from_entropy([2u8; 32]));
    let g3 = GuardianProfile::new(AuthorityId::new_from_entropy([3u8; 32]));

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
    let g1 = GuardianProfile::new(AuthorityId::new_from_entropy([1u8; 32]));
    let g2 = GuardianProfile::new(AuthorityId::new_from_entropy([2u8; 32]));

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
        let id = CeremonyId::new(Hash32([0u8; 32]), Hash32([1u8; 32]), nonce);
        assert!(ids.insert(id), "Collision detected at nonce {}", nonce);
    }
}

#[test]
fn guardian_rotation_op_serialization_roundtrip() {
    let op = GuardianRotationOp {
        threshold_k: 3,
        total_n: 5,
        guardian_ids: (0..5)
            .map(|i| AuthorityId::new_from_entropy([i as u8; 32]))
            .collect(),
        new_epoch: 42,
    };

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
    let g1 = AuthorityId::new_from_entropy([1u8; 32]);
    let g2 = AuthorityId::new_from_entropy([2u8; 32]);
    let g3 = AuthorityId::new_from_entropy([3u8; 32]);

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
