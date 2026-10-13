//! Original protected-decision authority for the finite committed control.
use super::*;
use aura_signature::SecurityTranscript;

pub(super) struct OriginalPositiveTerminalRequest {
    approval: Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    issued:
        std::sync::Weak<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
    original: std::sync::Weak<super::super::ceremony_tracker::RegisteredEnrollmentWindowCapability>,
    decision_digest: [u8; 32],
    transcript: aura_invitation::enrollment_signing_intent::EnrollmentPositiveTerminalTranscript,
}
impl OriginalPositiveTerminalRequest {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalEnrollmentPositiveDecisionReceipt",
        family = "runtime_helper"
    )]
    pub(super) fn from_original_decision(
        approval: Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        issued: &Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        decision: &super::super::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt,
    ) -> Result<Self, AuraError> {
        decision.require_original_control(issued)?;
        child.require_issued_notice_owner(issued)?;
        let original = decision.original_owner_for_control(issued)?;
        original
            .upgrade()
            .ok_or_else(|| AuraError::permission_denied("original positive owner lost"))?
            .require_approved_manifest(&approval)?;
        let transcript = aura_invitation::enrollment_signing_intent::EnrollmentPositiveTerminalTranscript::from_manifest(
            approval.manifest(), decision.committed_ops(),
        )?;
        Ok(Self {
            approval,
            issued: Arc::downgrade(issued),
            original,
            decision_digest: decision.decision_digest(),
            transcript,
        })
    }
    pub(super) fn require_original_decision(&self) -> Result<(), AuraError> {
        let issued = self
            .issued
            .upgrade()
            .ok_or_else(|| AuraError::permission_denied("original positive control lost"))?;
        let original = self
            .original
            .upgrade()
            .ok_or_else(|| AuraError::permission_denied("original positive owner lost"))?;
        original.require_approved_manifest(&self.approval)?;
        let decision = original.original_positive_decision(&issued)?;
        decision.require_original_control(&issued)?;
        let actual = aura_invitation::enrollment_signing_intent::EnrollmentPositiveTerminalTranscript::from_manifest(self.approval.manifest(), decision.committed_ops())?;
        if decision.decision_digest() != self.decision_digest
            || actual.required_transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source("encode original committed control", Arc::new(source))
            })? != self.message()?
        {
            return Err(AuraError::permission_denied(
                "original positive decision changed",
            ));
        }
        Ok(())
    }
    pub(super) fn message(&self) -> Result<Vec<u8>, AuraError> {
        self.transcript
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source("encode finite committed control", Arc::new(source))
            })
    }
    pub(super) fn require_material(
        &self,
        material: &OriginalPositiveTerminalSigningMaterial,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        self.require_original_decision()?;
        material.require_original(&self.approval, child)?;
        match &material.origin {
            OriginalPositiveTerminalOrigin::Issuer {
                original,
                decision_digest,
            } if std::sync::Weak::ptr_eq(original, &self.original)
                && *decision_digest == self.decision_digest =>
            {
                Ok(())
            }
            _ => Err(AuraError::permission_denied(
                "positive request differs from actual original signing owner",
            )),
        }
    }
}

