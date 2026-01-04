//! # Ceremony Types Integration Tests
//!
//! Tests for the type-safe ceremony patterns.

use super::*;
use aura_core::identifiers::{AuthorityId, ChannelId, DeviceId};
use uuid::Uuid;

// =============================================================================
// Test Helpers
// =============================================================================

fn make_authority() -> AuthorityId {
    AuthorityId::from_uuid(Uuid::new_v4())
}

fn make_device() -> DeviceId {
    DeviceId::from_uuid(Uuid::new_v4())
}

fn make_channel_id() -> ChannelId {
    ChannelId::from_bytes([0u8; 32])
}

// =============================================================================
// Threshold Config Tests
// =============================================================================

#[test]
fn test_threshold_display() {
    let config = ThresholdConfig::new(2, 3).unwrap();
    assert_eq!(config.to_string(), "2-of-3");
}

#[test]
fn test_threshold_edge_cases() {
    // Minimum valid
    assert!(ThresholdConfig::new(1, 1).is_ok());

    // Maximum single-byte
    assert!(ThresholdConfig::new(255, 255).is_ok());

    // k == n is valid (unanimous)
    let unanimous = ThresholdConfig::new(5, 5).unwrap();
    assert!(unanimous.is_unanimous());
}

// =============================================================================
// Guardian Candidates Tests
// =============================================================================

#[test]
fn test_guardian_candidates_max_threshold() {
    let contacts: Vec<_> = (0..5).map(|_| make_authority()).collect();
    let candidates = GuardianCandidates::from_contacts(contacts).unwrap();
    assert_eq!(candidates.max_threshold_n(), 5);
}

#[test]
fn test_guardian_candidates_into_contacts() {
    let contacts = vec![make_authority(), make_authority()];
    let candidates = GuardianCandidates::from_contacts(contacts.clone()).unwrap();
    let recovered = candidates.into_contacts();
    assert_eq!(recovered.len(), 2);
}

// =============================================================================
// MFA Device Set Tests
// =============================================================================

#[test]
fn test_mfa_device_set_max_threshold() {
    let devices: Vec<_> = (0..4).map(|_| make_device()).collect();
    let mfa_set = MfaDeviceSet::from_devices(devices).unwrap();
    assert_eq!(mfa_set.max_threshold_k(), 4);
}

#[test]
fn test_mfa_device_set_into_devices() {
    let devices = vec![make_device(), make_device()];
    let mfa_set = MfaDeviceSet::from_devices(devices).unwrap();
    let recovered = mfa_set.into_devices();
    assert_eq!(recovered.len(), 2);
}

// =============================================================================
// Enrollment Context Tests
// =============================================================================

#[test]
fn test_enrollment_accessors() {
    let auth = make_authority();
    let device = make_device();
    let ctx = EnrollmentContext::new(auth, device, true).unwrap();
    assert_eq!(*ctx.authority(), auth);
    assert_eq!(*ctx.parent_device(), device);
}

// =============================================================================
// Recovery Eligible Tests
// =============================================================================

#[test]
fn test_recovery_approvals_needed() {
    let threshold = ThresholdConfig::new(3, 5).unwrap();
    let guardians: Vec<_> = (0..5).map(|_| make_authority()).collect();
    let eligible = RecoveryEligible::check(Some(threshold), &guardians).unwrap();
    assert_eq!(eligible.approvals_needed(), 3);
}

// =============================================================================
// Channel Participants Tests
// =============================================================================

#[test]
fn test_channel_participants_into() {
    let p = ChannelParticipants::pairwise(make_authority(), make_authority());
    let participants = p.into_participants();
    assert_eq!(participants.len(), 2);
}

// =============================================================================
// Invitation Config Tests
// =============================================================================

#[test]
fn test_invitation_from_authority() {
    let auth = make_authority();
    let inv = InvitationConfig::contact(auth);
    assert_eq!(*inv.from_authority(), auth);
}

#[test]
fn test_channel_invitation_with_role() {
    let inv = InvitationConfig::channel(
        make_authority(),
        make_channel_id(),
        invitation::ChannelRole::Admin,
    );
    if let InvitationConfig::Channel { role, .. } = inv {
        assert_eq!(role, invitation::ChannelRole::Admin);
    } else {
        panic!("Expected channel invitation");
    }
}

// =============================================================================
// Integration: Combining Types
// =============================================================================

#[test]
fn test_guardian_setup_flow() {
    // Step 1: Check we have contacts
    let contacts: Vec<_> = (0..3).map(|_| make_authority()).collect();
    let candidates = GuardianCandidates::from_contacts(contacts).unwrap();

    // Step 2: Configure threshold
    let threshold = ThresholdConfig::new(2, 3).unwrap();

    // Step 3: Validate candidates support threshold
    candidates.validate_for_threshold(threshold.n()).unwrap();

    // Step 4: Check recovery would be possible
    let eligible = RecoveryEligible::check(Some(threshold), candidates.contacts()).unwrap();
    assert_eq!(eligible.approvals_needed(), 2);
}

#[test]
fn test_mfa_setup_flow() {
    // Step 1: Check we have enough devices
    let devices: Vec<_> = (0..3).map(|_| make_device()).collect();
    let mfa_set = MfaDeviceSet::from_devices(devices).unwrap();

    // Step 2: Get recommended threshold
    let k = mfa_set.recommended_threshold();
    let n = mfa_set.count() as u8;

    // Step 3: Create threshold config
    let threshold = ThresholdConfig::new(k, n).unwrap();
    assert!(threshold.is_majority());
}
