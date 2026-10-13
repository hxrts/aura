//! Original coordinator's public-only proxy for one approved remote participant.
//! Native retained participant keys authenticate packets; routing metadata grants
//! neither approval nor physical participant authority.

use super::enrollment_transcript_wire::{binding_session, OriginalConditionalPacket};
use super::{
    enrollment_transcript_signing::{
        remote_public_ingress, EnrollmentTranscriptParticipantIngress, ParticipantRequest,
    },
    ValidatedLocalEnrollmentSigningMaterial,
};
use crate::runtime::services::enrollment_window::EnrollmentExecutionChild;
use crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent;
use aura_core::{AuraError, DeviceId};
use aura_protocol::transcript_round_packet::{
    ParticipantProvenRoundPacket, TranscriptRoundBody, TranscriptRoundPacket,
};
use aura_signature::SecurityTranscript;
use std::sync::Arc;

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

pub(in crate::runtime::services) struct OriginalEnrollmentTranscriptProxy {
    peer: DeviceId,
    index: u16,
    receiver: tokio::sync::mpsc::Receiver<ParticipantRequest>,
    terminal_sender: tokio::sync::watch::Sender<Option<AuraError>>,
    initial_started: bool,
    initial_completed: bool,
    conditional: OriginalConditionalProxyState,
    positive: super::positive_terminal::OriginalPositiveProxy,
}

pub(in crate::runtime::services) fn original_remote_proxy_actor(
    peer: DeviceId,
    index: u16,
) -> (
    EnrollmentTranscriptParticipantIngress,
    OriginalEnrollmentTranscriptProxy,
) {
    let (ingress, receiver, terminal_sender) = remote_public_ingress(peer, index);
    (
        ingress,
        OriginalEnrollmentTranscriptProxy {
            peer,
            index,
            receiver,
            terminal_sender,
            positive: super::positive_terminal::OriginalPositiveProxy::default(),
            initial_started: false,
            initial_completed: false,
            conditional: OriginalConditionalProxyState::default(),
        },
    )
}