pub(super) enum AdmittedPositiveTerminalRequest {
    Local(Arc<OriginalPositiveTerminalRequest>),
    Remote {
        approval:
            Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
        proven: Box<aura_protocol::transcript_round_packet::CoordinatorProvenPositiveDecision>,
    },
}
impl AdmittedPositiveTerminalRequest {
    pub(super) fn approval(
        &self,
    ) -> &Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>
    {
        match self {
            Self::Local(actual) => &actual.approval,
            Self::Remote { approval, .. } => approval,
        }
    }
    pub(super) fn message(&self) -> Result<Vec<u8>, AuraError> {
        match self {
            Self::Local(actual) => actual.message(),
            Self::Remote { proven, .. } => Ok(proven.attestation.terminal_transcript.clone()),
        }
    }
    pub(super) fn require_material(
        &self,
        material: &OriginalPositiveTerminalSigningMaterial,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        match self {
            Self::Local(actual) => actual.require_material(material, child),
            Self::Remote { approval, proven } => {
                material.require_original(approval, child)?;
                let OriginalPositiveTerminalOrigin::Peer(actual) = &material.origin else {
                    return Err(AuraError::permission_denied(
                        "remote positive decision requires actual peer activation",
                    ));
                };
                let manifest = approval.manifest();
                let verifier = super::enrollment_transcript_wire::original_participant_verifier(
                    &material.ordered_devices,
                    &material.public_package,
                    manifest.initiator_device,
                )?;
                proven.verify_exact_coordinator(&verifier)?;
                let canonical=aura_invitation::enrollment_signing_intent::EnrollmentPositiveTerminalTranscript::from_manifest(manifest,&[actual.commit.attested_leaf_op.clone().ok_or_else(||AuraError::invalid("actual committed leaf absent"))?,actual.commit.attested_epoch_op.clone().ok_or_else(||AuraError::invalid("actual committed fence absent"))?])?;
                let message = canonical.required_transcript_bytes().map_err(|source| {
                    AuraError::crypto_with_source(
                        "bind actual peer positive control",
                        Arc::new(source),
                    )
                })?;
                let statement = &proven.attestation;
                let manifest_bytes = manifest.required_transcript_bytes().map_err(|source| {
                    AuraError::crypto_with_source("bind actual peer manifest", Arc::new(source))
                })?;
                if statement.approved_intent_digest != approval.canonical_intent_digest()
                    || statement.manifest != manifest_bytes
                    || statement.coordinator != manifest.initiator_device
                    || statement.participant != material.device
                    || statement.terminal_transcript != message
                    || statement.session
                        != super::enrollment_transcript_wire::binding_session(
                            approval,
                            aura_core::hash::hash(&message),
                            material.device,
                        )?
                {
                    return Err(AuraError::permission_denied(
                        "positive decision differs from actual peer committed transition",
                    ));
                }
                Ok(())
            }
        }
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalPositiveTerminalSigningMaterial",
        family = "runtime_helper"
    )]
    pub(super) fn admit(
        proven: aura_protocol::transcript_round_packet::CoordinatorProvenPositiveDecision,
        material: &OriginalPositiveTerminalSigningMaterial,
        approval: Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<Self, AuraError> {
        let actual = Self::Remote {
            approval,
            proven: Box::new(proven),
        };
        actual.require_material(material, child)?;
        Ok(actual)
    }
    pub(super) fn retirement_binding(
        &self,
    ) -> Result<([u8; 32], aura_core::effects::SecureStorageLocation), AuraError> {
        let digest = match self {
            Self::Local(actual) => actual.decision_digest,
            Self::Remote { proven, .. } => proven.attestation.decision_digest,
        };
        let binding = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
            "aura.enrollment.original-positive-nonce.v1",
            self.approval().canonical_intent_digest(),
            digest,
            self.message()?,
        ))?);
        Ok((
            binding,
            aura_core::effects::SecureStorageLocation::new(
                "device_enrollment_positive_nonce_v1",
                hex::encode(binding),
            ),
        ))
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum OriginalPositivePacket {
    Decision(aura_protocol::transcript_round_packet::CoordinatorProvenPositiveDecision),
    Round(aura_protocol::transcript_round_packet::ParticipantProvenRoundPacket),
}

