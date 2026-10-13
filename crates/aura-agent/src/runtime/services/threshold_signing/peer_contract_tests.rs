use super::{
    OriginalDeviceProposalConsent, OriginalEnrollmentPeerAdmission,
    OriginalEnrollmentPeerContinuation,
};

#[test]
fn activated_configuration_preserves_original_pending_commitment() {
    use crate::runtime::effects::ThresholdConfigMetadata;
    use aura_core::{crypto::single_signer::SigningMode, threshold::AgreementMode};
    let first = aura_core::threshold::ParticipantIdentity::device(
        aura_core::DeviceId::new_from_entropy([1; 32]),
    );
    let second = aura_core::threshold::ParticipantIdentity::device(
        aura_core::DeviceId::new_from_entropy([2; 32]),
    );
    let pending = ThresholdConfigMetadata {
        threshold_k: 2,
        total_n: 2,
        participants: vec![first, second],
        mode: SigningMode::Threshold,
        agreement_mode: AgreementMode::Provisional,
        bootstrap_migration_origin: None,
    };
    let original = pending.pending_generation_digest().unwrap();
    assert!(pending
        .require_activated_pending_generation(original)
        .is_err());
    let mut active = pending.clone();
    active.agreement_mode = AgreementMode::ConsensusFinalized;
    active
        .require_activated_pending_generation(original)
        .unwrap();
    for fault in 0..6 {
        let mut substituted = active.clone();
        match fault {
            0 => substituted.threshold_k = 1,
            1 => substituted.total_n = 3,
            2 => substituted.participants.reverse(),
            3 => substituted.mode = SigningMode::SingleSigner,
            4 => substituted.bootstrap_migration_origin = Some([3; 32]),
            _ => {
                substituted.participants.pop();
            }
        }
        assert!(substituted
            .require_activated_pending_generation(original)
            .is_err());
    }
    assert!(active
        .require_activated_pending_generation([4; 32])
        .is_err());
}

// A second applicable implementation makes inference ambiguous if a custody
// type acquires either trait. This checks the compiled declarations directly.
macro_rules! assert_move_only_custody {
    ($custody:ty) => {{
        struct CloneImplemented;
        trait AmbiguousIfClone<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfClone<()> for T {}
        impl<T: Clone> AmbiguousIfClone<CloneImplemented> for T {}
        let _ = <$custody as AmbiguousIfClone<_>>::marker;

        struct DeserializeImplemented;
        trait AmbiguousIfDeserialize<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
        impl<T: serde::de::DeserializeOwned> AmbiguousIfDeserialize<DeserializeImplemented> for T {}
        let _ = <$custody as AmbiguousIfDeserialize<_>>::marker;
    }};
}

#[test]
fn original_peer_continuation_cannot_clone_or_deserialize_custody() {
    assert_move_only_custody!(OriginalEnrollmentPeerContinuation);
    assert_move_only_custody!(super::OriginalIssuerSigningResourceRetirement);
    assert_move_only_custody!(
        crate::runtime::services::enrollment_window::OriginalIssuerSigningResourceSettlement
    );
    assert_move_only_custody!(
        crate::runtime::services::enrollment_window::OriginalIssuerResourceCleanupOwner
    );
    assert_move_only_custody!(
        crate::runtime::services::ceremony_tracker::OriginalIssuerResourceClosure
    );
    assert_move_only_custody!(OriginalEnrollmentPeerAdmission);
    assert_move_only_custody!(OriginalDeviceProposalConsent);
    assert_move_only_custody!(super::OriginalPositiveTerminalSigningMaterial);
    assert_move_only_custody!(super::positive_terminal::OriginalPositiveTerminalRequest);
    assert_move_only_custody!(
        crate::runtime::services::enrollment_window::OriginalPeerPositiveContinuationOwner
    );
    assert_move_only_custody!(
        crate::runtime::services::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt
    );
    assert_move_only_custody!(crate::runtime::effects::OriginalApprovedNativeAdmission);
    assert_move_only_custody!(
        crate::handlers::invitation::enrollment_trust::RecoveredEnrollmentRegistration<'static>
    );
}
