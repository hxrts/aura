//! Invitation transfer contract and feature manifest binding.
//! The canonical codec and transcript implementation lives in aura-signature.

use aura_core::hash::hash;
use aura_core::invitation::InvitationType;
pub use aura_signature::invitation::*;

/// Require exact feature manifest agreement; public data does not grant native approval.
pub fn require_transport_manifest(
    intent: &PublicEnrollmentTransportSigningIntent,
    manifest: &crate::enrollment_manifest::EnrollmentTrustManifest,
) -> Result<(), ShareableInvitationError> {
    intent.validate_public_shape()?;
    let payload = intent.public_invitation();
    let transport = intent.transport_metadata();
    let InvitationType::DeviceEnrollment {
        subject_authority,
        invitee_authority,
        initiator_device_id,
        device_id,
        ceremony_id,
        pending_epoch,
        setup_binding,
        key_package,
        public_key_package,
        threshold_config,
        baseline_tree_ops,
        ..
    } = &payload.invitation_type
    else {
        return Err(ShareableInvitationError::InvalidFormat);
    };
    let baseline = aura_core::util::serialization::to_vec(baseline_tree_ops).map_err(|source| {
        ShareableInvitationError::SerializationFailed(std::sync::Arc::new(source))
    })?;
    if payload.version != ShareableInvitation::ENROLLMENT_QUORUM_VERSION
        || payload.invitation_id != manifest.invitation
        || payload.sender_id != manifest.subject
        || transport.sender_device_id != Some(manifest.initiator_device)
        || *subject_authority != manifest.subject
        || *invitee_authority != manifest.invitee_authority
        || *initiator_device_id != manifest.initiator_device
        || *device_id != manifest.invitee_device
        || *ceremony_id != manifest.ceremony
        || *pending_epoch != manifest.pending_epoch
        || setup_binding != &manifest.setup
        || key_package.as_slice() != manifest.pending_share_digest.as_slice()
        || hash(public_key_package) != manifest.pending_public_key_package_digest
        || aura_core::Hash32::from_bytes(threshold_config)
            != manifest.pending_threshold_config_digest
        || baseline_tree_ops.len() != manifest.baseline_count as usize
        || hash(&baseline) != manifest.baseline_digest
    {
        return Err(ShareableInvitationError::InvalidSenderProof);
    }
    Ok(())
}

#[cfg(test)]
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId, InvitationId};
#[cfg(test)]
use aura_signature::SecurityTranscript;

#[cfg(test)]
mod current_codec_tests {
    use super::*;

    #[test]
    fn obsolete_contact_version_is_refused_before_payload_decode() {
        assert!(matches!(
            ShareableInvitation::from_code("aura:v1:e30"),
            Err(ShareableInvitationError::UnsupportedVersion(1))
        ));
    }

    #[test]
    fn current_codec_retains_concrete_decode_sources() {
        let base64 = ShareableInvitation::from_code("aura:v2:!!!").unwrap_err();
        assert!(std::error::Error::source(&base64)
            .unwrap()
            .is::<base64::DecodeError>());
        let json = ShareableInvitation::from_code("aura:v2:e30").unwrap_err();
        assert!(std::error::Error::source(&json)
            .unwrap()
            .is::<serde_json::Error>());
    }