pub(crate) struct OriginalPositiveTransport<'a> {
    material: &'a OriginalPositiveTerminalSigningMaterial,
    approval:
        &'a Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    child: &'a super::super::enrollment_window::EnrollmentExecutionChild,
    request: Option<&'a AdmittedPositiveTerminalRequest>,
}
impl OriginalPositiveTerminalSigningMaterial {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalPositiveTerminalSigningMaterial", receiver_type = OriginalPositiveTerminalSigningMaterial, family = "runtime_helper")]
    pub(super) fn positive_transport<'a>(
        &'a self,
        approval: &'a Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &'a super::super::enrollment_window::EnrollmentExecutionChild,
        request: Option<&'a AdmittedPositiveTerminalRequest>,
    ) -> Result<OriginalPositiveTransport<'a>, AuraError> {
        self.require_original(approval, child)?;
        if let Some(actual) = request {
            actual.require_material(self, child)?;
        }
        Ok(OriginalPositiveTransport {
            material: self,
            approval,
            child,
            request,
        })
    }
}
impl OriginalPositiveTransport<'_> {
    pub(crate) fn original_effects_origin(&self) -> std::sync::Weak<AuraEffectSystem> {
        Arc::downgrade(&self.material.effects)
    }
    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        if !std::ptr::eq(effects, self.material.effects.as_ref()) {
            return Err(AuraError::permission_denied(
                "positive transport physical runtime differs",
            ));
        }
        self.material.require_original(self.approval, self.child)
    }
    pub(crate) fn receive_locator(&self, peer: DeviceId) -> Result<[u8; 32], AuraError> {
        self.require_effects(self.material.effects.as_ref())?;
        let coordinator = self.approval.manifest().initiator_device;
        if peer == self.material.device
            || !self.material.ordered_devices.contains(&peer)
            || (self.material.device != coordinator && peer != coordinator)
        {
            return Err(AuraError::permission_denied(
                "positive transport route differs from native roster",
            ));
        }
        let participant = if self.material.device == coordinator {
            peer
        } else {
            self.material.device
        };
        Ok(aura_core::hash::hash(
            &aura_core::util::serialization::to_vec(&(
                "aura.enrollment.original-positive-transport.v1",
                self.approval.canonical_intent_digest(),
                self.approval.manifest().subject,
                &self.approval.manifest().ceremony,
                coordinator,
                participant,
            ))?,
        ))
    }
    pub(crate) fn decode_received(
        &self,
        payload: &[u8],
    ) -> Result<OriginalPositivePacket, AuraError> {
        self.require_effects(self.material.effects.as_ref())?;
        if payload.len() > 128 * 1024 {
            return Err(AuraError::invalid("positive packet exceeds bound"));
        }
        let packet: OriginalPositivePacket = aura_core::util::serialization::from_slice(payload)?;
        if aura_core::util::serialization::to_vec(&packet)? != payload {
            return Err(AuraError::invalid("positive packet noncanonical"));
        }
        Ok(packet)
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalPositiveTerminalSigningMaterial", receiver_type = OriginalPositiveTransport, family = "runtime_helper")]
    pub(crate) async fn execute<T, F, Fut>(
        &self,
        operation: F,
    ) -> Result<aura_core::time::timeout::NegativeCleanupExecution<&Self, T, AuraError>, AuraError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, AuraError>>,
    {
        let observed = self
            .child
            .execute_original_positive_terminal(self.material, self.approval, operation)
            .await?;
        Ok(aura_core::time::timeout::NegativeCleanupExecution {
            primary: self,
            operation: observed.operation,
            validation: observed.validation,
        })
    }
}
impl AdmittedPositiveTerminalRequest {
    pub(super) async fn prove_decision(
        &self,
        material: &OriginalPositiveTerminalSigningMaterial,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        destination: DeviceId,
    ) -> Result<OriginalPositivePacket, AuraError> {
        self.require_material(material, child)?;
        let Self::Local(actual) = self else {
            return Err(AuraError::permission_denied(
                "remote cannot originate positive decision",
            ));
        };
        let approval = self.approval();
        let message = self.message()?;
        let manifest = approval
            .manifest()
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "bind positive attestation manifest",
                    Arc::new(source),
                )
            })?;
        let digest = aura_core::hash::hash(&message);
        let attestation = aura_protocol::transcript_round_packet::PositiveDecisionAttestation {
            version: 1,
            approved_intent_digest: approval.canonical_intent_digest(),
            manifest_digest: aura_core::hash::hash(&manifest),
            manifest,
            coordinator: material.device,
            participant: destination,
            session: super::enrollment_transcript_wire::binding_session(
                approval,
                digest,
                destination,
            )?,
            sequence: 0,
            decision_digest: actual.decision_digest,
            terminal_transcript: message,
            transcript_digest: digest,
        };
        let bytes = attestation.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode positive decision proof", Arc::new(source))
        })?;
        let proof = material
            .effects
            .sign_participant_key_proof(
                &bytes,
                &material.local_share,
                aura_core::effects::crypto::SigningMode::Threshold,
            )
            .await?;
        let packet = OriginalPositivePacket::Decision(
            aura_protocol::transcript_round_packet::CoordinatorProvenPositiveDecision {
                attestation,
                possession_proof: proof,
            },
        );
        material
            .positive_transport(approval, child, Some(self))?
            .validate_send(destination, &packet)?;
        Ok(packet)
    }
    pub(super) fn require_round(
        &self,
        material: &OriginalPositiveTerminalSigningMaterial,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        proven: &aura_protocol::transcript_round_packet::ParticipantProvenRoundPacket,
        sender: DeviceId,
        participant: DeviceId,
    ) -> Result<(), AuraError> {
        use aura_protocol::transcript_round_packet::TranscriptRoundBody;
        self.require_material(material, child)?;
        let approval = self.approval();
        let packet = &proven.packet;
        let message = self.message()?;
        let digest = aura_core::hash::hash(&message);
        let coordinator = approval.manifest().initiator_device;
        let allowed = if sender == coordinator {
            matches!(
                (&packet.body, packet.sequence),
                (TranscriptRoundBody::RequestCommitment, 1)
                    | (TranscriptRoundBody::RequestShare(_), 3)
            )
        } else {
            sender == participant
                && matches!(
                    (&packet.body, packet.sequence),
                    (TranscriptRoundBody::Commitment(_), 2) | (TranscriptRoundBody::Share(_), 4)
                )
        };
        if !allowed
            || !material.ordered_devices.contains(&participant)
            || packet.version != 1
            || packet.approved_intent_digest != approval.canonical_intent_digest()
            || packet.coordinator != coordinator
            || packet.participant != participant
            || packet.transcript_digest != digest
            || packet.session
                != super::enrollment_transcript_wire::binding_session(
                    approval,
                    digest,
                    participant,
                )?
        {
            return Err(AuraError::permission_denied(
                "positive round differs from original finite scope",
            ));
        }
        if let TranscriptRoundBody::RequestShare(package) = &packet.body {
            if package.message != message
                || package.public_key_package != material.public_package
                || !package.participants.contains(
                    &material
                        .ordered_devices
                        .iter()
                        .position(|d| *d == participant)
                        .and_then(|i| u16::try_from(i + 1).ok())
                        .ok_or_else(|| AuraError::invalid("positive participant index absent"))?,
                )
            {
                return Err(AuraError::permission_denied(
                    "positive share package differs",
                ));
            }
        }
        let key = super::enrollment_transcript_wire::original_participant_verifier(
            &material.ordered_devices,
            &material.public_package,
            sender,
        )?;
        proven.verify_exact_participant(&key)
    }
    pub(super) async fn prove_round(
        &self,
        material: &OriginalPositiveTerminalSigningMaterial,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        packet: aura_protocol::transcript_round_packet::TranscriptRoundPacket,
    ) -> Result<OriginalPositivePacket, AuraError> {
        self.require_material(material, child)?;
        let bytes = packet.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode positive round", Arc::new(source))
        })?;
        let proof = material
            .effects
            .sign_participant_key_proof(
                &bytes,
                &material.local_share,
                aura_core::effects::crypto::SigningMode::Threshold,
            )
            .await?;
        let proven = aura_protocol::transcript_round_packet::ParticipantProvenRoundPacket {
            packet,
            possession_proof: proof,
        };
        self.require_round(
            material,
            child,
            &proven,
            material.device,
            proven.packet.participant,
        )?;
        Ok(OriginalPositivePacket::Round(proven))
    }
}
impl OriginalPositiveTransport<'_> {
    pub(crate) fn validate_send(
        &self,
        destination: DeviceId,
        packet: &OriginalPositivePacket,
    ) -> Result<(AuthorityId, [u8; 32], Vec<u8>), AuraError> {
        let locator = self.receive_locator(destination)?;
        let request = self.request.ok_or_else(|| {
            AuraError::permission_denied("positive send requires admitted finite request")
        })?;
        request.require_material(self.material, self.child)?;
        if let OriginalPositivePacket::Round(actual) = packet {
            let coordinator = self.approval.manifest().initiator_device;
            let participant = if self.material.device == coordinator {
                destination
            } else {
                self.material.device
            };
            if actual.packet.participant != participant
                || (self.material.device != coordinator && destination != coordinator)
            {
                return Err(AuraError::permission_denied(
                    "positive packet destination differs from native participant",
                ));
            }
        }
        match packet {
            OriginalPositivePacket::Round(actual) => request.require_round(
                self.material,
                self.child,
                actual,
                self.material.device,
                actual.packet.participant,
            )?,
            OriginalPositivePacket::Decision(actual) => {
                let AdmittedPositiveTerminalRequest::Local(local) = request else {
                    return Err(AuraError::permission_denied(
                        "participant cannot emit positive decision",
                    ));
                };
                let key = super::enrollment_transcript_wire::original_participant_verifier(
                    &self.material.ordered_devices,
                    &self.material.public_package,
                    self.material.device,
                )?;
                actual.verify_exact_coordinator(&key)?;
                let statement = &actual.attestation;
                let message = request.message()?;
                if self.material.device != self.approval.manifest().initiator_device
                    || statement.coordinator != self.material.device
                    || statement.participant != destination
                    || statement.approved_intent_digest != self.approval.canonical_intent_digest()
                    || statement.decision_digest != local.decision_digest
                    || statement.terminal_transcript != message
                    || statement.session
                        != super::enrollment_transcript_wire::binding_session(
                            self.approval,
                            aura_core::hash::hash(&message),
                            destination,
                        )?
                    || statement.manifest
                        != self
                            .approval
                            .manifest()
                            .required_transcript_bytes()
                            .map_err(|source| {
                                AuraError::crypto_with_source(
                                    "bind positive send manifest",
                                    Arc::new(source),
                                )
                            })?
                {
                    return Err(AuraError::permission_denied(
                        "positive decision send differs from actual receipt",
                    ));
                }
            }
        }
        Ok((
            self.approval.manifest().subject,
            locator,
            aura_core::util::serialization::to_vec(packet)?,
        ))
    }
}

