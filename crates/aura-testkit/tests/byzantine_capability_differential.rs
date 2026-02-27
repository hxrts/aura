//! Differential checks for Byzantine-capability admission profiles.

#![allow(clippy::expect_used)]

use aura_core::effects::{AdmissionError, CapabilityKey};
use aura_protocol::admission::{
    validate_consensus_profile_capabilities, ConsensusCapabilityProfile,
    CAPABILITY_BYZANTINE_ENVELOPE, CAPABILITY_MIXED_DETERMINISM,
};

#[test]
fn detects_silent_downgrade_of_required_consensus_capabilities() {
    let baseline_inventory = vec![
        (CapabilityKey::new(CAPABILITY_BYZANTINE_ENVELOPE), true),
        (CapabilityKey::new(CAPABILITY_MIXED_DETERMINISM), true),
    ];
    let downgraded_inventory = vec![
        (CapabilityKey::new(CAPABILITY_BYZANTINE_ENVELOPE), true),
        (CapabilityKey::new(CAPABILITY_MIXED_DETERMINISM), false),
    ];

    validate_consensus_profile_capabilities(
        ConsensusCapabilityProfile::FastPath,
        &baseline_inventory,
    )
    .expect("baseline inventory should satisfy fast-path profile");

    let err = validate_consensus_profile_capabilities(
        ConsensusCapabilityProfile::FastPath,
        &downgraded_inventory,
    )
    .expect_err("downgrade must be rejected");

    assert!(matches!(
        err,
        AdmissionError::MissingCapability { capability }
            if capability == CapabilityKey::new(CAPABILITY_MIXED_DETERMINISM)
    ));
}