    #[tokio::test]
    async fn actual_signed_import_uses_the_canonical_expiry_boundary() {
        let provider = aura_effects::crypto::RealCryptoHandler::new();
        let private = aura_core::crypto::ed25519::Ed25519SigningKey::from_bytes([225; 32]);
        let public = private.verifying_key().unwrap();
        let invitation = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("signed exact local expiry"),
            sender_id: AuthorityId::new_from_entropy([226; 32]),
            context_id: None,
            invitation_type: InvitationType::Contact { nickname: None },
            expires_at: Some(2000),
            message: None,
        };
        let signature = aura_signature::sign_ed25519_transcript(
            &provider,
            &invitation.signing_transcript(),
            private.as_bytes(),
        )
        .await
        .unwrap();
        let code = invitation
            .to_signed_code(ShareableInvitationSenderProof {
                scheme: ShareableInvitation::SENDER_PROOF_SCHEME.into(),
                public_key: public.as_bytes().to_vec(),
                signature,
                sender_device_id: None,
                key_epoch: None,
            })
            .unwrap();
        for now in [1999, 2000, 2001] {
            let imported = ValidatedImportedInvitation::verify_code(
                &provider,
                &code,
                AuthorityId::new_from_entropy([227; 32]),
                ContextId::new_from_entropy([228; 32]),
                now,
            )
            .await;
            if now == 1999 {
                assert!(!imported.unwrap().invitation().is_expired(now));
            } else {
                assert!(matches!(
                    imported,
                    Err(ImportedInvitationVerificationError::Code(
                        ShareableInvitationError::Expired
                    ))
                ));
            }
        }
    }

    #[tokio::test]
    async fn shared_verifier_preserves_original_selected_provider_failure() {
        use aura_testkit::stateful_effects::custom_provider::{
            CustomCryptoProbe, CustomEd25519Fault,
        };
        let provider = CustomCryptoProbe::default();
        provider.set_ed25519_fault(Some(CustomEd25519Fault::Verify));
        let invitation = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: InvitationId::new("required source-bearing invitation verification"),
            sender_id: AuthorityId::new_from_entropy([223; 32]),
            context_id: None,
            invitation_type: InvitationType::Contact { nickname: None },
            expires_at: None,
            message: None,
        };
        let failure = aura_signature::verify_ed25519_transcript(
            &provider,
            &invitation.signing_transcript(),
            &[0; 64],
            &[0; 32],
        )
        .await
        .unwrap_err();
        assert!(matches!(
            failure,
            aura_signature::TranscriptCryptoError::Provider(_)
        ));
        let mut current: &(dyn std::error::Error + 'static) = &failure;
        while !current.is::<CustomEd25519Fault>() {
            current = current
                .source()
                .expect("original provider source must remain traversable");
        }
        assert!(matches!(
            current.downcast_ref::<CustomEd25519Fault>(),
            Some(CustomEd25519Fault::Verify)
        ));
        provider.set_ed25519_fault(Some(CustomEd25519Fault::Sign));
        let failure = aura_signature::sign_ed25519_transcript(
            &provider,
            &invitation.signing_transcript(),
            &[0; 32],
        )
        .await
        .unwrap_err();
        let aura_signature::TranscriptCryptoError::Provider(ref native) = failure else {
            panic!("invalid native signing material must retain the provider error");
        };
        assert!(matches!(native, aura_core::AuraError::Crypto { .. }));
        assert!(std::error::Error::source(&failure)
            .unwrap()
            .is::<aura_core::AuraError>());
        let mut current: &(dyn std::error::Error + 'static) = &failure;
        while !current.is::<CustomEd25519Fault>() {
            current = current
                .source()
                .expect("original signer source must remain traversable");
        }
        assert!(matches!(
            current.downcast_ref::<CustomEd25519Fault>(),
            Some(CustomEd25519Fault::Sign)
        ));
    }
}

#[cfg(test)]
mod enrollment_quorum_transport_tests {
    use super::*;
    use aura_core::effects::CryptoExtendedEffects;
    use aura_effects::crypto::RealCryptoHandler;