#[derive(Default)]
pub(super) struct OriginalPositiveProxy {
    request: Option<Arc<AdmittedPositiveTerminalRequest>>,
    pending: Option<super::enrollment_transcript_signing::ParticipantRequest>,
    begin_reply: Option<tokio::sync::oneshot::Sender<Result<(), AuraError>>>,
    decision: Option<OriginalPositivePacket>,
    decision_send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    outgoing: Option<OriginalPositivePacket>,
    send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    receive: crate::runtime::effects::OriginalEnrollmentReceiveProgress<OriginalPositivePacket>,
    incoming: Option<OriginalPositivePacket>,
    phase: u8,
    complete: bool,
    failure: Option<AuraError>,
    outcome: super::enrollment_transcript_signing::ConditionalOutcomeCustody,
}
impl OriginalPositiveProxy {
    pub(super) async fn run(
        &mut self,
        receiver: &mut tokio::sync::mpsc::Receiver<
            super::enrollment_transcript_signing::ParticipantRequest,
        >,
        peer: DeviceId,
        index: u16,
        material: &OriginalPositiveTerminalSigningMaterial,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        if let Some(source) = &self.failure {
            return Err(source.clone());
        }
        let result = self
            .run_inner(receiver, peer, index, material, approval, child)
            .await;
        if let Err(source) = &result {
            self.failure.get_or_insert_with(|| source.clone());
        }
        result
    }
    async fn run_inner(
        &mut self,
        receiver: &mut tokio::sync::mpsc::Receiver<
            super::enrollment_transcript_signing::ParticipantRequest,
        >,
        peer: DeviceId,
        index: u16,
        material: &OriginalPositiveTerminalSigningMaterial,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        use super::enrollment_transcript_signing::ParticipantRequest;
        use aura_protocol::transcript_round_packet::{TranscriptRoundBody, TranscriptRoundPacket};
        material.require_original(approval, child)?;
        if self.complete {
            let observed = child
                .execute_original_positive_terminal(material, approval, || async { Ok(()) })
                .await?;
            return self.outcome.retain(observed);
        }
        if self.request.is_none() {
            let observed = child
                .execute_original_positive_terminal(material, approval, || async {
                    if self.pending.is_none() {
                        self.pending = Some(receiver.recv().await.ok_or_else(|| {
                            AuraError::invalid("original positive proxy ingress stopped")
                        })?);
                    }
                    Ok(())
                })
                .await?;
            self.outcome.retain(observed)?;
            match self
                .pending
                .take()
                .ok_or_else(|| AuraError::invalid("positive begin missing"))?
            {
                ParticipantRequest::BeginPositiveTerminal(request, response) => {
                    if !matches!(request.as_ref(), AdmittedPositiveTerminalRequest::Local(_)) {
                        return Err(AuraError::permission_denied(
                            "proxy requires actual local positive receipt",
                        ));
                    }
                    request.require_material(material, child)?;
                    self.request = Some(request);
                    self.begin_reply = Some(response);
                    self.phase = 1;
                }
                other => {
                    self.pending = Some(other);
                    return Err(AuraError::permission_denied(
                        "positive proxy requires finite decision admission",
                    ));
                }
            }
        }
        let request = Arc::clone(
            self.request
                .as_ref()
                .ok_or_else(|| AuraError::invalid("positive proxy decision absent"))?,
        );
        let owner = material.positive_transport(approval, child, Some(&request))?;
        if self.decision.is_none() {
            let observed = child
                .execute_original_positive_terminal(material, approval, || async {
                    match request.prove_decision(material, child, peer).await {
                        Ok(packet) => self.decision = Some(packet),
                        Err(source) => {
                            self.failure.get_or_insert_with(|| source.clone());
                            return Err(source);
                        }
                    }
                    Ok(())
                })
                .await?;
            self.outcome.retain(observed)?;
        }
        let observed = material
            .effects
            .send_original_positive_enrollment_packet(
                &owner,
                peer,
                self.decision
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("positive decision proof missing"))?,
                &mut self.decision_send,
            )
            .await?;
        self.outcome.retain(observed)?;
        if let Some(response) = self.begin_reply.take() {
            response
                .send(Ok(()))
                .map_err(|_| AuraError::invalid("original positive begin observer dropped"))?;
        }
        while self.phase <= 3 {
            if self.pending.is_none() {
                let observed = child
                    .execute_original_positive_terminal(material, approval, || async {
                        self.pending =
                            Some(receiver.recv().await.ok_or_else(|| {
                                AuraError::invalid("positive proxy ingress stopped")
                            })?);
                        Ok(())
                    })
                    .await?;
                self.outcome.retain(observed)?;
            }
            if self.outgoing.is_none() {
                let body = match (self.phase, self.pending.as_ref()) {
                    (1, Some(ParticipantRequest::Commitment(_))) => {
                        TranscriptRoundBody::RequestCommitment
                    }
                    (3, Some(ParticipantRequest::Share(package, _))) => {
                        TranscriptRoundBody::RequestShare(package.clone())
                    }
                    _ => {
                        return Err(AuraError::permission_denied(
                            "positive proxy repeated or skipped phase",
                        ))
                    }
                };
                let digest = aura_core::hash::hash(&request.message()?);
                let packet = TranscriptRoundPacket {
                    version: 1,
                    session: super::enrollment_transcript_wire::binding_session(
                        approval, digest, peer,
                    )?,
                    approved_intent_digest: approval.canonical_intent_digest(),
                    transcript_digest: digest,
                    coordinator: material.device,
                    participant: peer,
                    sequence: self.phase,
                    body,
                };
                let observed = child
                    .execute_original_positive_terminal(material, approval, || async {
                        match request.prove_round(material, child, packet).await {
                            Ok(packet) => self.outgoing = Some(packet),
                            Err(source) => {
                                self.failure.get_or_insert_with(|| source.clone());
                                return Err(source);
                            }
                        }
                        Ok(())
                    })
                    .await?;
                self.outcome.retain(observed)?;
            }
            let observed = material
                .effects
                .send_original_positive_enrollment_packet(
                    &owner,
                    peer,
                    self.outgoing
                        .as_ref()
                        .ok_or_else(|| AuraError::invalid("positive outgoing absent"))?,
                    &mut self.send,
                )
                .await?;
            self.outcome.retain(observed)?;
            if self.incoming.is_none() {
                let observed = material
                    .effects
                    .receive_original_positive_enrollment_packet(&owner, peer, &mut self.receive)
                    .await?;
                self.outcome.retain(observed)?;
                self.incoming = self.receive.take_packet();
            }
            let Some(OriginalPositivePacket::Round(proven)) = self.incoming.as_ref() else {
                return Err(AuraError::permission_denied(
                    "positive response is not exact round",
                ));
            };
            request.require_round(material, child, proven, peer, peer)?;
            if proven.packet.sequence != self.phase + 1 {
                return Err(AuraError::permission_denied(
                    "positive response sequence differs",
                ));
            }
            match (self.pending.take(), &proven.packet.body) {
                (
                    Some(ParticipantRequest::Commitment(response)),
                    TranscriptRoundBody::Commitment(commitment),
                ) if commitment.participant_index == index => {
                    response
                        .send(Ok(commitment.clone()))
                        .map_err(|_| AuraError::invalid("positive commitment receiver dropped"))?;
                }
                (
                    Some(ParticipantRequest::Share(_, response)),
                    TranscriptRoundBody::Share(share),
                ) => {
                    response
                        .send(Ok(share.clone()))
                        .map_err(|_| AuraError::invalid("positive share receiver dropped"))?;
                }
                _ => {
                    return Err(AuraError::permission_denied(
                        "positive response body differs",
                    ))
                }
            }
            self.phase += 2;
            self.outgoing = None;
            self.incoming = None;
            self.send = crate::runtime::effects::OriginalEnrollmentSendProgress::default();
            self.receive = crate::runtime::effects::OriginalEnrollmentReceiveProgress::default();
        }
        self.complete = true;
        Ok(())
    }
}

