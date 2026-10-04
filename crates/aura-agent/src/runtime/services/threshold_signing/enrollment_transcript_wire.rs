//! Guarded packet dispatch for an already approved original local participant.
//! Network ingress never admits an approval or selects a private package.
use super::{
    enrollment_transcript_signing::EnrollmentTranscriptParticipantIngress,
    ValidatedLocalEnrollmentSigningMaterial,
};
use crate::runtime::services::enrollment_window::EnrollmentWindowCapability;
use crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent;
use aura_core::effects::crypto::SigningMode;
use aura_core::effects::CryptoExtendedEffects;
use aura_core::{AuraError, DeviceId};
use aura_protocol::transcript_round_packet::{
    ParticipantProvenRoundPacket, ParticipantRoundSession, TranscriptRoundBody,
    TranscriptRoundPacket,
};
use aura_signature::SecurityTranscript;
use std::sync::Arc;

pub(super) fn binding_session(
    approval: &RuntimeApprovedEnrollmentSigningIntent,
    transcript_digest: [u8; 32],
    participant: DeviceId,
) -> Result<[u8; 32], AuraError> {
    Ok(aura_core::hash::hash(
        &aura_core::util::serialization::to_vec(&(
            "aura.enrollment.original-approved-round-session.v1",
            approval.manifest().subject,
            &approval.manifest().ceremony,
            &approval.manifest().invitation,
            approval.canonical_intent_digest(),
            transcript_digest,
            approval.manifest().initiator_device,
            participant,
        ))?,
    ))
}

impl ValidatedLocalEnrollmentSigningMaterial<'_, '_, '_, '_> {
    pub(super) fn expected_participant_verifier(
        &self,
        device: DeviceId,
    ) -> Result<Vec<u8>, AuraError> {
        let index = self
            .ordered_devices
            .iter()
            .position(|candidate| *candidate == device)
            .and_then(|offset| u16::try_from(offset + 1).ok())
            .ok_or_else(|| {
                AuraError::permission_denied(
                    "packet party is not an original retained physical participant",
                )
            })?;
        let identifier = frost_ed25519::Identifier::try_from(index).map_err(|source| {
            AuraError::crypto_with_source(
                "retain original packet participant index",
                Arc::new(source),
            )
        })?;
        let public = frost_ed25519::keys::PublicKeyPackage::deserialize(&self.public_package)
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "decode original packet verifying inventory",
                    Arc::new(source),
                )
            })?;
        public
            .verifying_shares()
            .get(&identifier)
            .map(|verifier| verifier.serialize().to_vec())
            .ok_or_else(|| {
                AuraError::permission_denied(
                    "original native public package excludes packet participant",
                )
            })
    }

    pub(super) async fn prove_original_round(
        &self,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
        packet: TranscriptRoundPacket,
    ) -> Result<ParticipantProvenRoundPacket, AuraError> {
        self.custody
            .require_manifest(self.effects.as_ref(), approval.manifest())?;
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || packet.version != 1
            || packet.approved_intent_digest != approval.canonical_intent_digest()
            || packet.coordinator != approval.manifest().initiator_device
            || !self.ordered_devices.contains(&packet.participant)
            || (self.device != packet.coordinator && self.device != packet.participant)
            || packet.session
                != binding_session(approval, packet.transcript_digest, packet.participant)?
        {
            return Err(AuraError::permission_denied(
                "packet proof differs from original approved native owner",
            ));
        }
        let manifest_message = approval.manifest().transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "retain approved packet manifest domain",
                Arc::new(source),
            )
        })?;
        let transport_message = approval.transport().transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "retain approved packet transport domain",
                Arc::new(source),
            )
        })?;
        let request_message = approval
            .initial_request()
            .transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "retain explicitly approved initial request domain",
                    Arc::new(source),
                )
            })?;
        if packet.transcript_digest != aura_core::hash::hash(&manifest_message)
            && packet.transcript_digest != aura_core::hash::hash(&transport_message)
            && packet.transcript_digest != aura_core::hash::hash(&request_message)
        {
            return Err(AuraError::permission_denied(
                "packet requests an unapproved transcript domain",
            ));
        }
        let message = packet.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "encode bounded original participant packet proof",
                Arc::new(source),
            )
        })?;
        let possession_proof = self
            .effects
            .sign_participant_key_proof(&message, &self.local_share, SigningMode::Threshold)
            .await?;
        Ok(ParticipantProvenRoundPacket {
            packet,
            possession_proof,
        })
    }

    /// Execute beside the original local participant future in one owned frame.
    /// No additional spawn or independent task/window owner is created.
    pub(super) async fn dispatch_original_participant(
        &self,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
        window: &EnrollmentWindowCapability,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<(), AuraError> {
        if self.device == approval.manifest().initiator_device {
            return Err(AuraError::permission_denied(
                "issuer cannot substitute for a remote participant dispatcher",
            ));
        }
        let expected = self.expected_participant_verifier(approval.manifest().initiator_device)?;
        let messages = [
            approval.manifest().transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source(
                    "admit original dispatcher manifest",
                    Arc::new(source),
                )
            })?,
            approval.transport().transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source(
                    "admit original dispatcher public transport",
                    Arc::new(source),
                )
            })?,
            approval
                .initial_request()
                .transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "admit explicitly approved initial request",
                        Arc::new(source),
                    )
                })?,
        ];
        window
            .execute(self.effects.as_ref(), || async {
                for message in messages {
                    let transcript_digest = aura_core::hash::hash(&message);
                    let session = binding_session(approval, transcript_digest, self.device)?;
                    let mut validator = ParticipantRoundSession::new(
                        session,
                        approval.canonical_intent_digest(),
                        transcript_digest,
                        approval.manifest().initiator_device,
                        self.device,
                    );
                    for (request_sequence, response_sequence) in [(1, 2), (3, 4)] {
                        let candidate = self
                            .effects
                            .receive_owned_enrollment_round(window, session)
                            .await?;
                        // The packet's source metadata does not participate in this
                        // proof. Expected key originates only in native custody.
                        let request = validator.admit(candidate, &expected)?;
                        let body =
                            match (request_sequence, request) {
                                (1, TranscriptRoundBody::RequestCommitment) => {
                                    TranscriptRoundBody::Commitment(ingress.commitment().await?)
                                }
                                (3, TranscriptRoundBody::RequestShare(package)) => {
                                    TranscriptRoundBody::Share(ingress.share(package).await?)
                                }
                                (_, TranscriptRoundBody::Cancel) => {
                                    return Err(AuraError::permission_denied(
                                        "original coordinator cancelled admitted signing",
                                    ))
                                }
                                _ => return Err(AuraError::permission_denied(
                                    "original dispatcher phase differs from admitted transcript",
                                )),
                            };
                        let response = self
                            .prove_original_round(
                                approval,
                                TranscriptRoundPacket {
                                    version: 1,
                                    session,
                                    approved_intent_digest: approval.canonical_intent_digest(),
                                    transcript_digest,
                                    coordinator: approval.manifest().initiator_device,
                                    participant: self.device,
                                    sequence: response_sequence,
                                    body,
                                },
                            )
                            .await?;
                        self.effects
                            .send_owned_enrollment_round(
                                self.custody,
                                approval.manifest(),
                                window,
                                approval.manifest().initiator_device,
                                &response,
                            )
                            .await?;
                    }
                }
                Ok(())
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "original sealed participant dispatcher",
                    Arc::new(source),
                )
            })
    }
}