    fn invitation() -> ShareableInvitation {
        ShareableInvitation {
            version: ShareableInvitation::ENROLLMENT_QUORUM_VERSION,
            invitation_id: InvitationId::new("approved actual enrollment transport"),
            sender_id: AuthorityId::new_from_entropy(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.sender-authority")),
            context_id: Some(ContextId::new_from_entropy(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.context"))),
            invitation_type: InvitationType::DeviceEnrollment {
                setup_binding: crate::enrollment_setup::DeviceEnrollmentSetupBinding {
                    nonce: [203; 32],
                    digest: [204; 32],
                },
                subject_authority: AuthorityId::new_from_entropy(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.sender-authority")),
                invitee_authority: AuthorityId::new_from_entropy(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.invitee-authority")),
                initiator_device_id: DeviceId::new_from_entropy(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.initiator-device")),
                device_id: DeviceId::new_from_entropy(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.invitee-device")),
                nickname_suggestion: Some("Actual next device".into()),
                ceremony_id: aura_core::CeremonyId::new("actual transport quorum"),
                pending_epoch: 3,
                key_package: (0u8..128).collect(),
                threshold_config: vec![208; 128],
                public_key_package: vec![209; 128],
                baseline_tree_ops: vec![vec![210; 64]],
            },
            expires_at: Some(300),
            message: None,
        }
    }

    #[tokio::test]
    async fn actual_threshold_transport_signature_uses_public_commitment_and_binds_private_payload()
    {
        let first = RealCryptoHandler::for_simulation_seed(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.first-crypto-owner"));
        let second = RealCryptoHandler::for_simulation_seed(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.second-crypto-owner"));
        let keys = first.generate_signing_keys(2, 2).await.unwrap();
        assert_eq!(
            keys.mode,
            aura_core::effects::crypto::SigningMode::Threshold
        );
        let invitation = invitation();
        let transport = ShareableInvitationTransportMetadata {
            sender_hint: Some("public locator".into()),
            sender_device_id: Some(DeviceId::new_from_entropy(aura_core::hash::hash(b"aura-invitation.enrollment-quorum-transport.actual-threshold-public-commitment.initiator-device"))),
        };
        let public_intent =
            PublicEnrollmentTransportSigningIntent::from_invitation(&invitation, &transport)
                .unwrap();
        let message = public_intent.transcript_bytes().unwrap();
        assert_eq!(
            message,
            public_intent.required_transcript_bytes().unwrap(),
            "required public transport encoding preserves exact original v4 bytes"
        );
        assert_eq!(
            message,
            invitation
                .signing_transcript_with_transport(&transport)
                .transcript_bytes()
                .unwrap()
        );
        assert_eq!(
            message,
            invitation
                .signing_transcript_with_transport(&transport)
                .required_transcript_bytes()
                .unwrap(),
            "required native transport encoding preserves exact original v4 bytes"
        );
        for obsolete_version in [2, 3] {
            let mut obsolete = invitation.clone();
            obsolete.version = obsolete_version;
            assert!(
                matches!(obsolete.to_code(), Err(ShareableInvitationError::UnsupportedVersion(found)) if found == obsolete_version)
            );
            assert!(
                PublicEnrollmentTransportSigningIntent::from_invitation(&obsolete, &transport)
                    .is_err()
            );
        }
        let private_payload: Vec<u8> = (0u8..128).collect();
        assert!(!message
            .windows(private_payload.len())
            .any(|window| window == private_payload));
        let first_nonce = first
            .frost_generate_nonces(&keys.key_packages[0])
            .await
            .unwrap();
        let second_nonce = second
            .frost_generate_nonces(&keys.key_packages[1])
            .await
            .unwrap();
        let commitments = [
            first_nonce.public_commitment(),
            second_nonce.public_commitment(),
        ];
        let log = aura_core::crypto::tree_signing::ProcessFrostNonceRetirement::default();
        let first_nonce = first_nonce.retire(&log).await.unwrap();
        let second_nonce = second_nonce.retire(&log).await.unwrap();
        let package = first
            .frost_create_public_signing_package(
                &message,
                &commitments,
                &keys.public_key_package,
                2,
            )
            .await
            .unwrap();
        let first_share = first
            .frost_sign_share_for_message(
                &package,
                &keys.key_packages[0],
                first_nonce,
                &message,
                &keys.public_key_package,
                2,
            )
            .await
            .unwrap();
        let second_share = second
            .frost_sign_share_for_message(
                &package,
                &keys.key_packages[1],
                second_nonce,
                &message,
                &keys.public_key_package,
                2,
            )
            .await
            .unwrap();
        let signature = first
            .frost_aggregate_signatures(&package, &[first_share, second_share])
            .await
            .unwrap();
        let public = aura_core::crypto::tree_signing::public_key_package_from_bytes(
            &keys.public_key_package,
        )
        .unwrap()
        .group_public_key;
        assert!(aura_signature::verify_ed25519_transcript(
            &first,
            &invitation.signing_transcript_with_transport(&transport),
            &signature,
            &public
        )
        .await
        .unwrap());
        let mut changed = invitation.clone();
        if let InvitationType::DeviceEnrollment { key_package, .. } = &mut changed.invitation_type {
            key_package[0] ^= 1;
        }
        assert!(!aura_signature::verify_ed25519_transcript(
            &first,
            &changed.signing_transcript_with_transport(&transport),
            &signature,
            &public
        )
        .await
        .unwrap());
        let changed_transport = ShareableInvitationTransportMetadata {
            sender_hint: Some("substituted locator".into()),
            ..transport
        };
        assert!(!aura_signature::verify_ed25519_transcript(
            &first,
            &invitation.signing_transcript_with_transport(&changed_transport),
            &signature,
            &public
        )
        .await
        .unwrap());
    }
}