#[derive(Default)]
pub(in crate::runtime::services) struct OriginalPositiveDispatcher {
    request: Option<Arc<AdmittedPositiveTerminalRequest>>,
    incoming: Option<OriginalPositivePacket>,
    receive: crate::runtime::effects::OriginalEnrollmentReceiveProgress<OriginalPositivePacket>,
    begin: Option<super::enrollment_transcript_signing::RetainedParticipantCall<()>>,
    commitment: Option<
        super::enrollment_transcript_signing::RetainedParticipantCall<
            aura_core::effects::crypto::FrostPublicCommitment,
        >,
    >,
    share: Option<super::enrollment_transcript_signing::RetainedParticipantCall<Vec<u8>>>,
    outgoing: Option<OriginalPositivePacket>,
    send: crate::runtime::effects::OriginalEnrollmentSendProgress,
    phase: u8,
    complete: bool,
    failure: Option<AuraError>,
    outcome: super::enrollment_transcript_signing::ConditionalOutcomeCustody,
}
impl OriginalPositiveDispatcher {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalPositiveTerminalSigningMaterial", receiver_type = OriginalPositiveDispatcher, family = "runtime_helper")]
    pub(in crate::runtime::services) async fn run(
        &mut self,
        material: &OriginalPositiveTerminalSigningMaterial,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        ingress: &super::EnrollmentTranscriptParticipantIngress,
    ) -> Result<(), AuraError> {
        if let Some(source) = &self.failure {
            return Err(source.clone());
        }
        let result = self.run_inner(material, approval, child, ingress).await;
        if let Err(source) = &result {
            self.failure.get_or_insert_with(|| source.clone());
        }
        result
    }
    async fn run_inner(
        &mut self,
        material: &OriginalPositiveTerminalSigningMaterial,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        ingress: &super::EnrollmentTranscriptParticipantIngress,
    ) -> Result<(), AuraError> {
        use aura_protocol::transcript_round_packet::{TranscriptRoundBody, TranscriptRoundPacket};
        material.require_original(approval, child)?;
        if self.complete {
            let observed = child
                .execute_original_positive_terminal(material, approval, || async { Ok(()) })
                .await?;
            return self.outcome.retain(observed);
        }
        let coordinator = approval.manifest().initiator_device;
        if self.request.is_none() {
            let owner = material.positive_transport(approval, child, None)?;
            if self.incoming.is_none() {
                let observed = material
                    .effects
                    .receive_original_positive_enrollment_packet(
                        &owner,
                        coordinator,
                        &mut self.receive,
                    )
                    .await?;
                self.outcome.retain(observed)?;
                self.incoming = self.receive.take_packet();
            }
            let packet = self
                .incoming
                .take()
                .ok_or_else(|| AuraError::invalid("positive decision data absent"))?;
            let OriginalPositivePacket::Decision(proven) = packet else {
                self.incoming = Some(packet);
                return Err(AuraError::permission_denied(
                    "positive dispatcher requires first decision",
                ));
            };
            let request = Arc::new(AdmittedPositiveTerminalRequest::admit(
                proven,
                material,
                Arc::clone(approval),
                child,
            )?);
            self.begin = Some(
                super::enrollment_transcript_signing::RetainedParticipantCall::begin_positive(
                    Arc::clone(&request),
                ),
            );
            self.request = Some(request);
            self.receive = crate::runtime::effects::OriginalEnrollmentReceiveProgress::default();
            self.phase = 1;
        }
        if let Some(begin) = &mut self.begin {
            let observed = child
                .execute_original_positive_terminal(material, approval, || async {
                    begin.run(ingress).await
                })
                .await?;
            self.outcome.retain(observed)?;
        }
        let request = Arc::clone(
            self.request
                .as_ref()
                .ok_or_else(|| AuraError::invalid("positive admitted request absent"))?,
        );
        let owner = material.positive_transport(approval, child, Some(&request))?;
        while self.phase <= 3 {
            if self.incoming.is_none() {
                let observed = material
                    .effects
                    .receive_original_positive_enrollment_packet(
                        &owner,
                        coordinator,
                        &mut self.receive,
                    )
                    .await?;
                self.outcome.retain(observed)?;
                self.incoming = self.receive.take_packet();
            }
            let Some(OriginalPositivePacket::Round(proven)) = self.incoming.as_ref() else {
                return Err(AuraError::permission_denied(
                    "positive dispatcher requires exact round",
                ));
            };
            request.require_round(material, child, proven, coordinator, material.device)?;
            if proven.packet.sequence != self.phase {
                return Err(AuraError::permission_denied(
                    "positive request replay or skipped phase",
                ));
            }
            if self.outgoing.is_none() {
                let body = match (&proven.packet.body, self.phase) {
                    (TranscriptRoundBody::RequestCommitment, 1) => {
                        if self.commitment.is_none() {
                            self.commitment=Some(super::enrollment_transcript_signing::RetainedParticipantCall::commitment());
                        }
                        let call = self
                            .commitment
                            .as_mut()
                            .ok_or_else(|| AuraError::invalid("positive commitment call absent"))?;
                        let observed = child
                            .execute_original_positive_terminal(material, approval, || async {
                                call.run(ingress).await.map(|_| ())
                            })
                            .await?;
                        self.outcome.retain(observed)?;
                        TranscriptRoundBody::Commitment(call.retained_result()?)
                    }
                    (TranscriptRoundBody::RequestShare(package), 3) => {
                        if self.share.is_none() {
                            self.share=Some(super::enrollment_transcript_signing::RetainedParticipantCall::share(package.clone()));
                        }
                        let call = self
                            .share
                            .as_mut()
                            .ok_or_else(|| AuraError::invalid("positive share call absent"))?;
                        let observed = child
                            .execute_original_positive_terminal(material, approval, || async {
                                call.run(ingress).await.map(|_| ())
                            })
                            .await?;
                        self.outcome.retain(observed)?;
                        TranscriptRoundBody::Share(call.retained_result()?)
                    }
                    _ => {
                        return Err(AuraError::permission_denied(
                            "positive request body differs",
                        ))
                    }
                };
                let digest = aura_core::hash::hash(&request.message()?);
                let packet = TranscriptRoundPacket {
                    version: 1,
                    session: super::enrollment_transcript_wire::binding_session(
                        approval,
                        digest,
                        material.device,
                    )?,
                    approved_intent_digest: approval.canonical_intent_digest(),
                    transcript_digest: digest,
                    coordinator,
                    participant: material.device,
                    sequence: self.phase + 1,
                    body,
                };
                let observed = child
                    .execute_original_positive_terminal(material, approval, || async {
                        match request.prove_round(material, child, packet).await {
                            Ok(packet) => self.outgoing = Some(packet),
                            Err(source) => {
                                self.failure.get_or_insert_with(|| source.clone());
                                return Err(source);
                            }
                        }
                        Ok(())
                    })
                    .await?;
                self.outcome.retain(observed)?;
            }
            let observed = material
                .effects
                .send_original_positive_enrollment_packet(
                    &owner,
                    coordinator,
                    self.outgoing
                        .as_ref()
                        .ok_or_else(|| AuraError::invalid("positive response packet absent"))?,
                    &mut self.send,
                )
                .await?;
            self.outcome.retain(observed)?;
            self.phase += 2;
            self.incoming = None;
            self.outgoing = None;
            self.receive = crate::runtime::effects::OriginalEnrollmentReceiveProgress::default();
            self.send = crate::runtime::effects::OriginalEnrollmentSendProgress::default();
        }
        self.complete = true;
        Ok(())
    }
}

