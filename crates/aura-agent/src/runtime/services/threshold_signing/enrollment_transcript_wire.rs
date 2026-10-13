//! Guarded packet dispatch for an already approved original local participant.
//! Network ingress never admits an approval or selects a private package.
use super::{
    enrollment_transcript_signing::EnrollmentTranscriptParticipantIngress,
    ValidatedLocalEnrollmentSigningMaterial,
};
use crate::runtime::services::enrollment_window::EnrollmentExecutionChild;
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

impl ValidatedLocalEnrollmentSigningMaterial {
    pub(super) fn expected_participant_verifier(
        &self,
        device: DeviceId,
    ) -> Result<Vec<u8>, AuraError> {
        original_participant_verifier(&self.ordered_devices, &self.public_package, device)
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
    pub(in crate::runtime::services) async fn dispatch_original_participant(
        &self,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
        window: &EnrollmentExecutionChild,
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
                        self.custody
                            .send_owned_enrollment_round(
                                self.effects.as_ref(),
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

pub(super) fn original_participant_verifier(
    ordered_devices: &[DeviceId],
    public_package: &[u8],
    device: DeviceId,
) -> Result<Vec<u8>, AuraError> {
    let index = ordered_devices
        .iter()
        .position(|candidate| *candidate == device)
        .and_then(|offset| u16::try_from(offset + 1).ok())
        .ok_or_else(|| {
            AuraError::permission_denied(
                "packet party is not an original retained physical participant",
            )
        })?;
    let identifier = frost_ed25519::Identifier::try_from(index).map_err(|source| {
        AuraError::crypto_with_source("retain original packet participant index", Arc::new(source))
    })?;
    let public =
        frost_ed25519::keys::PublicKeyPackage::deserialize(public_package).map_err(|source| {
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
/// Runtime admission of a finite original-coordinator negative statement.
/// This owner is distinct from the coordinator's local protected decision receipt.
pub(super) struct AdmittedRemoteNegativeTerminalRequest {
    proven: aura_protocol::transcript_round_packet::CoordinatorProvenNegativeDecision,
    approval: Arc<RuntimeApprovedEnrollmentSigningIntent>,
    retirement_location: aura_core::effects::SecureStorageLocation,
    approval_digest: [u8; 32],
}

impl AdmittedRemoteNegativeTerminalRequest {
    pub(super) fn admit(
        proven: aura_protocol::transcript_round_packet::CoordinatorProvenNegativeDecision,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: Arc<RuntimeApprovedEnrollmentSigningIntent>,
    ) -> Result<Self, AuraError> {
        Self::validate(&proven, material, &approval)?;
        let statement = &proven.attestation;
        let kind = negative_kind(statement.outcome);
        let (approval_digest, retirement_location) = negative_domain_binding(
            &approval,
            statement.decision_digest,
            kind,
            &statement.terminal_transcript,
        )?;
        Ok(Self {
            proven,
            approval,
            retirement_location,
            approval_digest,
        })
    }

    fn validate(
        proven: &aura_protocol::transcript_round_packet::CoordinatorProvenNegativeDecision,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        material.require_original(approval)?;
        let statement = &proven.attestation;
        let expected = original_participant_verifier(
            &material.ordered_devices,
            &material.public_package,
            approval.manifest().initiator_device,
        )?;
        proven.verify_exact_coordinator(&expected)?;
        let manifest = approval.manifest().transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("bind admitted negative manifest", Arc::new(source))
        })?;
        let kind = negative_kind(statement.outcome);
        approval
            .negative_terminal_policy()
            .require_manifest(approval.manifest())?;
        let message = aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalTranscript::from_manifest(approval.manifest(), kind)?
            .required_transcript_bytes().map_err(|source|
                AuraError::crypto_with_source("bind admitted negative transcript", Arc::new(source)))?;
        if statement.approved_intent_digest != approval.canonical_intent_digest()
            || statement.manifest != manifest
            || statement.coordinator != approval.manifest().initiator_device
            || statement.participant != material.device
            || statement.coordinator == material.device
            || !material.ordered_devices.contains(&statement.participant)
            || !approval
                .negative_terminal_policy()
                .outcomes()
                .contains(&kind)
            || statement.terminal_transcript != message
            || statement.session
                != binding_session(approval, aura_core::hash::hash(&message), material.device)?
        {
            return Err(AuraError::permission_denied(
                "remote negative statement differs from original approved native scope",
            ));
        }
        Ok(())
    }

    pub(super) fn require_material(
        &self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
    ) -> Result<(), AuraError> {
        Self::validate(&self.proven, material, &self.approval)
    }
    pub(super) fn message(&self) -> Result<&[u8], AuraError> {
        // The retained proof and approval are immutable; material binding is
        // revalidated by the actor before every use of this message.
        self.proven.attestation.require_shape()?;
        Ok(&self.proven.attestation.terminal_transcript)
    }
    pub(super) fn retirement_location(&self) -> &aura_core::effects::SecureStorageLocation {
        &self.retirement_location
    }
    pub(super) fn approval_digest(&self) -> [u8; 32] {
        self.approval_digest
    }
}

fn negative_kind(
    outcome: aura_protocol::transcript_round_packet::ConditionalNegativeOutcome,
) -> aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind {
    use aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind as Domain;
    use aura_protocol::transcript_round_packet::ConditionalNegativeOutcome as Wire;
    match outcome {
        Wire::Rejected => Domain::Rejected,
        Wire::Cancelled => Domain::Cancelled,
        Wire::TimedOut => Domain::TimedOut,
        Wire::ChoreographyFailed => Domain::ChoreographyFailed,
        Wire::RuntimeFailed => Domain::RuntimeFailed,
        Wire::Superseded => Domain::Superseded,
    }
}
/// Finite authenticated grammars of the original conditional lifetime.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum OriginalConditionalPacket {
    Ready(aura_protocol::transcript_round_packet::ParticipantProvenConditionalReadiness),
    Decision(aura_protocol::transcript_round_packet::CoordinatorProvenNegativeDecision),
    Round(ParticipantProvenRoundPacket),
    RetirementRequest(aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest),
    RetirementAcknowledgment(aura_protocol::transcript_round_packet::ParticipantProvenConditionalRetirementAcknowledgment),
}

/// Authenticated finite stop scope, distinct from a local durable decision.
/// Public packet decoding cannot construct this runtime admission.
pub(super) struct AdmittedConditionalRetirementRequest {
    proven: aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest,
    approval: Arc<RuntimeApprovedEnrollmentSigningIntent>,
}
impl AdmittedConditionalRetirementRequest {
    pub(super) fn admit(
        proven: aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
    ) -> Result<Self, AuraError> {
        let admitted = Self { proven, approval };
        admitted.require_material(material, child)?;
        Ok(admitted)
    }

    pub(super) fn request(
        &self,
    ) -> &aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest
    {
        &self.proven
    }

    pub(super) fn require_material(
        &self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        child: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        material.require_original(&self.approval)?;
        material.require_original_window(child)?;
        let coordinator = self.approval.manifest().initiator_device;
        let verifier = original_participant_verifier(
            &material.ordered_devices,
            &material.public_package,
            coordinator,
        )?;
        self.proven.verify_exact_coordinator(&verifier)?;
        let manifest = self
            .approval
            .manifest()
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "bind original conditional retirement manifest",
                    Arc::new(source),
                )
            })?;
        let request = &self.proven.request;
        let owner = material.conditional_transport(&self.approval, child, None)?;
        if material.device == coordinator
            || request.version != 1
            || request.approved_intent_digest != self.approval.canonical_intent_digest()
            || request.manifest_digest != aura_core::hash::hash(&manifest)
            || request.coordinator != coordinator
            || request.participant != material.device
            || request.locator != owner.receive_locator(coordinator)?
        {
            return Err(AuraError::permission_denied(
                "conditional retirement differs from original approved participant",
            ));
        }
        Ok(())
    }
}

/// Finite runtime provenance; neither variant can be decoded from a packet.
pub(super) enum AdmittedNegativeTerminalRequest {
    Local(Arc<super::OriginalNegativeTerminalRequest>),
    Remote(Arc<AdmittedRemoteNegativeTerminalRequest>),
}

impl AdmittedNegativeTerminalRequest {
    pub(super) fn require_material(
        &self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
    ) -> Result<(), AuraError> {
        match self {
            Self::Local(request) => request.require_material(material),
            Self::Remote(request) => request.require_material(material),
        }
    }
    pub(super) fn message(&self) -> Result<&[u8], AuraError> {
        match self {
            Self::Local(request) => request.message(),
            Self::Remote(request) => request.message(),
        }
    }
    pub(super) fn retirement_location(&self) -> &aura_core::effects::SecureStorageLocation {
        match self {
            Self::Local(request) => request.retirement_location(),
            Self::Remote(request) => request.retirement_location(),
        }
    }
    pub(super) fn approval_digest(&self) -> [u8; 32] {
        match self {
            Self::Local(request) => request.approval_digest(),
            Self::Remote(request) => request.approval_digest(),
        }
    }
    fn approval(&self) -> &RuntimeApprovedEnrollmentSigningIntent {
        match self {
            Self::Local(request) => &request.approval,
            Self::Remote(request) => &request.approval,
        }
    }
    pub(super) fn require_packet(
        &self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        packet: &OriginalConditionalPacket,
        destination: DeviceId,
    ) -> Result<(), AuraError> {
        self.require_material(material)?;
        let approval = self.approval();
        let digest = aura_core::hash::hash(self.message()?);
        match (self, packet) {
            (Self::Local(request), OriginalConditionalPacket::Decision(proven)) => {
                let statement = &proven.attestation;
                let expected = original_participant_verifier(
                    &material.ordered_devices,
                    &material.public_package,
                    material.device,
                )?;
                proven.verify_exact_coordinator(&expected)?;
                let manifest = approval.manifest().transcript_bytes().map_err(|source| {
                    AuraError::crypto_with_source(
                        "bind original conditional manifest",
                        Arc::new(source),
                    )
                })?;
                if material.device != approval.manifest().initiator_device
                    || destination == material.device
                    || !material.ordered_devices.contains(&destination)
                    || statement.approved_intent_digest != approval.canonical_intent_digest()
                    || statement.coordinator != material.device
                    || statement.participant != destination
                    || statement.manifest != manifest
                    || negative_kind(statement.outcome) != request.kind
                    || statement.decision_digest != request.decision_digest
                    || statement.terminal_transcript != request.message()?
                    || statement.session != binding_session(approval, digest, destination)?
                {
                    return Err(AuraError::permission_denied(
                        "conditional attestation differs from actual original decision",
                    ));
                }
            }
            (_, OriginalConditionalPacket::Round(proven)) => {
                let packet = &proven.packet;
                let coordinator = approval.manifest().initiator_device;
                let local_coordinator = matches!(self, Self::Local(_));
                let participant = if local_coordinator {
                    destination
                } else {
                    material.device
                };
                let allowed = if local_coordinator {
                    material.device == coordinator
                        && destination != coordinator
                        && matches!(
                            (&packet.body, packet.sequence),
                            (TranscriptRoundBody::RequestCommitment, 1)
                                | (TranscriptRoundBody::RequestShare(_), 3)
                        )
                } else {
                    destination == coordinator
                        && material.device != coordinator
                        && matches!(
                            (&packet.body, packet.sequence),
                            (TranscriptRoundBody::Commitment(_), 2)
                                | (TranscriptRoundBody::Share(_), 4)
                        )
                };
                if !allowed
                    || !material.ordered_devices.contains(&participant)
                    || packet.version != 1
                    || packet.approved_intent_digest != approval.canonical_intent_digest()
                    || packet.coordinator != coordinator
                    || packet.participant != participant
                    || packet.transcript_digest != digest
                    || packet.session != binding_session(approval, digest, participant)?
                {
                    return Err(AuraError::permission_denied(
                        "conditional round differs from admitted original scope",
                    ));
                }
                if let TranscriptRoundBody::RequestShare(package) = &packet.body {
                    if package.message != self.message()?
                        || package.public_key_package != material.public_package
                    {
                        return Err(AuraError::permission_denied(
                            "conditional share requests a different original transcript",
                        ));
                    }
                }
                let expected = original_participant_verifier(
                    &material.ordered_devices,
                    &material.public_package,
                    material.device,
                )?;
                proven.verify_exact_participant(&expected)?;
            }
            (_, OriginalConditionalPacket::RetirementRequest(_))
            | (_, OriginalConditionalPacket::RetirementAcknowledgment(_)) => {
                return Err(AuraError::permission_denied(
                    "negative decision cannot authorize conditional retirement transport",
                ))
            }
            (_, OriginalConditionalPacket::Ready(_)) => {
                return Err(AuraError::permission_denied(
                    "readiness emission requires actual native disposal owner",
                ))
            }
            (Self::Remote(_), OriginalConditionalPacket::Decision(_)) => {
                return Err(AuraError::permission_denied(
                    "remote participant cannot originate a coordinator decision",
                ))
            }
        }
        Ok(())
    }

