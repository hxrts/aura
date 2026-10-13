//! Actual original conditional-share retirement and its finite ACK transport.
use super::*;
use aura_protocol::transcript_round_packet::{
    ConditionalRetirementAcknowledgment, ConditionalRetirementRequest,
    CoordinatorProvenConditionalRetirementRequest,
    ParticipantProvenConditionalRetirementAcknowledgment,
};
use aura_signature::SecurityTranscript;

pub(crate) struct OriginalRetiredConditionalTransportCustody {
    retired: RetiredOriginalSigningMaterial,
    approval: Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    original_window: super::super::enrollment_window::OriginalConditionalSigningWindowIdentity,
    acknowledgment_bytes: Vec<u8>,
    locator: [u8; 32],
    coordinator: DeviceId,
}

impl OriginalNegativeTerminalSigningMaterial {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "EnrollmentActivationCapability", receiver_type = OriginalNegativeTerminalSigningMaterial, family = "runtime_helper")]
    pub(in crate::runtime::services) async fn produce_original_conditional_retirement_request(
        &self,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        activation: &super::super::ceremony_tracker::EnrollmentActivationCapability<'_>,
        participant: DeviceId,
        retained: &mut Option<CoordinatorProvenConditionalRetirementRequest>,
    ) -> Result<(), AuraError> {
        let transport = self.conditional_transport(approval, child, None)?;
        let manifest = approval.manifest();
        activation.require_effects(self.effects.as_ref())?;
        if self.device != manifest.initiator_device
            || activation.ceremony_id() != &manifest.ceremony
        {
            return Err(AuraError::permission_denied(
                "conditional stop lacks original activation readiness",
            ));
        }
        activation
            .require_generation(manifest.subject, manifest.pending_epoch)
            .await?;
        let request = ConditionalRetirementRequest {
            version: 1,
            approved_intent_digest: approval.canonical_intent_digest(),
            manifest_digest: aura_core::hash::hash(&manifest.required_transcript_bytes().map_err(
                |source| {
                    AuraError::crypto_with_source("encode original stop manifest", Arc::new(source))
                },
            )?),
            coordinator: self.device,
            participant,
            locator: transport.receive_locator(participant)?,
        };
        let verifier = enrollment_transcript_wire::original_participant_verifier(
            &self.ordered_devices,
            &self.public_package,
            self.device,
        )?;
        if let Some(actual) = retained.as_ref() {
            if actual.request != request {
                return Err(AuraError::permission_denied(
                    "retained stop belongs to another original participant",
                ));
            }
            return actual.verify_exact_coordinator(&verifier);
        }
        let bytes = request.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode original stop request", Arc::new(source))
        })?;
        let possession_proof = self
            .effects
            .sign_participant_key_proof(&bytes, &self.local_share, SigningMode::Threshold)
            .await?;
        let actual = CoordinatorProvenConditionalRetirementRequest {
            request,
            possession_proof,
        };
        actual.verify_exact_coordinator(&verifier)?;
        *retained = Some(actual);
        Ok(())
    }

    fn require_original_retirement_request(
        &self,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        proven: &CoordinatorProvenConditionalRetirementRequest,
    ) -> Result<(), AuraError> {
        let transport = self.conditional_transport(approval, child, None)?;
        let request = &proven.request;
        let manifest = approval.manifest();
        if self.device == manifest.initiator_device
            || request.approved_intent_digest != approval.canonical_intent_digest()
            || request.manifest_digest
                != aura_core::hash::hash(&manifest.required_transcript_bytes().map_err(
                    |source| {
                        AuraError::crypto_with_source(
                            "encode original retirement manifest",
                            Arc::new(source),
                        )
                    },
                )?)
            || request.coordinator != manifest.initiator_device
            || request.participant != self.device
            || request.locator != transport.receive_locator(manifest.initiator_device)?
        {
            return Err(AuraError::permission_denied(
                "conditional retirement differs from original participant custody",
            ));
        }
        let coordinator = enrollment_transcript_wire::original_participant_verifier(
            &self.ordered_devices,
            &self.public_package,
            manifest.initiator_device,
        )?;
        proven.verify_exact_coordinator(&coordinator)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalNegativeTerminalSigningMaterial", receiver_type = OriginalNegativeTerminalSigningMaterial, family = "runtime_helper")]
    pub(in crate::runtime::services) async fn produce_original_conditional_retirement_ack(
        &self,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        request: &CoordinatorProvenConditionalRetirementRequest,
        retained: &mut Option<ParticipantProvenConditionalRetirementAcknowledgment>,
    ) -> Result<(), AuraError> {
        self.require_original_retirement_request(approval, child, request)?;
        let verifier = enrollment_transcript_wire::original_participant_verifier(
            &self.ordered_devices,
            &self.public_package,
            self.device,
        )?;
        if let Some(actual) = retained.as_ref() {
            actual.acknowledgment.require_request(&request.request)?;
            return actual.verify_exact_participant(&verifier);
        }
        let acknowledgment = ConditionalRetirementAcknowledgment {
            request: request.request.clone(),
            request_digest: request.request.digest()?,
        };
        let bytes = acknowledgment
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode original retirement acknowledgment",
                    Arc::new(source),
                )
            })?;
        let possession_proof = self
            .effects
            .sign_participant_key_proof(&bytes, &self.local_share, SigningMode::Threshold)
            .await?;
        let actual = ParticipantProvenConditionalRetirementAcknowledgment {
            acknowledgment,
            possession_proof,
        };
        actual.verify_exact_participant(&verifier)?;
        *retained = Some(actual);
        Ok(())
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalNegativeTerminalSigningMaterial", receiver_type = OriginalNegativeTerminalSigningMaterial, family = "runtime_helper")]
    pub(in crate::runtime::services) fn retire_original_conditional_transport(
        mut self,
        approval: Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        request: &CoordinatorProvenConditionalRetirementRequest,
        acknowledgment: &ParticipantProvenConditionalRetirementAcknowledgment,
    ) -> Result<OriginalRetiredConditionalTransportCustody, Box<(Self, AuraError)>> {
        let checked = (|| -> Result<Vec<u8>, AuraError> {
            self.require_original_retirement_request(&approval, child, request)?;
            acknowledgment
                .acknowledgment
                .require_request(&request.request)?;
            let verifier = enrollment_transcript_wire::original_participant_verifier(
                &self.ordered_devices,
                &self.public_package,
                self.device,
            )?;
            acknowledgment.verify_exact_participant(&verifier)?;
            Ok(aura_core::util::serialization::to_vec(acknowledgment)?)
        })();
        let acknowledgment_bytes = match checked {
            Ok(actual) => actual,
            Err(source) => return Err(Box::new((self, source))),
        };
        use zeroize::Zeroize;
        self.local_share.zeroize();
        Ok(OriginalRetiredConditionalTransportCustody {
            retired: RetiredOriginalSigningMaterial {
                effects: self.effects,
                approved_intent_digest: self.approved_intent_digest,
                device: self.device,
            },
            approval,
            original_window: self.original_window,
            acknowledgment_bytes,
            locator: request.request.locator,
            coordinator: request.request.coordinator,
        })
    }
}

