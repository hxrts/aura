//! Original coordinator's public-only proxy for one approved remote participant.
//! Native retained participant keys authenticate packets; routing metadata grants
//! neither approval nor physical participant authority.

use super::enrollment_transcript_wire::binding_session;
use super::{
    enrollment_transcript_signing::{
        remote_public_ingress, EnrollmentTranscriptParticipantIngress, ParticipantRequest,
    },
    ValidatedLocalEnrollmentSigningMaterial,
};
use crate::runtime::services::enrollment_window::EnrollmentWindowCapability;
use crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent;
use aura_core::{AuraError, DeviceId};
use aura_protocol::transcript_round_packet::{
    ParticipantProvenRoundPacket, TranscriptRoundBody, TranscriptRoundPacket,
};
use aura_signature::SecurityTranscript;
use std::{future::Future, sync::Arc};

fn reject(message: &'static str) -> AuraError {
    AuraError::permission_denied(message)
}

fn admit_original_response(
    candidate: ParticipantProvenRoundPacket,
    expected: &[u8],
    request: &TranscriptRoundPacket,
    sequence: u8,
) -> Result<TranscriptRoundBody, AuraError> {
    candidate.verify_exact_participant(expected)?;
    let packet = candidate.packet;
    if packet.version != 1
        || packet.session != request.session
        || packet.approved_intent_digest != request.approved_intent_digest
        || packet.transcript_digest != request.transcript_digest
        || packet.coordinator != request.coordinator
        || packet.participant != request.participant
        || packet.sequence != sequence
    {
        return Err(reject(
            "remote response differs from original approved native round",
        ));
    }
    match (&packet.body, sequence) {
        (TranscriptRoundBody::Commitment(_), 2) | (TranscriptRoundBody::Share(_), 4) => {
            Ok(packet.body)
        }
        _ => Err(reject(
            "remote response phase differs from original approved native round",
        )),
    }
}

impl<'tree, 'custody, 'owner, 'runtime>
    ValidatedLocalEnrollmentSigningMaterial<'tree, 'custody, 'owner, 'runtime>
{
    pub(super) fn original_remote_proxy<'borrow>(
        &'borrow self,
        approval: &'borrow RuntimeApprovedEnrollmentSigningIntent,
        window: &'borrow EnrollmentWindowCapability,
        peer: DeviceId,
    ) -> Result<
        (
            EnrollmentTranscriptParticipantIngress,
            impl Future<Output = Result<(), AuraError>>
                + 'borrow
                + use<'borrow, 'tree, 'custody, 'owner, 'runtime>,
        ),
        AuraError,
    > {
        self.custody
            .require_manifest(self.effects.as_ref(), approval.manifest())?;
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.device != approval.manifest().initiator_device
            || peer == self.device
        {
            return Err(reject(
                "public remote proxy is not the original approved coordinator",
            ));
        }
        aura_invitation::shareable::require_transport_manifest(
            approval.transport(),
            approval.manifest(),
        )
        .map_err(|source| {
            AuraError::crypto_with_source(
                "retain original proxy transport intent",
                Arc::new(source),
            )
        })?;
        let expected = self.expected_participant_verifier(peer)?;
        let index = self
            .ordered_devices
            .iter()
            .position(|candidate| *candidate == peer)
            .and_then(|offset| u16::try_from(offset + 1).ok())
            .ok_or_else(|| reject("remote proxy peer lacks original native participant index"))?;
        let messages = [
            approval.manifest().transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source("retain original proxy manifest", Arc::new(source))
            })?,
            approval.transport().transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source("retain original proxy transport", Arc::new(source))
            })?,
            approval
                .initial_request()
                .transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "retain explicitly approved proxy initial request",
                        Arc::new(source),
                    )
                })?,
        ];
        let (ingress, mut requests, terminal) = remote_public_ingress(peer, index);
        let future = async move {
            let outcome = window.execute(self.effects.as_ref(), || async {
                for message in messages {
                    let transcript_digest = aura_core::hash::hash(&message);
                    let session = binding_session(approval, transcript_digest, peer)?;
                    for phase in [1, 3] {
                        let pending = requests.recv().await.ok_or_else(|| reject("original remote proxy ingress stopped"))?;
                        match (phase, pending) {
                            (1, ParticipantRequest::Commitment(response)) => {
                                let result = async {
                                    let request = TranscriptRoundPacket {
                                        version: 1, session, approved_intent_digest: approval.canonical_intent_digest(),
                                        transcript_digest, coordinator: self.device, participant: peer,
                                        sequence: 1, body: TranscriptRoundBody::RequestCommitment,
                                    };
                                    let signed = self.prove_original_round(approval, request.clone()).await?;
                                    self.effects.send_owned_enrollment_round(self.custody, approval.manifest(), window, peer, &signed).await?;
                                    let candidate = self.effects.receive_owned_enrollment_round(window, session).await?;
                                    match admit_original_response(candidate, &expected, &request, 2)? {
                                        TranscriptRoundBody::Commitment(commitment) if commitment.participant_index == index => Ok(commitment),
                                        _ => Err(reject("remote commitment index differs from original native roster")),
                                    }
                                }.await;
                                if let Err(source) = &result {
                                    let _ = response.send(Err(source.clone()));
                                    return Err(source.clone());
                                }
                                response.send(result).map_err(|_| reject("original remote commitment receiver cancelled"))?;
                            }
                            (3, ParticipantRequest::Share(package, response)) => {
                                let result = async {
                                    if package.message != message || package.public_key_package != self.public_package
                                        || !package.participants.contains(&index) {
                                        return Err(reject("remote share package differs from original approved public transcript"));
                                    }
                                    let request = TranscriptRoundPacket {
                                        version: 1, session, approved_intent_digest: approval.canonical_intent_digest(),
                                        transcript_digest, coordinator: self.device, participant: peer,
                                        sequence: 3, body: TranscriptRoundBody::RequestShare(package),
                                    };
                                    let signed = self.prove_original_round(approval, request.clone()).await?;
                                    self.effects.send_owned_enrollment_round(self.custody, approval.manifest(), window, peer, &signed).await?;
                                    let candidate = self.effects.receive_owned_enrollment_round(window, session).await?;
                                    match admit_original_response(candidate, &expected, &request, 4)? {
                                        TranscriptRoundBody::Share(share) => Ok(share),
                                        _ => Err(reject("remote share phase differs from original retained domain")),
                                    }
                                }.await;
                                if let Err(source) = &result {
                                    let _ = response.send(Err(source.clone()));
                                    return Err(source.clone());
                                }
                                response.send(result).map_err(|_| reject("original remote share receiver cancelled"))?;
                            }
                            (_, ParticipantRequest::Commitment(response)) => {
                                let source = reject("original remote proxy commitment phase already consumed");
                                let _ = response.send(Err(source.clone()));
                                return Err(source);
                            }
                            (_, ParticipantRequest::Share(_, response)) => {
                                let source = reject("original remote proxy share arrived before commitment");
                                let _ = response.send(Err(source.clone()));
                                return Err(source);
                            }
                        }
                    }
                }
                Ok(())
            }).await.map_err(|source| AuraError::crypto_with_source("original sealed remote participant proxy", Arc::new(source)));
            if let Err(source) = &outcome {
                terminal.send_replace(Some(source.clone()));
            }
            outcome
        };
        Ok((ingress, future))
    }
}