    pub(super) async fn prove_decision(
        &self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        destination: DeviceId,
    ) -> Result<OriginalConditionalPacket, AuraError> {
        self.require_material(material)?;
        let Self::Local(request) = self else {
            return Err(AuraError::permission_denied(
                "remote admission cannot create local decision evidence",
            ));
        };
        let approval = self.approval();
        let message = request.message()?;
        let manifest = approval.manifest().transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode original decision manifest", Arc::new(source))
        })?;
        let attestation = aura_protocol::transcript_round_packet::NegativeDecisionAttestation {
            version: 1,
            approved_intent_digest: approval.canonical_intent_digest(),
            manifest_digest: aura_core::hash::hash(&manifest),
            manifest,
            coordinator: material.device,
            participant: destination,
            session: binding_session(approval, aura_core::hash::hash(message), destination)?,
            sequence: 0,
            outcome: negative_outcome(request.kind),
            decision_digest: request.decision_digest,
            terminal_transcript: message.to_vec(),
            transcript_digest: aura_core::hash::hash(message),
        };
        attestation.require_shape()?;
        let bytes = attestation.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "encode original negative decision attestation",
                Arc::new(source),
            )
        })?;
        let possession_proof = material
            .effects
            .sign_participant_key_proof(&bytes, &material.local_share, SigningMode::Threshold)
            .await?;
        let packet = OriginalConditionalPacket::Decision(
            aura_protocol::transcript_round_packet::CoordinatorProvenNegativeDecision {
                attestation,
                possession_proof,
            },
        );
        self.require_packet(material, &packet, destination)?;
        Ok(packet)
    }
}