impl OriginalRetiredConditionalTransportCustody {
    pub(in crate::runtime::services) fn material_retirement(
        &self,
    ) -> &RetiredOriginalSigningMaterial {
        &self.retired
    }
    pub(crate) fn require_original_window(
        &self,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        self.retired.require_original(&self.approval)?;
        self.original_window.require_child(child)
    }
    pub(crate) fn effects(&self) -> &Arc<AuraEffectSystem> {
        &self.retired.effects
    }
    pub(in crate::runtime::services) fn transport<'owner>(
        &'owner self,
        child: &'owner super::super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<OriginalRetiredConditionalTransport<'owner>, AuraError> {
        self.require_original_window(child)?;
        Ok(OriginalRetiredConditionalTransport {
            custody: self,
            child,
        })
    }
}

pub(crate) struct OriginalRetiredConditionalTransport<'owner> {
    custody: &'owner OriginalRetiredConditionalTransportCustody,
    child: &'owner super::super::enrollment_window::EnrollmentExecutionChild,
}
impl<'custody> OriginalRetiredConditionalTransport<'custody> {
    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self.custody.require_original_window(self.child)?;
        if !std::ptr::eq(self.custody.effects().as_ref(), effects) {
            return Err(AuraError::permission_denied(
                "retired conditional transport belongs to another runtime",
            ));
        }
        Ok(())
    }
    pub(crate) fn original_effects_origin(&self) -> std::sync::Weak<AuraEffectSystem> {
        Arc::downgrade(self.custody.effects())
    }
    pub(crate) fn validate_send(
        &self,
        destination: DeviceId,
        packet: &OriginalConditionalPacket,
    ) -> Result<(AuthorityId, [u8; 32], Vec<u8>), AuraError> {
        self.require_effects(self.custody.effects().as_ref())?;
        let OriginalConditionalPacket::RetirementAcknowledgment(actual) = packet else {
            return Err(AuraError::permission_denied(
                "retired conditional transport permits only its original acknowledgment",
            ));
        };
        if destination != self.custody.coordinator
            || aura_core::util::serialization::to_vec(actual)? != self.custody.acknowledgment_bytes
        {
            return Err(AuraError::permission_denied(
                "retired conditional acknowledgment differs from original transition",
            ));
        }
        Ok((
            self.custody.approval.manifest().subject,
            self.custody.locator,
            aura_core::util::serialization::to_vec(packet)?,
        ))
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalRetiredConditionalTransport", receiver_type = OriginalRetiredConditionalTransport, family = "runtime_helper")]
    pub(crate) async fn execute<F, Fut, T>(
        &self,
        operation: F,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<
            &OriginalRetiredConditionalTransport<'custody>,
            T,
            AuraError,
        >,
        AuraError,
    >
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, AuraError>>,
    {
        let actual = self
            .child
            .execute_original_retired_conditional_transport(self.custody, operation)
            .await?;
        Ok(aura_core::time::timeout::NegativeCleanupExecution {
            primary: self,
            operation: actual.operation,
            validation: actual.validation,
        })
    }
}

/// Stop emission borrows the actual full-quorum activation claim until delivery.
pub(crate) struct OriginalConditionalRetirementTransport<'owner, 'activation> {
    material: &'owner OriginalNegativeTerminalSigningMaterial,
    approval: &'owner Arc<
        crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    >,
    child: &'owner super::super::enrollment_window::EnrollmentExecutionChild,
    activation: &'owner super::super::ceremony_tracker::EnrollmentActivationCapability<'activation>,
    request: &'owner CoordinatorProvenConditionalRetirementRequest,
}
impl OriginalNegativeTerminalSigningMaterial {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "EnrollmentActivationCapability", receiver_type = OriginalNegativeTerminalSigningMaterial, family = "runtime_helper")]
    pub(in crate::runtime::services) async fn retirement_request_transport<'owner, 'activation>(
        &'owner self,
        approval: &'owner Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        child: &'owner super::super::enrollment_window::EnrollmentExecutionChild,
        activation: &'owner super::super::ceremony_tracker::EnrollmentActivationCapability<
            'activation,
        >,
        request: &'owner CoordinatorProvenConditionalRetirementRequest,
    ) -> Result<OriginalConditionalRetirementTransport<'owner, 'activation>, AuraError> {
        self.require_original(approval)?;
        self.require_original_window(child)?;
        activation.require_effects(self.effects.as_ref())?;
        let manifest = approval.manifest();
        if activation.ceremony_id() != &manifest.ceremony
            || self.device != manifest.initiator_device
        {
            return Err(AuraError::permission_denied(
                "stop transport lacks original activation claim",
            ));
        }
        activation
            .require_generation(manifest.subject, manifest.pending_epoch)
            .await?;
        let actual = OriginalConditionalRetirementTransport {
            material: self,
            approval,
            child,
            activation,
            request,
        };
        actual.validate_send(
            request.request.participant,
            &OriginalConditionalPacket::RetirementRequest(
                aura_core::util::serialization::from_slice(
                    &aura_core::util::serialization::to_vec(request)?,
                )?,
            ),
        )?;
        Ok(actual)
    }
}
impl<'material, 'activation> OriginalConditionalRetirementTransport<'material, 'activation> {
    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self.material.require_original(self.approval)?;
        self.material.require_original_window(self.child)?;
        self.activation.require_effects(effects)?;
        if !std::ptr::eq(self.material.effects.as_ref(), effects)
            || self.activation.ceremony_id() != &self.approval.manifest().ceremony
        {
            return Err(AuraError::permission_denied(
                "stop transport differs from original runtime claim",
            ));
        }
        Ok(())
    }
    pub(crate) fn original_effects_origin(&self) -> std::sync::Weak<AuraEffectSystem> {
        Arc::downgrade(&self.material.effects)
    }
    pub(crate) fn validate_send(
        &self,
        destination: DeviceId,
        packet: &OriginalConditionalPacket,
    ) -> Result<(AuthorityId, [u8; 32], Vec<u8>), AuraError> {
        self.require_effects(self.material.effects.as_ref())?;
        let OriginalConditionalPacket::RetirementRequest(actual) = packet else {
            return Err(AuraError::permission_denied(
                "stop transport permits only its original request",
            ));
        };
        let manifest = self.approval.manifest();
        let locator = self
            .material
            .conditional_transport(self.approval, self.child, None)?
            .receive_locator(destination)?;
        let request = &actual.request;
        if destination != request.participant
            || request.coordinator != self.material.device
            || request.approved_intent_digest != self.approval.canonical_intent_digest()
            || request.locator != locator
            || request.manifest_digest
                != aura_core::hash::hash(&manifest.required_transcript_bytes().map_err(
                    |source| {
                        AuraError::crypto_with_source("encode stop manifest", Arc::new(source))
                    },
                )?)
            || aura_core::util::serialization::to_vec(actual)?
                != aura_core::util::serialization::to_vec(self.request)?
        {
            return Err(AuraError::permission_denied(
                "stop packet differs from retained original activation request",
            ));
        }
        actual.verify_exact_coordinator(
            &enrollment_transcript_wire::original_participant_verifier(
                &self.material.ordered_devices,
                &self.material.public_package,
                self.material.device,
            )?,
        )?;
        Ok((
            manifest.subject,
            locator,
            aura_core::util::serialization::to_vec(packet)?,
        ))
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalConditionalRetirementTransport", receiver_type = OriginalConditionalRetirementTransport, family = "runtime_helper")]
    pub(crate) async fn execute<F, Fut, T>(
        &self,
        operation: F,
    ) -> Result<aura_core::time::timeout::NegativeCleanupExecution<&Self, T, AuraError>, AuraError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, AuraError>>,
    {
        self.require_effects(self.material.effects.as_ref())?;
        let actual = self
            .child
            .execute_original_negative_terminal(self.material, self.approval, operation)
            .await?;
        Ok(aura_core::time::timeout::NegativeCleanupExecution {
            primary: self,
            operation: actual.operation,
            validation: actual.validation,
        })
    }
}