#[derive(Default)]
/// Locally retained positive-signing progress inside the original custody.
/// The enclosing owner coordinates participant actors and task disposal.
pub(super) struct OriginalPositiveControlState {
    request: Option<Arc<AdmittedPositiveTerminalRequest>>,
    calls: Vec<super::enrollment_transcript_signing::RetainedParticipantCall<()>>,
    signature: Option<Vec<u8>>,
    started: bool,
    complete: bool,
    retirement: Option<RetiredOriginalSigningMaterial>,
    failure: Option<AuraError>,
    outcome: super::enrollment_transcript_signing::ConditionalOutcomeCustody,
}
impl OriginalIssuerTranscriptSigningCustody {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalEnrollmentPositiveDecisionReceipt", receiver_type = OriginalIssuerTranscriptSigningCustody, family = "runtime_helper")]
    pub(crate) fn sign_original_positive_terminal<'a>(
        &'a mut self,
        issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        child: &'a super::super::enrollment_window::EnrollmentExecutionChild,
        decision: &'a super::super::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt,
    ) -> super::EnrollmentOperationFuture<'a, Result<Vec<u8>, AuraError>> {
        Box::pin(async move {
            self.require_original_issued_control(&issued, child)?;
            decision.require_original_control(&issued)?;
            if let Some(source) = &self.positive_control.failure {
                return Err(source.clone());
            }
            if self.positive_control.complete {
                self.positive_control
                    .request
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("positive request absent"))?
                    .require_material_if_retired(&self.approval, &issued, decision)?;
                return self
                    .positive_control
                    .signature
                    .clone()
                    .ok_or_else(|| AuraError::invalid("positive aggregate absent"));
            }
            self.retain_original_positive_terminal(&issued, child, decision)?;
            if self.positive_control.request.is_none() {
                let request = Arc::new(AdmittedPositiveTerminalRequest::Local(Arc::new(
                    OriginalPositiveTerminalRequest::from_original_decision(
                        Arc::clone(&self.approval),
                        &issued,
                        child,
                        decision,
                    )?,
                )));
                self.positive_control.calls=self.ingresses.iter().map(|_|super::enrollment_transcript_signing::RetainedParticipantCall::begin_positive(Arc::clone(&request))).collect();
                self.positive_control.request = Some(request);
            }
            if self.positive_control.started {
                return Err(AuraError::permission_denied(
                    "original positive signing remains unresolved; nonce cannot restart",
                ));
            }
            let material = self
                .positive_terminal
                .as_ref()
                .ok_or_else(|| AuraError::invalid("actual original positive material absent"))?;
            let request = Arc::clone(
                self.positive_control
                    .request
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("positive request absent"))?,
            );
            request.require_material(material, child)?;
            let AdmittedPositiveTerminalRequest::Local(local) = request.as_ref() else {
                return Err(AuraError::permission_denied(
                    "coordinator lacks original protected positive decision",
                ));
            };
            let participants = material
                .ordered_devices
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    u16::try_from(i + 1)
                        .map(|index| (*d, index))
                        .map_err(|source| {
                            AuraError::crypto_with_source("positive roster index", Arc::new(source))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let verifier = self
                .approval
                .manifest()
                .initiator_confirmation_verifier
                .clone();
            let key = aura_core::TrustedPublicKey::active(
                aura_core::TrustedKeyDomain::AuthorityThreshold,
                Some(self.approval.manifest().final_epoch),
                verifier.clone(),
                aura_core::Hash32(aura_core::hash::hash(&verifier)),
            );
            let policy =
                aura_protocol::public_transcript_signing::PublicTranscriptSigningPolicy::checked(
                    &local.transcript,
                    &material.public_package,
                    &key,
                    material.threshold,
                    &participants,
                )?;
            self.positive_control.started = true;
            let ingresses = &self.ingresses;
            let calls = &mut self.positive_control.calls;
            let signature = &mut self.positive_control.signature;
            let first_failure = &mut self.positive_control.failure;
            let actor = self
                .local_actor
                .as_mut()
                .ok_or_else(|| AuraError::invalid("same positive local actor absent"))?;
            let proxies = &mut self.remote_actors;
            let approval = &self.approval;
            let observed = child
                .execute_original_positive_terminal(material, approval, || async {
                    let coordinate = async {
                        futures::future::try_join_all(
                            calls
                                .iter_mut()
                                .zip(ingresses.iter())
                                .map(|(call, ingress)| call.run(ingress)),
                        )
                        .await?;
                        let actual =
                            aura_protocol::public_transcript_signing::sign_public_transcript(
                                material.effects.as_ref(),
                                policy,
                                |device, index| async move {
                                    ingresses
                                        .iter()
                                        .find(|i| i.matches_original_participant(device, index))
                                        .ok_or_else(|| {
                                            AuraError::invalid("positive participant absent")
                                        })?
                                        .commitment()
                                        .await
                                },
                                |device, index, package| async move {
                                    ingresses
                                        .iter()
                                        .find(|i| i.matches_original_participant(device, index))
                                        .ok_or_else(|| {
                                            AuraError::invalid("positive participant absent")
                                        })?
                                        .share(package)
                                        .await
                                },
                            )
                            .await?;
                        *signature = Some(actual);
                        Ok::<(), AuraError>(())
                    };
                    let result = tokio::try_join!(
                        actor.run_positive(material, approval, child),
                        futures::future::try_join_all(
                            proxies
                                .iter_mut()
                                .map(|p| p.run_positive(material, approval, child))
                        ),
                        coordinate
                    )
                    .map(|_| ());
                    if let Err(source) = &result {
                        first_failure.get_or_insert_with(|| source.clone());
                    }
                    result
                })
                .await?;
            if let Err(source) = self.positive_control.outcome.retain(observed) {
                self.positive_control
                    .failure
                    .get_or_insert_with(|| source.clone());
                return Err(source);
            }
            let actual = self
                .positive_terminal
                .take()
                .ok_or_else(|| AuraError::invalid("original positive owner disappeared"))?;
            match actual.retire_original(&self.approval, child) {
                Ok(receipt) => self.positive_control.retirement = Some(receipt),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.positive_terminal = Some(actual);
                    self.positive_control.failure = Some(source.clone());
                    return Err(source);
                }
            }
            self.positive_control.complete = true;
            self.positive_control
                .signature
                .clone()
                .ok_or_else(|| AuraError::invalid("actual positive aggregate absent"))
        })
    }
    pub(crate) fn require_original_positive_terminal_message(
        &self,
        message: &[u8],
    ) -> Result<(), AuraError> {
        let request = self
            .positive_control
            .request
            .as_ref()
            .ok_or_else(|| AuraError::invalid("actual positive request absent"))?;
        if !self.positive_control.complete
            || self.positive_control.retirement.is_none()
            || request.message()? != message
        {
            return Err(AuraError::permission_denied(
                "positive frame differs from actual completed original control",
            ));
        }
        Ok(())
    }
}
impl AdmittedPositiveTerminalRequest {
    fn require_material_if_retired(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        decision: &super::super::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt,
    ) -> Result<(), AuraError> {
        let Self::Local(local) = self else {
            return Err(AuraError::permission_denied(
                "completed positive request is not original local",
            ));
        };
        local.require_original_decision()?;
        decision.require_original_control(issued)?;
        if local.decision_digest != decision.decision_digest()
            || local.approval.canonical_intent_digest() != approval.canonical_intent_digest()
        {
            return Err(AuraError::permission_denied(
                "completed positive decision changed",
            ));
        }
        Ok(())
    }
}