pub(super) fn negative_domain_binding(
    approval: &RuntimeApprovedEnrollmentSigningIntent,
    decision_digest: [u8; 32],
    kind: aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind,
    message: &[u8],
) -> Result<([u8; 32], aura_core::effects::SecureStorageLocation), AuraError> {
    let digest = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
        "aura.enrollment.original-negative-domain.v1",
        approval.canonical_intent_digest(),
        decision_digest,
        kind,
        aura_core::hash::hash(message),
    ))?);
    let location = aura_core::effects::SecureStorageLocation::with_sub_key(
        "enrollment_signing_negative_retirement",
        approval.manifest().subject.to_string(),
        hex::encode(digest),
    );
    Ok((digest, location))
}

fn negative_outcome(
    kind: aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind,
) -> aura_protocol::transcript_round_packet::ConditionalNegativeOutcome {
    use aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind as Domain;
    use aura_protocol::transcript_round_packet::ConditionalNegativeOutcome as Wire;
    match kind {
        Domain::Rejected => Wire::Rejected,
        Domain::Cancelled => Wire::Cancelled,
        Domain::TimedOut => Wire::TimedOut,
        Domain::ChoreographyFailed => Wire::ChoreographyFailed,
        Domain::RuntimeFailed => Wire::RuntimeFailed,
        Domain::Superseded => Wire::Superseded,
    }
}
impl AdmittedNegativeTerminalRequest {
    pub(super) async fn prove_round(
        &self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        destination: DeviceId,
        packet: TranscriptRoundPacket,
    ) -> Result<OriginalConditionalPacket, AuraError> {
        self.require_material(material)?;
        let bytes = packet.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode original conditional round", Arc::new(source))
        })?;
        let possession_proof = material
            .effects
            .sign_participant_key_proof(&bytes, &material.local_share, SigningMode::Threshold)
            .await?;
        let packet = OriginalConditionalPacket::Round(ParticipantProvenRoundPacket {
            packet,
            possession_proof,
        });
        self.require_packet(material, &packet, destination)?;
        Ok(packet)
    }
    pub(super) fn require_incoming_round(
        &self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        proven: &ParticipantProvenRoundPacket,
        peer: DeviceId,
        sequence: u8,
    ) -> Result<(), AuraError> {
        self.require_material(material)?;
        let approval = self.approval();
        let coordinator = approval.manifest().initiator_device;
        let participant = if material.device == coordinator {
            peer
        } else {
            material.device
        };
        let expected = original_participant_verifier(
            &material.ordered_devices,
            &material.public_package,
            peer,
        )?;
        proven.verify_exact_participant(&expected)?;
        let packet = &proven.packet;
        let digest = aura_core::hash::hash(self.message()?);
        let allowed = if material.device == coordinator {
            peer != coordinator
                && matches!(
                    (&packet.body, sequence),
                    (TranscriptRoundBody::Commitment(_), 2) | (TranscriptRoundBody::Share(_), 4)
                )
        } else {
            peer == coordinator
                && matches!(
                    (&packet.body, sequence),
                    (TranscriptRoundBody::RequestCommitment, 1)
                        | (TranscriptRoundBody::RequestShare(_), 3)
                )
        };
        if !allowed
            || packet.version != 1
            || packet.sequence != sequence
            || packet.coordinator != coordinator
            || packet.participant != participant
            || packet.approved_intent_digest != approval.canonical_intent_digest()
            || packet.transcript_digest != digest
            || packet.session != binding_session(approval, digest, participant)?
        {
            return Err(AuraError::permission_denied(
                "received conditional round differs from original admitted phase",
            ));
        }
        if let TranscriptRoundBody::RequestShare(package) = &packet.body {
            if package.message != self.message()?
                || package.public_key_package != material.public_package
                || !package.participants.contains(&material.index)
            {
                return Err(AuraError::permission_denied(
                    "received conditional package differs from original native policy",
                ));
            }
        }
        if let TranscriptRoundBody::Commitment(commitment) = &packet.body {
            let index = material
                .ordered_devices
                .iter()
                .position(|device| *device == peer)
                .and_then(|offset| u16::try_from(offset + 1).ok())
                .ok_or_else(|| {
                    AuraError::permission_denied("conditional peer absent from original roster")
                })?;
            if commitment.participant_index != index {
                return Err(AuraError::permission_denied(
                    "conditional commitment index differs from original native roster",
                ));
            }
        }
        Ok(())
    }
}