impl OriginalEnrollmentTranscriptProxy {
    pub(in crate::runtime::services) async fn run_initial(
        &mut self,
        material: &ValidatedLocalEnrollmentSigningMaterial,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
        window: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        if self.initial_started {
            return Err(reject(
                "original remote proxy initial phase already started",
            ));
        }
        let peer = self.peer;
        material
            .custody
            .require_manifest(material.effects.as_ref(), approval.manifest())?;
        if !Arc::ptr_eq(&material.effects, approval.effects())
            || material.device != approval.manifest().initiator_device
            || peer == material.device
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
        let expected = material.expected_participant_verifier(peer)?;
        let index = material
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
            approval
                .issuer_return_mailbox()
                .required_transcript_bytes()
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "retain approved proxy issuer return mailbox",
                        Arc::new(source),
                    )
                })?,
        ];
        if index != self.index {
            return Err(reject(
                "original remote proxy index differs from retained ingress",
            ));
        }
        self.initial_started = true;
        let outcome = window.execute(material.effects.as_ref(), || async {
                for message in messages {
                    let transcript_digest = aura_core::hash::hash(&message);
                    let session = binding_session(approval, transcript_digest, peer)?;
                    for phase in [1, 3] {
                        let pending = self.receiver.recv().await.ok_or_else(|| reject("original remote proxy ingress stopped"))?;
                        match (phase, pending) {
                            (_, ParticipantRequest::BeginPositiveTerminal(_, response))
                            | (_, ParticipantRequest::BeginNegativeTerminal(_, response))
                            | (_, ParticipantRequest::RetireConditional(response)) => {
                                let source = reject("conditional phase requires its separately admitted original transport");
                                let _ = response.send(Err(source.clone()));
                                return Err(source);
                            }

                            (1, ParticipantRequest::Commitment(response)) => {
                                let result = async {
                                    let request = TranscriptRoundPacket {
                                        version: 1, session, approved_intent_digest: approval.canonical_intent_digest(),
                                        transcript_digest, coordinator: material.device, participant: peer,
                                        sequence: 1, body: TranscriptRoundBody::RequestCommitment,
                                    };
                                    let signed = material.prove_original_round(approval, request.clone()).await?;
                                    material.custody.send_owned_enrollment_round(material.effects.as_ref(), approval.manifest(), window, peer, &signed).await?;
                                    let candidate = material.effects.receive_owned_enrollment_round(window, session).await?;
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
                                    if package.message != message || package.public_key_package != material.public_package
                                        || !package.participants.contains(&index) {
                                        return Err(reject("remote share package differs from original approved public transcript"));
                                    }
                                    let request = TranscriptRoundPacket {
                                        version: 1, session, approved_intent_digest: approval.canonical_intent_digest(),
                                        transcript_digest, coordinator: material.device, participant: peer,
                                        sequence: 3, body: TranscriptRoundBody::RequestShare(package),
                                    };
                                    let signed = material.prove_original_round(approval, request.clone()).await?;
                                    material.custody.send_owned_enrollment_round(material.effects.as_ref(), approval.manifest(), window, peer, &signed).await?;
                                    let candidate = material.effects.receive_owned_enrollment_round(window, session).await?;
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
            self.terminal_sender.send_replace(Some(source.clone()));
        } else {
            self.initial_completed = true;
        }
        outcome
    }
}
#[derive(Default)]
struct OriginalConditionalProxyState {
    request: Option<Arc<super::enrollment_transcript_wire::AdmittedNegativeTerminalRequest>>,
    retirement_started: bool,
    retirement_reply: Option<tokio::sync::oneshot::Sender<Result<(), AuraError>>>,
    retirement_proof: Option<
        aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest,
    >,
    retirement_packet: Option<super::enrollment_transcript_wire::OriginalConditionalPacket>,
    retirement_send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    retirement_ack: Option<super::enrollment_transcript_wire::OriginalConditionalPacket>,
    retirement_receive:
        crate::runtime::effects::OriginalEnrollmentReceiveProgress<OriginalConditionalPacket>,
    retirement_verified: bool,
    retirement_complete: bool,
    begin_reply: Option<tokio::sync::oneshot::Sender<Result<(), AuraError>>>,
    readiness: Option<super::enrollment_transcript_wire::OriginalConditionalPacket>,
    readiness_receive:
        crate::runtime::effects::OriginalEnrollmentReceiveProgress<OriginalConditionalPacket>,
    decision: Option<super::enrollment_transcript_wire::OriginalConditionalPacket>,
    decision_send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    pending: Option<ParticipantRequest>,
    outgoing: Option<super::enrollment_transcript_wire::OriginalConditionalPacket>,
    send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    incoming: Option<super::enrollment_transcript_wire::OriginalConditionalPacket>,
    receive: crate::runtime::effects::OriginalEnrollmentReceiveProgress<OriginalConditionalPacket>,
    phase: u8,
    complete: bool,
    outcome: super::enrollment_transcript_signing::ConditionalOutcomeCustody,
    first_failure: Option<AuraError>,
}

impl OriginalEnrollmentTranscriptProxy {
    pub(in crate::runtime::services) async fn run_negative(
        &mut self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        if let Some(source) = &self.conditional.first_failure {
            return Err(source.clone());
        }
        if self.conditional.retirement_started {
            return Err(reject("original proxy retirement already admitted"));
        }
        let result = self.run_negative_inner(material, approval, child).await;
        if let Err(source) = &result {
            self.conditional
                .first_failure
                .get_or_insert_with(|| source.clone());
            self.terminal_sender.send_replace(Some(source.clone()));
            if let Some(sender) = self.conditional.begin_reply.take() {
                let _ = sender.send(Err(source.clone()));
            }
            if let Some(pending) = self.conditional.pending.take() {
                match pending {
                    ParticipantRequest::BeginPositiveTerminal(_, sender)
                    | ParticipantRequest::BeginNegativeTerminal(_, sender)
                    | ParticipantRequest::RetireConditional(sender) => {
                        let _ = sender.send(Err(source.clone()));
                    }
                    ParticipantRequest::Commitment(sender) => {
                        let _ = sender.send(Err(source.clone()));
                    }
                    ParticipantRequest::Share(_, sender) => {
                        let _ = sender.send(Err(source.clone()));
                    }
                }
            }
        }
        result
    }

    async fn run_negative_inner(
        &mut self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        use super::enrollment_transcript_wire::{
            AdmittedNegativeTerminalRequest, OriginalConditionalPacket,
        };
        material.require_original(approval)?;
        material.require_original_window(child)?;
        if !self.initial_completed
            || material.device != approval.manifest().initiator_device
            || self.peer == material.device
        {
            return Err(reject(
                "conditional proxy lacks its original completed coordinator phase",
            ));
        }
        if self.conditional.complete {
            let actual = child
                .execute_original_negative_terminal(material, approval, || async { Ok(()) })
                .await?;
            return self.conditional.outcome.retain(actual);
        }
        if self.conditional.request.is_none() {
            if self.conditional.pending.is_none() {
                let actual = child
                    .execute_original_negative_terminal(material, approval, || async {
                        self.conditional.pending =
                            Some(self.receiver.recv().await.ok_or_else(|| {
                                reject("original conditional proxy ingress stopped")
                            })?);
                        Ok(())
                    })
                    .await?;
                self.conditional.outcome.retain(actual)?;
            }
            let pending = self
                .conditional
                .pending
                .take()
                .ok_or_else(|| reject("retained conditional proxy request absent"))?;
            match pending {
                ParticipantRequest::BeginNegativeTerminal(request, reply) => {
                    self.conditional.begin_reply = Some(reply);
                    if !matches!(request.as_ref(), AdmittedNegativeTerminalRequest::Local(_)) {
                        return Err(reject(
                            "coordinator proxy cannot originate a remote-admitted decision",
                        ));
                    }
                    request.require_material(material)?;
                    self.conditional.request = Some(request);
                    self.conditional.phase = 1;
                }
                other => {
                    self.conditional.pending = Some(other);
                    return Err(reject(
                        "conditional proxy requires actual original decision admission",
                    ));
                }
            }
        }
        let request = Arc::clone(
            self.conditional
                .request
                .as_ref()
                .ok_or_else(|| reject("original proxy decision absent"))?,
        );
        request.require_material(material)?;
        let owner = material.conditional_transport(approval, child, Some(&request))?;
        if self.conditional.readiness.is_none() {
            let actual = material
                .effects
                .receive_original_conditional_enrollment_packet(
                    &owner,
                    self.peer,
                    &mut self.conditional.readiness_receive,
                )
                .await?;
            self.conditional.outcome.retain(actual)?;
            self.conditional.readiness = self.conditional.readiness_receive.take_packet();
        }
        let Some(OriginalConditionalPacket::Ready(ready)) = self.conditional.readiness.as_ref()
        else {
            return Err(reject(
                "original peer disposal readiness must precede negative decision",
            ));
        };
        super::enrollment_transcript_wire::admit_original_conditional_readiness(
            ready, material, approval, child, self.peer,
        )?;
        if self.conditional.decision.is_none() {
            let actual = child
                .execute_original_negative_terminal(material, approval, || async {
                    self.conditional.decision =
                        Some(request.prove_decision(material, self.peer).await?);
                    Ok(())
                })
                .await?;
            self.conditional.outcome.retain(actual)?;
        }
        let actual = material
            .effects
            .send_original_conditional_enrollment_packet(
                &owner,
                self.peer,
                self.conditional
                    .decision
                    .as_ref()
                    .ok_or_else(|| reject("retained original decision proof absent"))?,
                &mut self.conditional.decision_send,
            )
            .await?;
        self.conditional.outcome.retain(actual)?;
        if let Some(reply) = self.conditional.begin_reply.take() {
            reply
                .send(Ok(()))
                .map_err(|_| reject("original proxy decision receiver cancelled"))?;
        }
        while self.conditional.phase <= 3 {
            if self.conditional.pending.is_none() {
                let actual = child
                    .execute_original_negative_terminal(material, approval, || async {
                        self.conditional.pending =
                            Some(self.receiver.recv().await.ok_or_else(|| {
                                reject("original conditional proxy ingress stopped")
                            })?);
                        Ok(())
                    })
                    .await?;
                self.conditional.outcome.retain(actual)?;
            }
            if self.conditional.outgoing.is_none() {
                let body = match (self.conditional.phase, self.conditional.pending.as_ref()) {
                    (1, Some(ParticipantRequest::Commitment(_))) => {
                        TranscriptRoundBody::RequestCommitment
                    }
                    (3, Some(ParticipantRequest::Share(package, _))) => {
                        if package.message != request.message()?
                            || package.public_key_package != material.public_package
                            || !package.participants.contains(&self.index)
                        {
                            return Err(reject("original conditional proxy package differs from approved native domain"));
                        }
                        TranscriptRoundBody::RequestShare(package.clone())
                    }
                    _ => {
                        return Err(reject(
                            "conditional proxy ingress repeated or skipped original phase",
                        ))
                    }
                };
                let digest = aura_core::hash::hash(request.message()?);
                let packet = TranscriptRoundPacket {
                    version: 1,
                    session: binding_session(approval, digest, self.peer)?,
                    approved_intent_digest: approval.canonical_intent_digest(),
                    transcript_digest: digest,
                    coordinator: material.device,
                    participant: self.peer,
                    sequence: self.conditional.phase,
                    body,
                };
                let actual = child
                    .execute_original_negative_terminal(material, approval, || async {
                        self.conditional.outgoing =
                            Some(request.prove_round(material, self.peer, packet).await?);
                        Ok(())
                    })
                    .await?;
                self.conditional.outcome.retain(actual)?;
            }
            let actual = material
                .effects
                .send_original_conditional_enrollment_packet(
                    &owner,
                    self.peer,
                    self.conditional
                        .outgoing
                        .as_ref()
                        .ok_or_else(|| reject("original proxy packet absent"))?,
                    &mut self.conditional.send,
                )
                .await?;
            self.conditional.outcome.retain(actual)?;
            if self.conditional.incoming.is_none() {
                let actual = material
                    .effects
                    .receive_original_conditional_enrollment_packet(
                        &owner,
                        self.peer,
                        &mut self.conditional.receive,
                    )
                    .await?;
                self.conditional.outcome.retain(actual)?;
                self.conditional.incoming = self.conditional.receive.take_packet();
            }
            let Some(OriginalConditionalPacket::Round(proven)) = self.conditional.incoming.as_ref()
            else {
                return Err(reject(
                    "conditional proxy received replayed decision/readiness",
                ));
            };
            request.require_incoming_round(
                material,
                proven,
                self.peer,
                self.conditional.phase + 1,
            )?;
            let response = proven.packet.body.clone();
            match (self.conditional.pending.take(), response) {
                (
                    Some(ParticipantRequest::Commitment(reply)),
                    TranscriptRoundBody::Commitment(value),
                ) => reply
                    .send(Ok(value))
                    .map_err(|_| reject("original proxy commitment receiver cancelled"))?,
                (Some(ParticipantRequest::Share(_, reply)), TranscriptRoundBody::Share(value)) => {
                    reply
                        .send(Ok(value))
                        .map_err(|_| reject("original proxy share receiver cancelled"))?;
                }
                _ => {
                    return Err(reject(
                        "original proxy response differs from retained ingress request",
                    ))
                }
            }
            self.conditional.phase += 2;
            self.conditional.incoming = None;
            self.conditional.outgoing = None;
            self.conditional.send =
                crate::runtime::effects::OriginalEnrollmentSendProgress::default();
            self.conditional.receive =
                crate::runtime::effects::OriginalEnrollmentReceiveProgress::default();
        }
        self.conditional.complete = true;
        Ok(())
    }
    pub(in crate::runtime::services) async fn run_retirement(
        &mut self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
        activation: &super::super::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> Result<(), AuraError> {
        if let Some(source) = &self.conditional.first_failure {
            return Err(source.clone());
        }
        let result = self
            .run_retirement_inner(material, approval, child, activation)
            .await;
        if let Err(source) = &result {
            self.conditional
                .first_failure
                .get_or_insert_with(|| source.clone());
            self.terminal_sender.send_replace(Some(source.clone()));
            if let Some(sender) = self.conditional.retirement_reply.take() {
                let _ = sender.send(Err(source.clone()));
            }
        }
        result
    }

    async fn run_retirement_inner(
        &mut self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
        activation: &super::super::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> Result<(), AuraError> {
        use super::enrollment_transcript_wire::OriginalConditionalPacket;
        material.require_original(approval)?;
        material.require_original_window(child)?;
        if !self.initial_completed
            || material.device != approval.manifest().initiator_device
            || self.peer == material.device
            || self.conditional.request.is_some()
            || self.conditional.begin_reply.is_some()
        {
            return Err(reject(
                "original proxy stop conflicts with participant/negative phase",
            ));
        }
        if self.conditional.retirement_complete {
            let actual = child
                .execute_original_negative_terminal(material, approval, || async { Ok(()) })
                .await?;
            return self.conditional.outcome.retain(actual);
        }
        if !self.conditional.retirement_started {
            if self.conditional.pending.is_none() {
                let actual = child
                    .execute_original_negative_terminal(material, approval, || async {
                        self.conditional.pending = Some(
                            self.receiver
                                .recv()
                                .await
                                .ok_or_else(|| reject("original stop ingress closed"))?,
                        );
                        Ok(())
                    })
                    .await?;
                self.conditional.outcome.retain(actual)?;
            }
            match self
                .conditional
                .pending
                .take()
                .ok_or_else(|| reject("original stop ingress absent"))?
            {
                ParticipantRequest::RetireConditional(sender) => {
                    self.conditional.retirement_reply = Some(sender);
                    self.conditional.retirement_started = true;
                }
                other => {
                    self.conditional.pending = Some(other);
                    return Err(reject(
                        "original proxy stop requires retained retirement ingress",
                    ));
                }
            }
        }
        let receive_owner = material.conditional_transport(approval, child, None)?;
        if self.conditional.readiness.is_none() {
            let actual = material
                .effects
                .receive_original_conditional_enrollment_packet(
                    &receive_owner,
                    self.peer,
                    &mut self.conditional.readiness_receive,
                )
                .await?;
            self.conditional.outcome.retain(actual)?;
            self.conditional.readiness = self.conditional.readiness_receive.take_packet();
        }
        let Some(OriginalConditionalPacket::Ready(ready)) = self.conditional.readiness.as_ref()
        else {
            return Err(reject(
                "original peer disposal readiness must precede retirement",
            ));
        };
        super::enrollment_transcript_wire::admit_original_conditional_readiness(
            ready, material, approval, child, self.peer,
        )?;
        if self.conditional.retirement_proof.is_none() {
            let actual = child
                .execute_original_negative_terminal(material, approval, || {
                    material.produce_original_conditional_retirement_request(
                        approval,
                        child,
                        activation,
                        self.peer,
                        &mut self.conditional.retirement_proof,
                    )
                })
                .await?;
            self.conditional.outcome.retain(actual)?;
        }
        let proof = self
            .conditional
            .retirement_proof
            .as_ref()
            .ok_or_else(|| reject("actual original stop proof absent"))?;
        let send_owner = material
            .retirement_request_transport(approval, child, activation, proof)
            .await?;
        if self.conditional.retirement_packet.is_none() {
            self.conditional.retirement_packet = Some(OriginalConditionalPacket::RetirementRequest(
            aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest::decode_bounded(
                &aura_core::util::serialization::to_vec(proof)?,
            )?,
        ));
        }
        let actual = material
            .effects
            .send_original_conditional_retirement_request(
                &send_owner,
                self.peer,
                self.conditional
                    .retirement_packet
                    .as_ref()
                    .ok_or_else(|| reject("retained original stop packet absent"))?,
                &mut self.conditional.retirement_send,
            )
            .await?;
        self.conditional.outcome.retain(actual)?;
        if self.conditional.retirement_ack.is_none() {
            let actual = material
                .effects
                .receive_original_conditional_enrollment_packet(
                    &receive_owner,
                    self.peer,
                    &mut self.conditional.retirement_receive,
                )
                .await?;
            self.conditional.outcome.retain(actual)?;
            self.conditional.retirement_ack = self.conditional.retirement_receive.take_packet();
        }
        let Some(OriginalConditionalPacket::RetirementAcknowledgment(ack)) =
            self.conditional.retirement_ack.as_ref()
        else {
            return Err(reject("original retirement acknowledgment required"));
        };
        ack.acknowledgment.require_request(&proof.request)?;
        let verifier = super::enrollment_transcript_wire::original_participant_verifier(
            &material.ordered_devices,
            &material.public_package,
            self.peer,
        )?;
        ack.verify_exact_participant(&verifier)?;
        self.conditional.retirement_verified = true;
        self.conditional
            .retirement_reply
            .take()
            .ok_or_else(|| reject("original retirement reply absent"))?
            .send(Ok(()))
            .map_err(|_| reject("original retirement reply cancelled"))?;
        self.conditional.retirement_complete = true;
        Ok(())
    }
}
impl OriginalEnrollmentTranscriptProxy {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalPositiveTerminalSigningMaterial", receiver_type = OriginalEnrollmentTranscriptProxy, family = "runtime_helper")]
    pub(in crate::runtime::services) async fn run_positive(
        &mut self,
        material: &super::OriginalPositiveTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        if !self.initial_completed {
            return Err(reject("positive proxy initial transport unfinished"));
        }
        let result = self
            .positive
            .run(
                &mut self.receiver,
                self.peer,
                self.index,
                material,
                approval,
                child,
            )
            .await;
        if let Err(source) = &result {
            self.terminal_sender.send_replace(Some(source.clone()));
        }
        result
    }
}
