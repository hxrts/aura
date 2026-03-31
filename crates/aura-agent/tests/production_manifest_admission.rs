//! Production manifest admission validation.
//!
//! Verifies that all production choreography manifests pass admission
//! requirements — capability validation, termination artifacts, etc.

#![allow(clippy::expect_used)]
#![allow(missing_docs)]

use aura_mpst::{CompositionManifest, GuardCapabilityAdmission};
use aura_protocol::admission::{protocol_admission_profile, required_artifacts};

fn production_manifests() -> Vec<CompositionManifest> {
    vec![
        aura_authentication::dkd::telltale_session_types_dkd_protocol::vm_artifacts::composition_manifest(),
        aura_authentication::guardian_auth_relational::telltale_session_types_guardian_auth_relational::vm_artifacts::composition_manifest(),
        aura_consensus::protocol::telltale_session_types_aura_consensus::vm_artifacts::composition_manifest(),
        aura_protocol::amp::choreography::telltale_session_types_amp_transport::vm_artifacts::composition_manifest(),
        aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::composition_manifest(),
        aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::composition_manifest(),
        aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::composition_manifest(),
        aura_rendezvous::protocol::exchange::telltale_session_types_rendezvous::vm_artifacts::composition_manifest(),
        aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::composition_manifest(),
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::composition_manifest(),
        aura_recovery::guardian_ceremony::telltale_session_types_guardian_ceremony::vm_artifacts::composition_manifest(),
        aura_recovery::guardian_setup::telltale_session_types_guardian_setup::vm_artifacts::composition_manifest(),
        aura_recovery::guardian_membership::telltale_session_types_guardian_membership_change::vm_artifacts::composition_manifest(),
        aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::composition_manifest(),
        aura_agent::handlers::sessions::coordination::telltale_session_types_session_coordination::vm_artifacts::composition_manifest(),
    ]
}

#[test]
fn production_manifests_have_explicit_admission_profiles() {
    for manifest in production_manifests() {
        assert!(
            protocol_admission_profile(&manifest.protocol_id).is_some(),
            "missing explicit admission profile for production manifest protocol_id={}",
            manifest.protocol_id
        );
    }
}

#[test]
fn production_manifests_preserve_core_protocol_metadata() {
    for manifest in production_manifests() {
        assert!(
            !manifest.protocol_name.is_empty(),
            "manifest protocol_name must not be empty for protocol_id={}",
            manifest.protocol_id
        );
        assert!(
            !manifest.protocol_id.is_empty(),
            "manifest protocol_id must not be empty for protocol_name={}",
            manifest.protocol_name
        );
        assert!(
            !manifest.role_names.is_empty(),
            "manifest role_names must not be empty for protocol_id={}",
            manifest.protocol_id
        );
        assert_eq!(
            manifest.protocol_qualified_name,
            CompositionManifest::qualified_name(
                manifest.protocol_namespace.as_deref(),
                &manifest.protocol_name,
            ),
            "qualified-name derivation drifted for protocol_id={}",
            manifest.protocol_id
        );
    }
}

#[test]
fn production_manifests_match_admission_capability_mapping() {
    for manifest in production_manifests() {
        let expected = required_artifacts(&manifest.protocol_id).to_vec();
        let actual = manifest
            .required_capabilities
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            actual, expected,
            "manifest capability mapping drifted for protocol_id={}",
            manifest.protocol_id
        );
    }
}

#[test]
fn production_manifests_emit_sorted_unique_guard_capabilities() {
    for manifest in production_manifests() {
        let actual = manifest
            .guard_capabilities
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>();
        let mut expected = actual.clone();
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(
            actual, expected,
            "guard capability ordering/dedup drifted for protocol_id={}",
            manifest.protocol_id
        );
    }
}

#[test]
fn production_manifests_declare_only_admitted_guard_capabilities() {
    for manifest in production_manifests() {
        manifest
            .validate_guard_capabilities(GuardCapabilityAdmission::first_party_only())
            .expect("production manifest guard capabilities must be canonical");
    }
}