pub(super) fn admit_original_conditional_readiness(
    proven: &aura_protocol::transcript_round_packet::ParticipantProvenConditionalReadiness,
    material: &super::OriginalNegativeTerminalSigningMaterial,
    approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
    child: &EnrollmentExecutionChild,
    peer: DeviceId,
) -> Result<(), AuraError> {
    material.require_original(approval)?;
    let expected =
        original_participant_verifier(&material.ordered_devices, &material.public_package, peer)?;
    proven.verify_exact_participant(&expected)?;
    let statement = &proven.statement;
    let manifest = approval
        .manifest()
        .required_transcript_bytes()
        .map_err(|source| {
            AuraError::crypto_with_source(
                "bind original conditional readiness manifest",
                Arc::new(source),
            )
        })?;
    let owner = material.conditional_transport(approval, child, None)?;
    if material.device != approval.manifest().initiator_device
        || peer == material.device
        || statement.version != 1
        || statement.approved_intent_digest != approval.canonical_intent_digest()
        || statement.manifest_digest != aura_core::hash::hash(&manifest)
        || statement.coordinator != material.device
        || statement.participant != peer
        || statement.locator != owner.receive_locator(peer)?
    {
        return Err(AuraError::permission_denied(
            "conditional readiness differs from original approved peer",
        ));
    }
    Ok(())
}
pub(in crate::runtime::services) enum OriginalConditionalDispatcherExit {
    NegativeSigned,
    RetirementRequested,
}

/// The original participant's conditional network and ingress progress.
#[derive(Default)]
pub(in crate::runtime::services) struct OriginalEnrollmentTranscriptDispatcher {
    terminal_failure: Option<AuraError>,
    retirement: Option<AdmittedConditionalRetirementRequest>,
    retirement_ack: Option<aura_protocol::transcript_round_packet::ParticipantProvenConditionalRetirementAcknowledgment>,
    retirement_call: Option<super::enrollment_transcript_signing::RetainedParticipantCall<()>>,
    retirement_packet: Option<OriginalConditionalPacket>,
    retirement_send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    readiness:
        Option<aura_protocol::transcript_round_packet::ParticipantProvenConditionalReadiness>,
    readiness_packet: Option<OriginalConditionalPacket>,
    readiness_send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    receive: crate::runtime::effects::OriginalEnrollmentReceiveProgress<OriginalConditionalPacket>,
    incoming: Option<OriginalConditionalPacket>,
    request: Option<Arc<AdmittedNegativeTerminalRequest>>,
    begin: Option<super::enrollment_transcript_signing::RetainedParticipantCall<()>>,
    commitment: Option<
        super::enrollment_transcript_signing::RetainedParticipantCall<
            aura_core::effects::crypto::FrostPublicCommitment,
        >,
    >,
    share: Option<super::enrollment_transcript_signing::RetainedParticipantCall<Vec<u8>>>,
    outgoing: Option<OriginalConditionalPacket>,
    send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    phase: u8,
    complete: bool,
    outcome: super::enrollment_transcript_signing::ConditionalOutcomeCustody,
}
impl OriginalEnrollmentTranscriptDispatcher {
    pub(in crate::runtime::services) async fn run_negative(
        &mut self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
        disposal: &crate::runtime::effects::AcknowledgedApprovedNativeDisposal,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<OriginalConditionalDispatcherExit, AuraError> {
        if let Some(source) = &self.terminal_failure {
            return Err(source.clone());
        }
        let result = self
            .run_negative_inner(material, approval, child, disposal, ingress)
            .await;
        if let Err(source) = &result {
            self.terminal_failure.get_or_insert_with(|| source.clone());
        }
        result
    }

    async fn run_negative_inner(
        &mut self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
        disposal: &crate::runtime::effects::AcknowledgedApprovedNativeDisposal,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<OriginalConditionalDispatcherExit, AuraError> {
        material.require_original(approval)?;
        disposal.require_original(approval)?;
        if let Some(source) = disposal.original_failure() {
            return Err(source.clone());
        }
        let coordinator = approval.manifest().initiator_device;
        if self.complete {
            let actual = child
                .execute_original_negative_terminal(material, approval, || async { Ok(()) })
                .await?;
            self.outcome.retain(actual)?;
            return Ok(OriginalConditionalDispatcherExit::NegativeSigned);
        }
        if self.readiness_packet.is_none() {
            let actual = child
                .execute_original_negative_terminal(material, approval, || async {
                    material
                        .produce_original_conditional_readiness(
                            approval,
                            child,
                            disposal,
                            &mut self.readiness,
                        )
                        .await
                })
                .await?;
            self.outcome.retain(actual)?;
            self.readiness_packet = Some(OriginalConditionalPacket::Ready(
                self.readiness
                    .take()
                    .ok_or_else(|| AuraError::invalid("actual conditional readiness missing"))?,
            ));
        }
        {
            let owner = material.readiness_transport(approval, child, disposal)?;
            let actual = material
                .effects
                .send_original_conditional_enrollment_packet(
                    &owner,
                    coordinator,
                    self.readiness_packet.as_ref().ok_or_else(|| {
                        AuraError::invalid("retained conditional readiness packet missing")
                    })?,
                    &mut self.readiness_send,
                )
                .await?;
            self.outcome.retain(actual)?;
        }
        if self.retirement.is_some() {
            return self
                .admit_retirement(material, approval, child, ingress)
                .await;
        }
        if self.request.is_none() {
            if self.incoming.is_none() {
                let owner = material.conditional_transport(approval, child, None)?;
                let actual = material
                    .effects
                    .receive_original_conditional_enrollment_packet(
                        &owner,
                        coordinator,
                        &mut self.receive,
                    )
                    .await?;
                self.outcome.retain(actual)?;
                self.incoming = self.receive.take_packet();
            }
            let actual = self
                .incoming
                .take()
                .ok_or_else(|| AuraError::invalid("original conditional decision absent"))?;
            let actual = match actual {
                OriginalConditionalPacket::RetirementRequest(proven) => {
                    self.retirement = Some(AdmittedConditionalRetirementRequest::admit(
                        proven,
                        material,
                        Arc::clone(approval),
                        child,
                    )?);
                    self.retirement_call = Some(
                        super::enrollment_transcript_signing::RetainedParticipantCall::retire(),
                    );
                    return self
                        .admit_retirement(material, approval, child, ingress)
                        .await;
                }
                actual => actual,
            };
            let OriginalConditionalPacket::Decision(proven) = actual else {
                return Err(AuraError::permission_denied(
                    "conditional decision must precede original commitment",
                ));
            };
            let admitted = Arc::new(AdmittedRemoteNegativeTerminalRequest::admit(
                proven,
                material,
                Arc::clone(approval),
            )?);
            let request = Arc::new(AdmittedNegativeTerminalRequest::Remote(admitted));
            self.begin = Some(
                super::enrollment_transcript_signing::RetainedParticipantCall::begin(Arc::clone(
                    &request,
                )),
            );
            self.request = Some(request);
            self.receive = crate::runtime::effects::OriginalEnrollmentReceiveProgress::default();
            self.phase = 1;
        }
        let request = self
            .request
            .as_ref()
            .ok_or_else(|| AuraError::invalid("retained remote decision absent"))?;
        request.require_material(material)?;
        {
            let begin = self.begin.as_mut().ok_or_else(|| {
                AuraError::invalid("original conditional ingress admission absent")
            })?;
            let actual = child
                .execute_original_negative_terminal(material, approval, || begin.run(ingress))
                .await?;
            self.outcome.retain(actual)?;
        }
        while self.phase <= 3 {
            let owner = material.conditional_transport(approval, child, Some(request))?;
            if self.incoming.is_none() {
                let actual = material
                    .effects
                    .receive_original_conditional_enrollment_packet(
                        &owner,
                        coordinator,
                        &mut self.receive,
                    )
                    .await?;
                self.outcome.retain(actual)?;
                self.incoming = self.receive.take_packet();
            }
            let Some(OriginalConditionalPacket::Round(proven)) = self.incoming.as_ref() else {
                return Err(AuraError::permission_denied(
                    "conditional decision replay or non-round after admission",
                ));
            };
            request.require_incoming_round(material, proven, coordinator, self.phase)?;
            if self.outgoing.is_none() {
                let body = match &proven.packet.body {
                    TranscriptRoundBody::RequestCommitment => {
                        let call=self.commitment.get_or_insert_with(super::enrollment_transcript_signing::RetainedParticipantCall::commitment);

                        let actual = child
                            .execute_original_negative_terminal(material, approval, || async {
                                call.run(ingress).await.map(|_| ())
                            })
                            .await?;
                        self.outcome.retain(actual)?;
                        // The call itself retains the actual returned value across helper Drop.
                        let commitment = call.retained_result()?;
                        TranscriptRoundBody::Commitment(commitment)
                    }
                    TranscriptRoundBody::RequestShare(package) => {
                        let call = self.share.get_or_insert_with(|| {
                            super::enrollment_transcript_signing::RetainedParticipantCall::share(
                                package.clone(),
                            )
                        });
                        let actual = child
                            .execute_original_negative_terminal(material, approval, || async {
                                call.run(ingress).await.map(|_| ())
                            })
                            .await?;
                        self.outcome.retain(actual)?;
                        TranscriptRoundBody::Share(call.retained_result()?)
                    }
                    _ => {
                        return Err(AuraError::permission_denied(
                            "conditional original request phase invalid",
                        ))
                    }
                };
                let packet = TranscriptRoundPacket {
                    version: 1,
                    session: proven.packet.session,
                    approved_intent_digest: approval.canonical_intent_digest(),
                    transcript_digest: proven.packet.transcript_digest,
                    coordinator,
                    participant: material.device,
                    sequence: self.phase + 1,
                    body,
                };
                let actual = child
                    .execute_original_negative_terminal(material, approval, || async {
                        self.outgoing =
                            Some(request.prove_round(material, coordinator, packet).await?);
                        Ok(())
                    })
                    .await?;
                self.outcome.retain(actual)?;
            }
            let actual = material
                .effects
                .send_original_conditional_enrollment_packet(
                    &owner,
                    coordinator,
                    self.outgoing.as_ref().ok_or_else(|| {
                        AuraError::invalid("original conditional response absent")
                    })?,
                    &mut self.send,
                )
                .await?;
            self.outcome.retain(actual)?;
            self.phase += 2;
            self.incoming = None;
            self.outgoing = None;
            self.receive = crate::runtime::effects::OriginalEnrollmentReceiveProgress::default();
            self.send = crate::runtime::effects::OriginalEnrollmentSendProgress::default();
        }
        self.complete = true;
        Ok(OriginalConditionalDispatcherExit::NegativeSigned)
    }
    async fn admit_retirement(
        &mut self,
        material: &super::OriginalNegativeTerminalSigningMaterial,
        approval: &Arc<RuntimeApprovedEnrollmentSigningIntent>,
        child: &EnrollmentExecutionChild,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<OriginalConditionalDispatcherExit, AuraError> {
        if self.request.is_some() {
            return Err(AuraError::permission_denied(
                "negative decision already admitted",
            ));
        }
        let request = self
            .retirement
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original stop absent"))?;
        request.require_material(material, child)?;
        let actual = child
            .execute_original_negative_terminal(material, approval, || async {
                if self.retirement_ack.is_none() {
                    material
                        .produce_original_conditional_retirement_ack(
                            approval,
                            child,
                            request.request(),
                            &mut self.retirement_ack,
                        )
                        .await?;
                }
                self.retirement_call
                    .as_mut()
                    .ok_or_else(|| AuraError::invalid("original stop call absent"))?
                    .admit(ingress)
                    .await
            })
            .await?;
        self.outcome.retain(actual)?;
        Ok(OriginalConditionalDispatcherExit::RetirementRequested)
    }

    pub(in crate::runtime::services) fn retirement_binding(&self) -> Result<(
        &aura_protocol::transcript_round_packet::CoordinatorProvenConditionalRetirementRequest,
        &aura_protocol::transcript_round_packet::ParticipantProvenConditionalRetirementAcknowledgment,
    ), AuraError>{
        Ok((
            self.retirement
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original stop absent"))?
                .request(),
            self.retirement_ack
                .as_ref()
                .ok_or_else(|| AuraError::invalid("private original stop acknowledgment absent"))?,
        ))
    }

    pub(in crate::runtime::services) async fn resume_retirement_reply(
        &mut self,
        retired: &super::OriginalRetiredConditionalTransportCustody,
        child: &EnrollmentExecutionChild,
        ingress: &EnrollmentTranscriptParticipantIngress,
    ) -> Result<(), AuraError> {
        let owner = retired.transport(child)?;
        let call = self
            .retirement_call
            .as_mut()
            .ok_or_else(|| AuraError::invalid("original stop call absent"))?;
        let actual = owner.execute(|| call.run(ingress)).await?;
        self.outcome.retain(actual)
    }

    pub(in crate::runtime::services) async fn send_retirement_ack(
        &mut self,
        retired: &super::OriginalRetiredConditionalTransportCustody,
        child: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        self.retirement_call
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original stop call absent"))?
            .retained_result()?;
        let coordinator = self
            .retirement
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original stop absent"))?
            .request()
            .request
            .coordinator;
        if self.retirement_packet.is_none() {
            self.retirement_packet = Some(OriginalConditionalPacket::RetirementAcknowledgment(
                self.retirement_ack
                    .take()
                    .ok_or_else(|| AuraError::invalid("private original acknowledgment absent"))?,
            ));
        }
        let owner = retired.transport(child)?;
        let actual = retired
            .effects()
            .send_original_retired_conditional_enrollment_packet(
                &owner,
                coordinator,
                self.retirement_packet
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("retained acknowledgment absent"))?,
                &mut self.retirement_send,
            )
            .await?;
        self.outcome.retain(actual)
    }
}
