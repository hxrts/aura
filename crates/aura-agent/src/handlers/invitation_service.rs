//! Invitation Service - Public API for Invitation Operations
//!
//! Provides a clean public interface for invitation operations.
//! Wraps `InvitationHandler` with ergonomic methods and proper error handling.

use super::invitation::{
    execute_invitation_effect_commands, DeferredInvitationNetworkEffects, Invitation,
    InvitationHandler, InvitationResult, InvitationStatus, InvitationType, ShareableInvitation,
    ShareableInvitationError, ShareableInvitationSenderProof, ShareableInvitationTransportMetadata,
};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::{AuraEffectSystem, TaskSupervisor};
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId};
use aura_core::DeviceId;
use aura_core::Hash32;
use aura_signature::sign_ed25519_transcript;
use std::str::FromStr;
use std::sync::Arc;

#[path = "invitation_service/cancellation_custody.rs"]
mod cancellation_custody;

const DEFERRED_INVITATION_DELIVERY_ATTEMPTS: usize = 12;
const DEFERRED_INVITATION_DELIVERY_BACKOFF_MS: u64 = 500;

/// A committed original shortcut with a verified native quorum proof over its
/// final recipient-bound code. Only the issuer method below can construct it.
pub(crate) struct IssuedOriginalShortcutDelivery {
    invitation_id: InvitationId,
    issuer: AuthorityId,
    recipient: AuthorityId,
    recipient_device: DeviceId,
    purpose: crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose,
    code: String,
}

impl IssuedOriginalShortcutDelivery {
    pub(crate) fn invitation_id(&self) -> &InvitationId {
        &self.invitation_id
    }

    pub(crate) fn issuer(&self) -> AuthorityId {
        self.issuer
    }

    pub(crate) fn recipient(&self) -> AuthorityId {
        self.recipient
    }

    pub(crate) fn recipient_device(&self) -> DeviceId {
        self.recipient_device
    }

    pub(crate) fn purpose(
        &self,
    ) -> crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose {
        self.purpose
    }

    pub(crate) fn code(&self) -> &str {
        &self.code
    }
}

// Pure trusted-key metadata is derived by the caller from its retained native
// owner; this helper verifies the actual typed domain, never arbitrary bytes.
async fn verify_original_quorum_transcript<T: aura_signature::SecurityTranscript + ?Sized>(
    effects: &AuraEffectSystem,
    transcript: &T,
    signature: &[u8],
    trusted_key: &aura_core::TrustedPublicKey,
) -> AgentResult<bool> {
    use aura_core::effects::CryptoExtendedEffects;
    let transcript_bytes = transcript.required_transcript_bytes().map_err(|source| {
        AgentError::from(aura_core::AuraError::crypto_with_source(
            "encode original quorum verification transcript",
            Arc::new(source),
        ))
    })?;
    Ok(effects
        .frost_verify(&transcript_bytes, signature, trusted_key.bytes())
        .await?)
}

impl InvitationServiceApi {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "issued", capability_type = IssuedEnrollmentManifestBinding, family = "runtime_helper")]
    pub(crate) async fn retain_quorum_initial_request(
        &self,
        issued: &IssuedEnrollmentManifestBinding,
        final_inventory: &crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<
            '_,
            '_,
        >,
        approved: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        signature: Vec<u8>,
    ) -> AgentResult<()> {
        super::invitation::retain_quorum_initial_request(
            self.effects.as_ref(),
            issued,
            final_inventory,
            approved,
            signature,
        )
        .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentFinalVerifierInventoryCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn preview_owned_enrollment_transport(
        &self,
        reserved: &super::invitation::ReservedInvitationIssuance,
        final_inventory: &crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<
            '_,
            '_,
        >,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
        invitation_type: InvitationType,
    ) -> AgentResult<(
        Invitation,
        aura_invitation::shareable::PublicEnrollmentTransportSigningIntent,
        ShareableInvitationTransportMetadata,
    )> {
        final_inventory.require_manifest(self.effects.as_ref(), manifest)?;
        let invitation = self.handler.preview_reserved_device_enrollment(
            self.effects.as_ref(),
            reserved,
            invitation_type,
        )?;
        Self::require_quorum_invitation_binding(&invitation, manifest)?;
        let transport = self.sender_transport_metadata();
        if transport.sender_device_id != Some(manifest.initiator_device) {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        }
        let shareable = ShareableInvitation::from(&invitation)
            .with_enrollment_quorum_transcript()
            .map_err(|source| {
                AgentError::EnrollmentManifest(
                    aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(
                        Box::new(source),
                    ),
                )
            })?;
        let public =
            aura_invitation::shareable::PublicEnrollmentTransportSigningIntent::from_invitation(
                &shareable, &transport,
            )
            .map_err(|source| {
                AgentError::EnrollmentManifest(
                    aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(
                        Box::new(source),
                    ),
                )
            })?;
        aura_invitation::shareable::require_transport_manifest(&public, manifest).map_err(
            |source| {
                AgentError::EnrollmentManifest(
                    aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(
                        Box::new(source),
                    ),
                )
            },
        )?;
        Ok((invitation, public, transport))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentFinalVerifierInventoryCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn export_quorum_enrollment_manifest(
        &self,
        reserved: &super::invitation::ReservedInvitationIssuance,
        selected_setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        final_inventory: &crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<
            '_,
            '_,
        >,
        manifest: aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
        signature: Vec<u8>,
    ) -> AgentResult<(
        aura_app::runtime_bridge::EnrollmentManifestTransferCodes,
        IssuedEnrollmentManifestBinding,
    )> {
        use aura_invitation::enrollment_manifest::{
            encode_initiator_verifier_transfer, EnrollmentManifestError,
            SignedEnrollmentTrustManifest,
        };
        final_inventory.require_manifest(self.effects.as_ref(), &manifest)?;
        manifest
            .validate_setup_validity(selected_setup.statement())
            .map_err(AgentError::EnrollmentManifest)?;
        if !reserved.owns_effects(self.effects.as_ref())
            || reserved.issuer_binding() != (manifest.subject, manifest.initiator_device)
            || manifest.invitation != *reserved.invitation_id()
            || manifest.subject != self.handler.authority_context().authority_id()
            || manifest.initiator_device != self.effects.device_id()
            || manifest.setup.nonce != selected_setup.statement().nonce
            || manifest.setup.digest != selected_setup.digest()
            || manifest.invitee_authority != selected_setup.statement().authority
            || manifest.invitee_device != selected_setup.statement().device
        {
            return Err(AgentError::invalid(
                "quorum manifest original reservation/setup differs",
            ));
        }
        // The expected group key is derived from original independently held
        // native inventory, never supplied by remote approval/signature fields.
        let root = final_inventory
            .inventory()
            .iter()
            .find(|entry| entry.signing_node == aura_core::tree::NodeIndex(0))
            .ok_or_else(|| AgentError::invalid("original quorum inventory lacks root"))?;
        if root.mode != aura_core::effects::crypto::SigningMode::Threshold || root.threshold < 2 {
            return Err(AgentError::invalid(
                "quorum export requires genuine retained threshold policy",
            ));
        }
        let native = frost_ed25519::keys::PublicKeyPackage::deserialize(&root.public_key_package)
            .map_err(|source| {
            AgentError::from(aura_core::AuraError::crypto_with_source(
                "decode original native quorum verifier",
                Arc::new(source),
            ))
        })?;
        let public = native.verifying_key().serialize().to_vec();
        if public != manifest.initiator_confirmation_verifier {
            return Err(AgentError::invalid(
                "manifest verifier differs from original native quorum key",
            ));
        }
        let trusted_key = aura_core::TrustedPublicKey::active(
            aura_core::TrustedKeyDomain::AuthorityThreshold,
            Some(manifest.final_epoch),
            public.clone(),
            aura_core::Hash32(aura_core::hash::hash(&public)),
        );
        if !verify_original_quorum_transcript(
            self.effects.as_ref(),
            &manifest,
            &signature,
            &trusted_key,
        )
        .await?
        {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        }
        let baseline = self
            .effects
            .export_tree_ops()
            .await?
            .iter()
            .map(aura_core::util::serialization::to_vec)
            .collect::<Result<Vec<_>, _>>()
            .map_err(aura_core::AuraError::from)?;
        let checked = manifest
            .clone()
            .verify_signature(self.effects.as_ref(), &public, &signature)
            .await
            .map_err(|source| {
                AgentError::EnrollmentManifest(EnrollmentManifestError::Runtime(Box::new(
                    EnrollmentManifestExportValidationError::Signature(source),
                )))
            })?
            .verify_baseline(&baseline)
            .map_err(|source| {
                AgentError::EnrollmentManifest(EnrollmentManifestError::Runtime(Box::new(
                    EnrollmentManifestExportValidationError::Baseline(source),
                )))
            })?;
        let digest = checked.manifest_digest();
        let manifest_code = SignedEnrollmentTrustManifest {
            manifest: manifest.clone(),
            signature,
        }
        .encode()
        .map_err(AgentError::EnrollmentManifest)?;
        let initiator_verifier_code = encode_initiator_verifier_transfer(
            manifest.subject,
            manifest.initiator_device,
            &public,
        )
        .map_err(AgentError::EnrollmentManifest)?;
        let binding = IssuedEnrollmentManifestBinding {
            runtime_owner: self.effects.clone(),
            manifest,
            digest,
            signed_code: manifest_code.clone(),
            confirmation_verifier: public,
        };
        Ok((
            aura_app::runtime_bridge::EnrollmentManifestTransferCodes {
                manifest_code,
                initiator_verifier_code,
            },
            binding,
        ))
    }

    fn require_quorum_invitation_binding(
        invitation: &Invitation,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> AgentResult<()> {
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
        } = &invitation.invitation_type
        else {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        };
        let baseline = aura_core::util::serialization::to_vec(baseline_tree_ops)
            .map_err(aura_core::AuraError::from)?;
        if invitation.invitation_id != manifest.invitation
            || invitation.sender_id != manifest.subject
            || invitation.receiver_id != manifest.invitee_authority
            || *subject_authority != manifest.subject
            || *invitee_authority != manifest.invitee_authority
            || *initiator_device_id != manifest.initiator_device
            || *device_id != manifest.invitee_device
            || *ceremony_id != manifest.ceremony
            || *pending_epoch != manifest.pending_epoch
            || setup_binding != &manifest.setup
            || aura_core::hash::hash(key_package) != manifest.pending_share_digest
            || aura_core::hash::hash(public_key_package)
                != manifest.pending_public_key_package_digest
            || aura_core::Hash32::from_bytes(threshold_config)
                != manifest.pending_threshold_config_digest
            || baseline_tree_ops.len() != manifest.baseline_count as usize
            || aura_core::hash::hash(&baseline) != manifest.baseline_digest
        {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "issued", capability_type = IssuedEnrollmentManifestBinding, family = "runtime_helper")]
    pub(crate) async fn export_quorum_enrollment_invitation(
        &self,
        invitation: &Invitation,
        issued: &IssuedEnrollmentManifestBinding,
        approved: &aura_invitation::shareable::PublicEnrollmentTransportSigningIntent,
        frozen_transport: &ShareableInvitationTransportMetadata,
        signature: Vec<u8>,
    ) -> AgentResult<String> {
        use aura_invitation::enrollment_manifest::EnrollmentManifestError;
        use aura_signature::SecurityTranscript;
        issued.require_effects(self.effects.as_ref())?;
        Self::require_quorum_invitation_binding(invitation, issued.manifest())?;
        aura_invitation::shareable::require_transport_manifest(approved, issued.manifest())
            .map_err(|source| {
                AgentError::EnrollmentManifest(EnrollmentManifestError::Runtime(Box::new(source)))
            })?;
        if approved.transport_metadata() != frozen_transport
            || frozen_transport.sender_device_id != Some(issued.manifest().initiator_device)
        {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        }
        let shareable = ShareableInvitation::from(invitation)
            .with_enrollment_quorum_transcript()
            .map_err(|source| {
                AgentError::EnrollmentManifest(EnrollmentManifestError::Runtime(Box::new(source)))
            })?;
        let actual = shareable
            .signing_transcript_with_transport(frozen_transport)
            .required_transcript_bytes()
            .map_err(|source| {
                AgentError::from(aura_core::AuraError::crypto_with_source(
                    "encode original quorum transport",
                    Arc::new(source),
                ))
            })?;
        let selected = approved.required_transcript_bytes().map_err(|source| {
            AgentError::from(aura_core::AuraError::crypto_with_source(
                "encode approved quorum transport",
                Arc::new(source),
            ))
        })?;
        let trusted_key = aura_core::TrustedPublicKey::active(
            aura_core::TrustedKeyDomain::AuthorityThreshold,
            Some(issued.manifest().final_epoch),
            issued.confirmation_verifier().to_vec(),
            aura_core::Hash32(aura_core::hash::hash(issued.confirmation_verifier())),
        );
        if actual != selected
            || !verify_original_quorum_transcript(
                self.effects.as_ref(),
                approved,
                &signature,
                &trusted_key,
            )
            .await?
        {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        }
        let code = shareable
            .to_signed_code_with_transport(
                ShareableInvitationSenderProof {
                    scheme: ShareableInvitation::SENDER_PROOF_SCHEME.into(),
                    public_key: issued.confirmation_verifier().to_vec(),
                    signature,
                    sender_device_id: frozen_transport.sender_device_id,
                    key_epoch: Some(issued.manifest().final_epoch),
                },
                frozen_transport.clone(),
            )
            .map_err(|source| {
                AgentError::EnrollmentManifest(EnrollmentManifestError::Runtime(Box::new(source)))
            })?;
        Ok(self.append_sender_hint(code, frozen_transport))
    }
}

/// Retains the original choreography failure and a distinct settlement failure.
/// The standard source chain follows the original concrete execution cause.
#[derive(Debug)]
pub(crate) struct EnrollmentInitiatorTaskFailure {
    execution: AgentError,
    terminal_publication: Option<aura_core::AuraError>,
}
impl EnrollmentInitiatorTaskFailure {
    pub(crate) fn terminal_publication_error(&self) -> Option<&aura_core::AuraError> {
        self.terminal_publication.as_ref()
    }
}
impl std::fmt::Display for EnrollmentInitiatorTaskFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "device enrollment initiator failed: {}",
            self.execution
        )?;
        if let Some(secondary) = self.terminal_publication_error() {
            write!(formatter, "; terminal publication also failed: {secondary}")?;
        }
        Ok(())
    }
}
impl std::error::Error for EnrollmentInitiatorTaskFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.execution)
    }
}
async fn settle_required_enrollment_initiator_failure(
    runner: &CeremonyRunner,
    ceremony: &CeremonyId,
    execution: AgentError,
) -> aura_core::AuraError {
    let reason = if execution.is_timeout() {
        aura_app::runtime_bridge::CeremonyFailureReason::TimedOut
    } else {
        aura_app::runtime_bridge::CeremonyFailureReason::ChoreographyFailed
    };
    // A committed cancellation is already this ceremony's terminal outcome;
    // a later execution failure (e.g. cancelled-generation cleanup) must not
    // publish a contradictory second terminal.
    let already_terminal = matches!(runner.terminal_outcome(ceremony).await, Ok(Some(_)));
    let terminal_publication = if already_terminal {
        None
    } else {
        runner
            .fail_with_reason(ceremony, reason, Some(execution.to_string()))
            .await
            .err()
    };
    let failure = EnrollmentInitiatorTaskFailure {
        execution,
        terminal_publication,
    };
    aura_core::AuraError::Internal {
        message: failure.to_string(),
        source: Some(Arc::new(failure)),
    }
}

/// The original registered owner retains protocol observation and signed result
/// through every borrowed execution/cleanup future. No receipt lives only in a
/// disposable async frame after it is delivered by the protocol child.
#[derive(Default)]
struct OriginalIssuerTerminalState {
    terminal: Option<super::invitation::IssuerEnrollmentTerminalReceipt>,
    cancelled: Option<Arc<super::invitation::OriginalIssuerCancellationTerminal>>,
    execution_failure: Option<aura_core::AuraError>,
    observation_failure: Option<aura_core::AuraError>,
    protocol_failure: Option<aura_core::AuraError>,
}
pub(crate) struct PreparedOriginalIssuerHandoff {
    pub(crate) running:
        crate::runtime::services::ceremony_tracker::PreparedRunningIssuerObservation,
    pub(crate) publication_sender: tokio::sync::oneshot::Sender<()>,
    pub(crate) initialized_receiver:
        tokio::sync::oneshot::Receiver<Result<(), aura_core::AuraError>>,
    pub(crate) admission_observer:
        crate::runtime::services::enrollment_window::HeldIssuerCompletionObserver,
}

pub(crate) struct OriginalIssuerTaskCustody {
    issuer_resource_cleanup:
        Option<crate::runtime::services::enrollment_window::OriginalIssuerResourceCleanupOwner>,
    conditional_signing: Option<
        Arc<
            tokio::sync::Mutex<
                crate::runtime::services::threshold_signing::OriginalIssuerTranscriptSigningCustody,
            >,
        >,
    >,
    registration_custody:
        Option<crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'static>>,
    held_negative:
        Option<crate::runtime::services::enrollment_window::OriginalHeldIssuerNegativeCleanupOwner>,
    finalizer: Option<crate::handlers::device_epoch_rotation::DeviceEpochRotationService>,
    issued: Option<Arc<super::invitation::enrollment_trust::RetainedEnrollmentVmControl>>,
    original: Option<
        crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
    >,
    negative:
        Option<crate::runtime::services::enrollment_window::OriginalIssuerNegativeCleanupOwner>,
    negative_acknowledged:
        Option<crate::runtime::services::enrollment_window::AcknowledgedIssuerNegativeTerminal>,
    handler: InvitationHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: Option<CeremonyRunner>,
    invitation: Invitation,
    terminal_receiver: Option<tokio::sync::oneshot::Receiver<()>>,
    terminal_destination: Arc<OriginalIssuerTerminalDestination>,
    terminal_state: OriginalIssuerTerminalState,
    acknowledged:
        Option<crate::runtime::services::enrollment_window::AcknowledgedIssuerEnrollmentWindow>,
    admission: Option<tokio::sync::oneshot::Receiver<()>>,
    initialized: Option<tokio::sync::oneshot::Sender<Result<(), aura_core::AuraError>>>,
    entered: bool,
    acknowledgment_retained: bool,
}
impl OriginalIssuerTaskCustody {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalIssuerTaskCustody",
        receiver_type = OriginalIssuerTaskCustody,
        family = "runtime_helper"
    )]
    pub(crate) fn original_enrollment_rotation_origin(
        &self,
    ) -> Result<
        crate::handlers::device_epoch_rotation::DeviceEpochRotationOrigin,
        aura_core::AuraError,
    > {
        if let Some(source) = self.terminal_state.execution_failure.as_ref() {
            return Err(source.clone());
        }
        if !self.entered || self.initialized.is_some() || self.registration_custody.is_some() {
            return Err(aura_core::AuraError::permission_denied(
                "original enrollment rotation requires initialized issuer custody",
            ));
        }
        let root = self.original.as_ref().ok_or_else(|| {
            aura_core::AuraError::permission_denied("original issuer root is consumed")
        })?;
        let issued = self.issued.as_ref().ok_or_else(|| {
            aura_core::AuraError::permission_denied("original issuer control is missing")
        })?;
        issued.require_runtime_owner(self.effects.as_ref())?;
        root.child().require_issued_notice_owner(issued)?;
        issued.original_conditional_signing_required(self.effects.as_ref(), root.child())?;
        Ok(
            crate::handlers::device_epoch_rotation::DeviceEpochRotationOrigin::Enrollment(
                crate::handlers::device_epoch_rotation::OriginalEnrollmentRotationContext {
                    issued: Arc::clone(issued),
                    original: root.owned_execution_child(),
                    children: root.children().clone(),
                },
            ),
        )
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerTaskCustody", receiver_type = OriginalIssuerTaskCustody, family = "runtime_helper")]
    pub(crate) fn retain_conditional_signing(
        &mut self,
        original: Arc<
            tokio::sync::Mutex<
                crate::runtime::services::threshold_signing::OriginalIssuerTranscriptSigningCustody,
            >,
        >,
    ) -> Result<(), aura_core::AuraError> {
        if self.conditional_signing.is_some() || self.entered {
            return Err(aura_core::AuraError::permission_denied(
                "original conditional signing handoff already entered",
            ));
        }
        original
            .try_lock()
            .map_err(|source| aura_core::AuraError::Internal {
                message: "original conditional signing state is borrowed".into(),
                source: Some(Arc::new(source)),
            })?
            .require_initial_transfer()?;
        self.conditional_signing = Some(original);
        Ok(())
    }
    pub(crate) fn retain_registration_custody(
        &mut self,
        original: &mut Option<
            crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'static>,
        >,
    ) -> Result<(), aura_core::AuraError> {
        if self.registration_custody.is_some() || original.is_none() {
            return Err(aura_core::AuraError::invalid(
                "original registration custody transfer refused",
            ));
        }
        self.registration_custody = original.take();
        Ok(())
    }

    async fn settle_original_negative(
        &mut self,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        if self.negative_acknowledged.is_none() {
            if self.negative.is_none() {
                let terminal = self
                    .terminal_state
                    .cancelled
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("actual original cancellation is missing")
                    })?
                    .clone();
                let root = self.original.take().ok_or_else(|| {
                    aura_core::AuraError::invalid("original issuer root is already consumed")
                })?;
                // Synchronous transfer: no await or fallible operation occurs
                // between removing the root and retaining its negative owner.
                self.negative = Some(crate::runtime::services::enrollment_window::OriginalIssuerNegativeCleanupOwner::from_original(
                    root, terminal, Arc::clone(&self.terminal_destination), self.conditional_signing.as_ref().map(Arc::clone),
                ));
            }
            self.negative
                .as_mut()
                .ok_or_else(|| aura_core::AuraError::invalid("negative original owner is missing"))?
                .settle(&self.effects)
                .await?;
            let owner = self
                .negative
                .take()
                .ok_or_else(|| aura_core::AuraError::invalid("negative original owner was lost"))?;
            match owner.into_acknowledged() {
                Ok(acknowledged) => self.negative_acknowledged = Some(acknowledged),
                Err(owner) => {
                    self.negative = Some(*owner);
                    return Err(aura_core::AuraError::invalid(
                        "negative original terminal is unresolved",
                    ));
                }
            }
        }
        let acknowledged = self.negative_acknowledged.as_ref().ok_or_else(|| {
            aura_core::AuraError::invalid("actual negative terminal acknowledgment is missing")
        })?;
        self.ceremony_runner = None;
        self.finalizer = None;
        Ok(
            aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                self.terminal_state
                    .execution_failure
                    .clone()
                    .or_else(|| self.terminal_state.protocol_failure.clone())
                    .or_else(|| self.terminal_state.observation_failure.clone())
                    .unwrap_or_else(|| acknowledged.domain_failure()),
            ),
        )
    }
    fn fail_initialization(&mut self, source: aura_core::AuraError) -> aura_core::AuraError {
        let original = self
            .terminal_state
            .execution_failure
            .get_or_insert(source)
            .clone();
        if let Some(initialized) = self.initialized.take() {
            let _ = initialized.send(Err(original.clone()));
        }
        original
    }
    async fn observe_original_terminal(&mut self) -> Result<(), aura_core::AuraError> {
        if let Some(source) = self.terminal_destination.retained_finalizer_failure() {
            self.terminal_state.execution_failure.get_or_insert(source);
        }
        if let Some(source) = self.terminal_state.execution_failure.as_ref() {
            self.terminal_receiver = None;
            return Err(source.clone());
        }
        if self.terminal_state.terminal.is_some()
            || self.terminal_state.cancelled.is_some()
            || self.acknowledged.is_some()
            || self.acknowledgment_retained
        {
            return Ok(());
        }
        if self.terminal_destination.result.get().is_some() {
            return capture_original_issuer_terminal(
                &self.terminal_destination,
                &mut self.terminal_state,
            );
        }
        if let Some(source) = self.terminal_state.observation_failure.as_ref() {
            return Err(source.clone());
        }
        let receiver = self.terminal_receiver.as_mut().ok_or_else(|| {
            aura_core::AuraError::invalid("original issuer protocol has not been admitted")
        })?;
        let destination = &self.terminal_destination;
        let terminal_state = &mut self.terminal_state;
        let observed = self
            .original
            .as_ref()
            .ok_or_else(|| {
                aura_core::AuraError::invalid("positive original issuer root is consumed")
            })?
            .child()
            .execute(self.effects.as_ref(), || async {
                let readiness = tokio::select! {
                    biased;
                    source = destination.await_finalizer_failure() => {
                        terminal_state.execution_failure.get_or_insert_with(|| source.clone());
                        return Err(AgentError::from(source));
                    }
                    result = receiver => result,
                };
                if let Some(source) = destination.retained_finalizer_failure() {
                    terminal_state
                        .execution_failure
                        .get_or_insert_with(|| source.clone());
                    return Err(AgentError::from(source));
                }
                if let Err(source) = readiness {
                    let source = aura_core::AuraError::Internal {
                        message: "original issuer terminal readiness observation failed".into(),
                        source: Some(Arc::new(source)),
                    };
                    terminal_state
                        .observation_failure
                        .get_or_insert_with(|| source.clone());
                    return Err(AgentError::from(source));
                }
                // Unit readiness conveys no receipt authority. Move only the exact
                // result already retained by this custodian's original destination,
                // and store it BEFORE Ready can lose the outer bounded race.
                capture_original_issuer_terminal(destination, terminal_state)
                    .map_err(AgentError::from)
            })
            .await;
        if self.terminal_state.terminal.is_some()
            || self.terminal_state.execution_failure.is_some()
            || self.terminal_state.observation_failure.is_some()
            || self.terminal_state.protocol_failure.is_some()
        {
            self.terminal_receiver = None;
        }
        observed.map_err(|source| match source {
            aura_core::TimeoutRunError::Timeout(source) => aura_core::AuraError::from(source),
            aura_core::TimeoutRunError::Operation(source) => aura_core::AuraError::Internal {
                message: "original issuer terminal result".into(),
                source: Some(Arc::new(source)),
            },
        })
    }
    async fn retain_original_ack(&mut self) -> Result<(), aura_core::AuraError> {
        if self.acknowledgment_retained {
            return Ok(());
        }
        if self.acknowledged.is_none() {
            self.original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .acknowledge_issuer_terminal(
                    self.effects.as_ref(),
                    &mut self.terminal_state.terminal,
                    &mut self.acknowledged,
                )
                .await?;
        }
        self.original
            .as_ref()
            .ok_or_else(|| {
                aura_core::AuraError::invalid("positive original issuer root is consumed")
            })?
            .retain_issuer_acknowledgment(&mut self.acknowledged)?;
        self.acknowledgment_retained = true;
        Ok(())
    }
    pub(crate) async fn initialize_original(&mut self) -> Result<(), aura_core::AuraError> {
        if !self.entered {
            let Some(admission) = self.admission.as_mut() else {
                return Err(self.fail_initialization(aura_core::AuraError::invalid(
                    "original issuer publication observation is missing",
                )));
            };
            let publication = self
                .original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .child()
                .execute(self.effects.as_ref(), || async {
                    admission
                        .await
                        .map_err(|source| aura_core::AuraError::Internal {
                            message: "observe original issuer entry publication".into(),
                            source: Some(Arc::new(source)),
                        })
                })
                .await
                .map_err(|source| match source {
                    aura_core::TimeoutRunError::Timeout(source) => {
                        aura_core::AuraError::from(source)
                    }
                    aura_core::TimeoutRunError::Operation(source) => source,
                });
            if let Err(source) = publication {
                return Err(self.fail_initialization(source));
            }
            self.admission = None;
            self.entered = true;
            if self.issued.is_none() {
                let original_child = self
                    .original
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("positive original issuer root is consumed")
                    })?
                    .child();
                let loaded = original_child
                    .execute(self.effects.as_ref(), || async {
                        super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load(
                            self.effects.clone(),
                            &self.invitation,
                        )
                        .await
                    })
                    .await
                    .map_err(|source| {
                        original_child.map_run_error("retain original finalizer control", source)
                    });
                match loaded {
                    Ok(issued) => self.issued = Some(Arc::new(issued)),
                    Err(source) => {
                        self.terminal_state
                            .execution_failure
                            .get_or_insert_with(|| aura_core::AuraError::Internal {
                                message: "retain original finalizer control failed".into(),
                                source: Some(Arc::new(source)),
                            });
                    }
                }
            }
            if let Some(issued) = self.issued.as_ref() {
                if let Err(source) = self
                    .original
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("positive original issuer root is consumed")
                    })?
                    .child()
                    .bind_registered_notice_control(issued)
                    .and_then(|_| {
                        self.original
                            .as_ref()
                            .ok_or_else(|| {
                                aura_core::AuraError::invalid(
                                    "positive original issuer root is consumed",
                                )
                            })?
                            .child()
                            .require_issued_notice_owner(issued)
                    })
                {
                    self.terminal_state.execution_failure.get_or_insert(source);
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                if let (Some(service), Some(issued)) =
                    (self.finalizer.as_ref(), self.issued.as_ref())
                {
                    match service
                        .requires_original_sole_finalization(
                            issued,
                            self.original
                                .as_ref()
                                .ok_or_else(|| {
                                    aura_core::AuraError::invalid(
                                        "positive original issuer root is consumed",
                                    )
                                })?
                                .child(),
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            self.finalizer = None;
                        }
                        Err(source) => {
                            self.terminal_state
                                .execution_failure
                                .get_or_insert_with(|| aura_core::AuraError::Internal {
                                    message: "select original finalizer role failed".into(),
                                    source: Some(Arc::new(source)),
                                });
                        }
                    }
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                if let Some(registration) = self.registration_custody.as_mut() {
                    if let (Some(signing), Some(issued), Some(root)) = (
                        self.conditional_signing.as_ref(),
                        self.issued.as_ref(),
                        self.original.as_ref(),
                    ) {
                        if let Err(source) = issued
                            .bind_original_conditional_signing(Arc::clone(signing), root.child())
                        {
                            self.terminal_state.execution_failure.get_or_insert(source);
                        }
                    }
                    let result = if let Some(source) =
                        self.terminal_state.execution_failure.as_ref()
                    {
                        Err(source.clone())
                    } else {
                        match (self.original.as_ref(), self.issued.as_ref()) {
                            (Some(root), Some(issued)) => registration
                                .release_original_registration(self.effects.as_ref(), root, issued),
                            _ => Err(aura_core::AuraError::invalid(
                                "actual original control is missing at registration transfer",
                            )),
                        }
                    };
                    if let Err(source) = result {
                        self.terminal_state.execution_failure.get_or_insert(source);
                    } else {
                        self.registration_custody = None;
                    }
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                if let Some(service) = self.finalizer.take() {
                    let original = self
                        .original
                        .as_ref()
                        .ok_or_else(|| {
                            aura_core::AuraError::invalid(
                                "positive original issuer root is consumed",
                            )
                        })?
                        .owned_execution_child();
                    let Some(issued) = self.issued.as_ref().cloned() else {
                        return Err(aura_core::AuraError::invalid(
                            "retained original finalizer control is missing",
                        ));
                    };
                    let runner = self.ceremony_runner.as_ref().cloned().ok_or_else(|| {
                        aura_core::AuraError::invalid("original issuer context already settled")
                    })?;
                    let effects = self.effects.clone();
                    let retained_destination = Arc::clone(&self.terminal_destination);
                    let finalize = async move {
                        #[cfg(test)]
                        eprintln!("original sole finalizer: entered required producer");
                        match service
                            .finalize_sole_device_enrollment(&issued, &original)
                            .await
                        {
                            Ok(()) => {
                                #[cfg(test)]
                                eprintln!("original sole finalizer: producer completed");
                                Ok(())
                            }
                            Err(source) => {
                                #[cfg(test)]
                                eprintln!("original sole finalizer: native failure {source:?}");
                                let primary = aura_core::AuraError::Internal {
                                    message: "original issuer finalization failed".into(),
                                    source: Some(Arc::new(source)),
                                };
                                retained_destination.retain_finalizer_failure(primary.clone());
                                let publication = original
                                    .execute(effects.as_ref(), || async {
                                        Ok::<_, AgentError>(
                                            settle_required_enrollment_initiator_failure(
                                                &runner,
                                                &issued.manifest().ceremony,
                                                AgentError::from(primary.clone()),
                                            )
                                            .await,
                                        )
                                    })
                                    .await;
                                match publication {
                                    Ok(source) => Err(source),
                                    Err(source) => Err(aura_core::AuraError::Internal {
                                        message:
                                            "original finalizer failure publication incomplete"
                                                .into(),
                                        source: Some(Arc::new(EnrollmentInitiatorTaskFailure {
                                            execution: AgentError::from(primary),
                                            terminal_publication: Some(
                                                match original.map_run_error(
                                                    "original finalizer failure publication",
                                                    source,
                                                ) {
                                                    AgentError::Aura(source) => source,
                                                    source => aura_core::AuraError::Internal {
                                                        message:
                                                            "original finalizer failure publication"
                                                                .into(),
                                                        source: Some(Arc::new(source)),
                                                    },
                                                },
                                            ),
                                        })),
                                    }),
                                }
                            }
                        }
                    };
                    cfg_if::cfg_if! {
                        if #[cfg(target_arch = "wasm32")] {
                            let admitted = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_local_try_named_observed("original_issuer_finalizer", finalize);
                        } else {
                            let admitted = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_try_named_observed("original_issuer_finalizer", finalize);
                        }
                    }
                    if let Err(source) = admitted {
                        self.terminal_state
                            .execution_failure
                            .get_or_insert_with(|| aura_core::AuraError::Internal {
                                message: "original issuer finalizer admission failed".into(),
                                source: Some(Arc::new(source)),
                            });
                    }
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                let child = self
                    .original
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("positive original issuer root is consumed")
                    })?
                    .owned_execution_child();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                self.terminal_receiver = Some(receiver);
                let handler = self.handler.clone();
                let effects = self.effects.clone();
                let runner = self.ceremony_runner.as_ref().cloned().ok_or_else(|| {
                    aura_core::AuraError::invalid("original issuer context already settled")
                })?;
                let issued = Arc::clone(self.issued.as_ref().ok_or_else(|| {
                    aura_core::AuraError::invalid(
                        "original issuer control is missing at child admission",
                    )
                })?);
                let destination = Arc::clone(&self.terminal_destination);
                let execution = async move {
                    let result = handler
                        .execute_device_enrollment_initiator_owned(
                            effects,
                            issued,
                            runner,
                            child,
                            Arc::clone(&destination),
                        )
                        .await;
                    let (result, failure) = match result {
                        Ok(terminal) => (Ok(terminal), None),
                        Err(source) => {
                            let failure = aura_core::AuraError::Internal {
                                message: "original issuer enrollment child failed".into(),
                                source: Some(Arc::new(source)),
                            };
                            (Err(AgentError::from(failure.clone())), Some(failure))
                        }
                    };
                    // The original destination retains the move-owned result before
                    // signalling. A lost observer cannot destroy signed custody.
                    destination.publish(result)?;
                    sender
                        .send(())
                        .map_err(|_| aura_core::AuraError::PermissionDenied {
                            message: "original issuer terminal observer was lost".into(),
                            source: Some(Arc::new(
                                super::invitation::EnrollmentVmAdmissionError::TerminalObserverLost,
                            )),
                        })?;
                    match failure {
                        Some(source) => Err(source),
                        None => Ok(()),
                    }
                };
                cfg_if::cfg_if! {
                    if #[cfg(target_arch = "wasm32")] {
                        let child_admission = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_local_try_named_observed("original_issuer_protocol", execution);
                    } else {
                        let child_admission = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_try_named_observed("original_issuer_protocol", execution);
                    }
                }
                if let Err(source) = child_admission {
                    self.terminal_state
                        .execution_failure
                        .get_or_insert_with(|| aura_core::AuraError::Internal {
                            message: "original issuer protocol admission failed".into(),
                            source: Some(Arc::new(source)),
                        });
                }
            }
            if let Some(initialized) = self.initialized.take() {
                let actual_admission = self
                    .terminal_state
                    .execution_failure
                    .as_ref()
                    .map_or(Ok(()), |source| Err(source.clone()));
                if initialized.send(actual_admission).is_err() {
                    self.terminal_state
                        .observation_failure
                        .get_or_insert_with(|| {
                            aura_core::AuraError::invalid(
                                "original issuer initialization observer was lost",
                            )
                        });
                }
            }
        }
        Ok(())
    }
    pub(crate) async fn execute_original(&mut self) -> Result<(), aura_core::AuraError> {
        self.initialize_original().await?;
        let terminal = match self.terminal_state.execution_failure.as_ref() {
            Some(source) => Err(source.clone()),
            None => self.observe_original_terminal().await,
        };
        if self.terminal_state.cancelled.is_some() {
            return terminal;
        }
        if let Err(source) = terminal {
            if let Some(original) = self.terminal_destination.finalizer_failure.get() {
                self.terminal_state
                    .execution_failure
                    .get_or_insert_with(|| original.clone());
            }
            self.terminal_state
                .execution_failure
                .get_or_insert_with(|| source.clone());
            let ceremony = match &self.invitation.invitation_type {
                InvitationType::DeviceEnrollment { ceremony_id, .. } => ceremony_id,
                _ => return Err(source),
            };
            let execution = AgentError::from(source.clone());
            let published = self
                .original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .child()
                .execute(self.effects.as_ref(), || async {
                    Ok::<_, AgentError>(
                        settle_required_enrollment_initiator_failure(
                            self.ceremony_runner.as_ref().ok_or_else(|| {
                                aura_core::AuraError::invalid(
                                    "original issuer context already settled",
                                )
                            })?,
                            ceremony,
                            execution,
                        )
                        .await,
                    )
                })
                .await;
            return Err(match published {
                Ok(failure) => failure,
                Err(publication_source) => {
                    let publication = self
                        .original
                        .as_ref()
                        .ok_or_else(|| {
                            aura_core::AuraError::invalid(
                                "positive original issuer root is consumed",
                            )
                        })?
                        .child()
                        .map_run_error("publish original issuer failure", publication_source);
                    let publication = match publication {
                        AgentError::Aura(source) => source,
                        source => aura_core::AuraError::Internal {
                            message: "original issuer terminal publication failed".into(),
                            source: Some(Arc::new(source)),
                        },
                    };
                    aura_core::AuraError::Internal {
                        message: "original issuer failure publication was incomplete".into(),
                        source: Some(Arc::new(EnrollmentInitiatorTaskFailure {
                            execution: AgentError::from(source),
                            terminal_publication: Some(publication),
                        })),
                    }
                }
            });
        }
        self.retain_original_ack().await
    }
    pub(crate) async fn cleanup_original(
        &mut self,
        exit: &aura_core::effects::task::RequiredTaskExit,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        if self.registration_custody.is_some() || self.held_negative.is_some() {
            if self.held_negative.is_none() {
                let primary = self
                    .terminal_state
                    .execution_failure
                    .clone()
                    .unwrap_or_else(|| match exit {
                        aura_core::effects::task::RequiredTaskExit::Failed(source) => {
                            source.clone()
                        }
                        _ => aura_core::AuraError::Internal {
                            message: "original registration handoff was interrupted".into(),
                            source: Some(Arc::new(OriginalIssuerInitializationIncomplete {
                                exit: exit.clone(),
                            })),
                        },
                    });
                if self.original.is_none() {
                    return Err(aura_core::AuraError::invalid("original held root missing"));
                }
                self.fail_initialization(primary.clone());
                let mut reservation = None;
                crate::runtime::effects::RegisteredEnrollmentGenerationCapability::take_original_registration(
                    &mut self.registration_custody, self.effects.as_ref(), &mut reservation,
                )?;
                let root = self
                    .original
                    .take()
                    .ok_or_else(|| aura_core::AuraError::invalid("original held root missing"))?;
                self.held_negative = Some(crate::runtime::services::enrollment_window::OriginalHeldIssuerNegativeCleanupOwner::from_original(
                    root, reservation.ok_or_else(|| aura_core::AuraError::invalid("original registration reservation missing"))?, primary,
                ));
            }
            let original = self.held_negative.as_mut().ok_or_else(|| {
                aura_core::AuraError::invalid("original held negative owner missing")
            })?;
            original.settle(&self.effects).await?;
            let settlement = original.settlement()?;
            self.ceremony_runner = None;
            return Ok(settlement);
        }
        // A protected Committed decision cannot be rewritten as cancellation.
        // Interrupted producer cleanup closes only its original resources.
        if self.issuer_resource_cleanup.is_some()
            || (self.terminal_state.terminal.is_none()
                && self.acknowledged.is_none()
                && !self.acknowledgment_retained
                && self
                    .original
                    .as_ref()
                    .zip(self.issued.as_ref())
                    .is_some_and(|(root, issued)| {
                        crate::runtime::services::enrollment_window::OriginalIssuerResourceDecision::from_original(issued, root.child()).is_ok()
                    }))
        {
            if self.issuer_resource_cleanup.is_none() {
                let primary = self
                    .terminal_state
                    .execution_failure
                    .clone()
                    .unwrap_or_else(|| match exit {
                        aura_core::effects::task::RequiredTaskExit::Failed(source) => {
                            source.clone()
                        }
                        _ => aura_core::AuraError::Internal {
                            message: "original committed issuer was interrupted".into(),
                            source: Some(Arc::new(OriginalIssuerInitializationIncomplete {
                                exit: exit.clone(),
                            })),
                        },
                    });
                let root = self.original.take().ok_or_else(|| {
                    aura_core::AuraError::invalid("committed original root missing")
                })?;
                let issued = Arc::clone(self.issued.as_ref().ok_or_else(|| {
                    aura_core::AuraError::invalid("committed original control missing")
                })?);
                match crate::runtime::services::enrollment_window::OriginalIssuerResourceCleanupOwner::from_original(root, issued, self.conditional_signing.clone(), primary) {
                    Ok(owner) => self.issuer_resource_cleanup = Some(owner),
                    Err(rejected) => { let (root, source) = *rejected; self.original = Some(root); return Err(source); }
                }
            }
            let settlement = self
                .issuer_resource_cleanup
                .as_mut()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("committed resource cleanup owner missing")
                })?
                .settle(&self.effects)
                .await?;
            self.ceremony_runner = None;
            self.finalizer = None;
            return Ok(settlement);
        }
        if let Some(cancelled) = self.terminal_destination.cancelled.get() {
            self.terminal_state
                .cancelled
                .get_or_insert_with(|| Arc::clone(cancelled));
        }
        if self.terminal_state.cancelled.is_some()
            || self.negative.is_some()
            || self.negative_acknowledged.is_some()
        {
            if self.terminal_destination.result.get().is_some() {
                if let Err(source) = capture_original_issuer_terminal(
                    &self.terminal_destination,
                    &mut self.terminal_state,
                ) {
                    self.terminal_state.execution_failure.get_or_insert(source);
                }
            }
            return self.settle_original_negative().await;
        }
        if self.initialized.is_some() {
            let source = match exit {
                aura_core::effects::task::RequiredTaskExit::Failed(source) => source.clone(),
                _ => aura_core::AuraError::Internal {
                    message: "original issuer initialization was interrupted".into(),
                    source: Some(Arc::new(OriginalIssuerInitializationIncomplete {
                        exit: exit.clone(),
                    })),
                },
            };
            self.fail_initialization(source);
        }
        if self.terminal_state.terminal.is_none()
            && self.acknowledged.is_none()
            && !self.acknowledgment_retained
            && self.terminal_destination.result.get().is_some()
        {
            capture_original_issuer_terminal(&self.terminal_destination, &mut self.terminal_state)?;
        }
        if self.terminal_receiver.is_none()
            && self.terminal_state.terminal.is_none()
            && !self.acknowledgment_retained
            && self.acknowledged.is_none()
        {
            if let Some(source) = self.terminal_state.execution_failure.as_ref() {
                return Err(aura_core::AuraError::Internal {
                    message: "original issuer cleanup has no signed terminal authority".into(),
                    source: Some(Arc::new(source.clone())),
                });
            }
        }
        // Complete the same signed terminal if one exists. A missing terminal
        // leaves the original durable Pending refusal; resource supervision is
        // never substituted for a negative domain acknowledgment.
        if !self.acknowledgment_retained
            && self.terminal_state.terminal.is_none()
            && self.acknowledged.is_none()
        {
            self.original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .children()
                .request_cancellation();
            self.observe_original_terminal().await?;
        }
        self.retain_original_ack().await?;
        self.ceremony_runner = None;
        self.finalizer = None;
        if let Some(source) = self.terminal_state.execution_failure.as_ref() {
            return Ok(
                aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                    source.clone(),
                ),
            );
        }
        Ok(match exit {
            aura_core::effects::task::RequiredTaskExit::Failed(source) => {
                aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                    source.clone(),
                )
            }
            _ => aura_core::effects::task::RequiredTaskSettlement::Resolved,
        })
    }
}
#[cfg(test)]
pub(crate) fn assert_original_single_signer_cancelled_shutdown_source(
    disposed: &aura_core::effects::task::RequiredTaskExit,
    shutdown: &crate::task_registry::TaskSupervisionError,
) {
    fn cause<'a, T: std::error::Error + 'static>(
        mut error: &'a (dyn std::error::Error + 'static),
    ) -> Option<&'a T> {
        loop {
            if let Some(actual) = error.downcast_ref::<T>() {
                return Some(actual);
            }
            error = error.source()?;
        }
    }
    let aura_core::effects::task::RequiredTaskExit::Failed(source) = disposed else {
        panic!("same cancelled issuer must retain failed domain disposition");
    };
    let terminal = cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(source)
        .expect("actual original terminal failure");
    assert!(matches!(
        terminal.execution(),
        aura_core::effects::task::RequiredTaskExit::Cancelled
    ));
    let original = cause::<OriginalIssuerInitializationIncomplete>(terminal.terminal())
        .expect("actual original cancellation initialization source");
    assert!(matches!(
        original.exit,
        aura_core::effects::task::RequiredTaskExit::Cancelled
    ));
    assert!(
        matches!(
            shutdown,
            crate::task_registry::TaskSupervisionError::TaskFailed { .. }
        ),
        "strict supervised issuer domain failure: {shutdown:?}"
    );
    let same_terminal = cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(shutdown)
        .is_some_and(|actual| std::ptr::eq(actual, terminal));
    let same_original = cause::<OriginalIssuerInitializationIncomplete>(shutdown)
        .is_some_and(|actual| std::ptr::eq(actual, original));
    assert!(
        same_terminal || same_original,
        "strict shutdown retains exact original terminal/native cause allocation"
    );
}
#[derive(Debug, thiserror::Error)]
#[error("original issuer initialization ended without acknowledgment: {exit:?}")]
struct OriginalIssuerInitializationIncomplete {
    exit: aura_core::effects::task::RequiredTaskExit,
}
#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for OriginalIssuerTaskCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::BoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original(exit))
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for OriginalIssuerTaskCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original(exit))
    }
}
#[derive(thiserror::Error)]
#[error("original issuer registration publication failed")]
struct UnpublishedIssuerTaskCustody {
    #[source]
    source: aura_core::AuraError,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    retained: parking_lot::Mutex<Option<crate::task_registry::OriginalRequiredTaskHandle>>,
}
impl std::fmt::Debug for UnpublishedIssuerTaskCustody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UnpublishedIssuerTaskCustody")
    }
}
#[derive(thiserror::Error)]
#[error("original issuer custody admission failed")]
struct RejectedIssuerTaskCustody<T: ?Sized + Send> {
    #[source]
    source: aura_core::AuraError,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    retained: parking_lot::Mutex<Box<T>>,
}
impl<T: ?Sized + Send> std::fmt::Debug for RejectedIssuerTaskCustody<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedIssuerTaskCustody")
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn retain_rejected_native_issuer(
    rejected: aura_core::effects::task::RejectedRequiredTask<
        dyn aura_core::effects::task::RequiredTaskCustody,
    >,
) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: "admit original issuer custody".into(),
        source: Some(Arc::new(RejectedIssuerTaskCustody {
            source: rejected.source,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
            retained: parking_lot::Mutex::new(rejected.obligation),
        })),
    })
}
#[cfg(target_arch = "wasm32")]
pub(crate) fn retain_rejected_local_issuer(
    rejected: aura_core::effects::task::RejectedRequiredTask<
        dyn aura_core::effects::task::LocalRequiredTaskCustody,
    >,
) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: "admit original local issuer custody".into(),
        source: Some(Arc::new(RejectedIssuerTaskCustody {
            source: rejected.source,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
            retained: parking_lot::Mutex::new(rejected.obligation),
        })),
    })
}

#[cfg(feature = "simulation")]
pub(crate) enum OriginalSimulationCleanupOutcome {
    Committed,
    Cancelled,
    Refused(crate::runtime::services::ceremony_tracker::OriginalSimulationCancellationFailure),
}
/// One original child writes one original custodian's result. This slot is
/// independent from diagnostic failure retention, never an addressable queue.
#[derive(Default)]
pub(crate) struct OriginalIssuerTerminalDestination {
    #[cfg(feature = "simulation")]
    simulated_subtree_disposal:
        std::sync::OnceLock<Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>>,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    cancelled_notice_gate: parking_lot::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    negative_cleanup_first_observation_gate: parking_lot::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)]
    // Synchronous test observation; guard never crosses an await.
    negative_cleanup_after_observation_gate: parking_lot::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<aura_core::TimeoutBudgetError>,
        )>,
    >,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    receipt_test_hook: parking_lot::Mutex<Option<OriginalTerminalReceiptTestHook>>,
    cancelled: std::sync::OnceLock<Arc<super::invitation::OriginalIssuerCancellationTerminal>>,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    retirement: parking_lot::Mutex<OriginalIssuerRetirementState>,
    retirement_changed: tokio::sync::Notify,
    signed_cancelled_notice: std::sync::OnceLock<Arc<super::invitation::SignedCancelledNotice>>,
    finalizer_failure: std::sync::OnceLock<aura_core::AuraError>,
    finalizer_failure_changed: tokio::sync::Notify,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    result: std::sync::OnceLock<
        parking_lot::Mutex<Option<AgentResult<super::invitation::IssuerEnrollmentTerminalOutcome>>>,
    >,
}
#[cfg(test)]
pub(crate) type OriginalNegativeCleanupObservationGate = (
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<aura_core::TimeoutBudgetError>,
);
#[cfg(test)]
pub(crate) type OriginalTerminalReceiptTestHook = Box<
    dyn FnOnce(
            super::invitation::IssuerEnrollmentTerminalReceipt,
        ) -> super::invitation::IssuerEnrollmentTerminalReceipt
        + Send,
>;
#[derive(thiserror::Error)]
#[error("original cancellation notice was not transferred")]
struct RejectedOriginalCancelledNotice {
    retained: Arc<super::invitation::SignedCancelledNotice>,
}
#[cfg(test)]
impl OriginalIssuerTerminalDestination {
    pub(crate) fn install_cancelled_notice_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        aura_core::AuraError,
    > {
        let mut slot = self.cancelled_notice_gate.lock();
        if slot.is_some() || self.cancelled.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original cancelled notice gate already used",
            ));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        *slot = Some((entered, released));
        Ok((observed, release))
    }
    pub(crate) fn install_negative_cleanup_first_observation_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        aura_core::AuraError,
    > {
        let mut slot = self.negative_cleanup_first_observation_gate.lock();
        if slot.is_some() || self.cancelled.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original cancelled notice gate already used",
            ));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        *slot = Some((entered, released));
        Ok((observed, release))
    }
    pub(crate) async fn await_cancelled_notice_gate_for_test(&self) {
        let gate = self.cancelled_notice_gate.lock().take();
        if let Some((entered, released)) = gate {
            assert!(
                self.cancelled.get().is_some(),
                "actual original cancellation retained"
            );
            assert!(
                self.signed_cancelled_notice.get().is_none(),
                "no notice signed before crash boundary"
            );
            assert!(
                !self.retains_original_retirement_acknowledgment(),
                "no retirement acknowledgment before crash boundary"
            );
            entered
                .send(())
                .expect("original crash boundary observer retained");
            let _ = released.await;
        }
    }
    pub(crate) async fn await_negative_cleanup_first_observation_gate_for_test(
        &self,
        checkpoint_retained: bool,
        disposal_retained: bool,
    ) {
        let gate = self.negative_cleanup_first_observation_gate.lock().take();
        if let Some((entered, released)) = gate {
            assert!(
                !checkpoint_retained,
                "no durable negative checkpoint acknowledgment before first cleanup observation"
            );
            assert!(
                !disposal_retained,
                "no original subtree disposal acknowledgment before first cleanup observation"
            );
            assert!(
                self.cancelled.get().is_some(),
                "actual original cancellation retained"
            );
            assert!(
                self.retains_original_retirement_acknowledgment(),
                "actual generation retirement precedes required negative cleanup"
            );
            entered
                .send(())
                .expect("actual first negative observation retained");
            released
                .await
                .expect("same original negative observation gate released");
        }
    }
    pub(crate) fn install_negative_cleanup_after_observation_gate_for_test(
        &self,
    ) -> Result<OriginalNegativeCleanupObservationGate, aura_core::AuraError> {
        let mut slot = self.negative_cleanup_after_observation_gate.lock();
        if slot.is_some() || self.cancelled.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original negative observation gate already used",
            ));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let (refused, refusal) = tokio::sync::oneshot::channel();
        *slot = Some((entered, released, refused));
        Ok((observed, release, refusal))
    }
    pub(crate) async fn await_negative_cleanup_after_observation_gate_for_test(
        &self,
        retained: &mut Option<tokio::sync::oneshot::Sender<aura_core::TimeoutBudgetError>>,
    ) {
        let gate = self.negative_cleanup_after_observation_gate.lock().take();
        if let Some((entered, released, refusal)) = gate {
            assert!(
                retained.is_none(),
                "original refusal observer retained only once"
            );
            *retained = Some(refusal);
            entered
                .send(())
                .expect("actual negative provider observation retained");
            released
                .await
                .expect("same original negative operation released");
        }
    }
    pub(crate) fn install_receipt_test_hook(
        &self,
        hook: OriginalTerminalReceiptTestHook,
    ) -> Result<(), aura_core::AuraError> {
        let mut slot = self.receipt_test_hook.lock();
        if slot.is_some() || self.result.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original receipt hook is already installed or published",
            ));
        }
        *slot = Some(hook);
        Ok(())
    }
    pub(crate) fn original_finalizer_failure_for_test(&self) -> Option<aura_core::AuraError> {
        self.finalizer_failure.get().cloned()
    }
}
impl std::fmt::Debug for RejectedOriginalCancelledNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalCancelledNotice")
    }
}

#[derive(Default)]
struct OriginalIssuerRetirementState {
    first_failure: Option<aura_core::AuraError>,
    acknowledged: Option<
        crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
    >,
    refused:
        Option<crate::runtime::services::ceremony_tracker::RejectedOriginalEnrollmentRetirement>,
}

#[derive(thiserror::Error)]
#[error("original cancellation terminal refused duplicate custody")]
struct RejectedOriginalCancellationTerminal {
    retained: Arc<super::invitation::OriginalIssuerCancellationTerminal>,
}
impl std::fmt::Debug for RejectedOriginalCancellationTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalCancellationTerminal")
    }
}
#[derive(thiserror::Error)]
pub(crate) enum RejectedOriginalRetirementPublication {
    #[error("original retirement retains its primary failure")]
    StoredFailure(#[source] aura_core::AuraError),
    #[error("original retirement already retains an acknowledgment")]
    UnacceptedAcknowledgment {
        receipt: Box<
            crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
        >,
    },
    #[error("original retirement already retains refused writes")]
    UnacceptedRefusal {
        #[source]
        source: aura_core::AuraError,
        rejected:
            Box<crate::runtime::services::ceremony_tracker::RejectedOriginalEnrollmentRetirement>,
    },
}
impl std::fmt::Debug for RejectedOriginalRetirementPublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalRetirementPublication")
    }
}
impl OriginalIssuerTerminalDestination {
    #[cfg(feature = "simulation")]
    pub(crate) fn retain_simulated_original_subtree_disposal(
        &self,
        disposal: Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>,
        original: &crate::task_registry::TaskGroup,
    ) -> Result<(), aura_core::AuraError> {
        disposal.require_original_group(original)?;
        if let Some(actual) = self.simulated_subtree_disposal.get() {
            if !Arc::ptr_eq(actual, &disposal) {
                return Err(aura_core::AuraError::permission_denied(
                    "foreign original subtree disposal",
                ));
            }
            return Ok(());
        }
        self.simulated_subtree_disposal.set(disposal).map_err(|_| {
            aura_core::AuraError::invalid("original subtree disposal already retained")
        })
    }
    #[cfg(feature = "simulation")]
    pub(crate) fn simulated_original_subtree_disposal(
        &self,
    ) -> Option<Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>> {
        self.simulated_subtree_disposal.get().map(Arc::clone)
    }
    pub(crate) fn retain_finalizer_failure(&self, source: aura_core::AuraError) {
        let _ = self.finalizer_failure.set(source);
        self.finalizer_failure_changed.notify_waiters();
    }

    pub(crate) async fn await_finalizer_failure(&self) -> aura_core::AuraError {
        loop {
            let changed = self.finalizer_failure_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(source) = self.finalizer_failure.get() {
                return source.clone();
            }
            changed.await;
        }
    }

    pub(crate) fn retained_finalizer_failure(&self) -> Option<aura_core::AuraError> {
        self.finalizer_failure.get().cloned()
    }

    pub(crate) fn retain_original_cancelled_retirement(
        &self,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        terminal: &Arc<super::invitation::OriginalIssuerCancellationTerminal>,
        writes: Result<
            crate::runtime::effects::AcknowledgedEnrollmentRetirement,
            aura_core::AuraError,
        >,
    ) -> Result<(), aura_core::AuraError> {
        self.require_original_cancelled_terminal(terminal)?;
        self.retain_original_retirement(original, terminal.issued(), writes)
            .map_err(|source| aura_core::AuraError::Internal {
                message: "retain original cancelled retirement".into(),
                source: Some(Arc::new(source)),
            })
    }

    pub(crate) fn require_original_cancelled_terminal(
        &self,
        terminal: &Arc<super::invitation::OriginalIssuerCancellationTerminal>,
    ) -> Result<(), aura_core::AuraError> {
        if !self
            .cancelled
            .get()
            .is_some_and(|actual| Arc::ptr_eq(actual, terminal))
        {
            return Err(aura_core::AuraError::permission_denied(
                "cancelled retirement belongs to another original terminal",
            ));
        }
        Ok(())
    }

    pub(crate) fn with_original_cancelled_retirement<T>(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        terminal: &Arc<super::invitation::OriginalIssuerCancellationTerminal>,
        consume: impl FnOnce(
            &crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
            &super::invitation::SignedCancelledNotice,
            Option<&aura_core::AuraError>,
        ) -> Result<T, aura_core::AuraError>,
    ) -> Result<T, aura_core::AuraError> {
        let retained = self.cancelled.get().ok_or_else(|| {
            aura_core::AuraError::invalid("original cancelled decision is not retained")
        })?;
        if !Arc::ptr_eq(retained, terminal) {
            return Err(aura_core::AuraError::invalid(
                "cancelled decision belongs to another original owner",
            ));
        }
        original.require_original_cancellation(terminal)?;
        let notice = self.signed_cancelled_notice.get().ok_or_else(|| {
            aura_core::AuraError::invalid("original signed cancellation is not retained")
        })?;
        let state = self.retirement.lock();
        let receipt = state.acknowledged.as_ref().ok_or_else(|| {
            state.first_failure.clone().unwrap_or_else(|| {
                aura_core::AuraError::invalid("original retirement acknowledgment is unresolved")
            })
        })?;
        receipt.require_original(original, terminal.issued())?;
        // A late genuine receipt settles resources independently of the first
        // failure. The caller must retain that failure as the domain outcome.
        consume(receipt, notice, state.first_failure.as_ref())
    }
    pub(crate) fn retain_cancelled_terminal(
        &self,
        terminal: super::invitation::OriginalIssuerCancellationTerminal,
    ) -> AgentResult<Arc<super::invitation::OriginalIssuerCancellationTerminal>> {
        let terminal = Arc::new(terminal);
        self.cancelled.set(terminal.clone()).map_err(|retained| {
            aura_core::AuraError::PermissionDenied {
                message: "original cancellation terminal was already retained".into(),
                source: Some(Arc::new(RejectedOriginalCancellationTerminal { retained })),
            }
        })?;
        Ok(terminal)
    }
    #[cfg(test)]
    pub(crate) fn retains_original_retirement_acknowledgment(&self) -> bool {
        self.retirement.lock().acknowledged.is_some()
    }
    pub(super) fn retain_original_retirement_failure(&self, source: aura_core::AuraError) {
        let mut state = self.retirement.lock();
        state.first_failure.get_or_insert(source);
        drop(state);
        self.retirement_changed.notify_waiters();
    }
    pub(super) fn retain_original_cancelled_notice(
        &self,
        notice: super::invitation::SignedCancelledNotice,
    ) -> AgentResult<Arc<super::invitation::SignedCancelledNotice>> {
        let notice = Arc::new(notice);
        match self.signed_cancelled_notice.set(Arc::clone(&notice)) {
            Ok(()) => Ok(notice),
            Err(returned) => Err(AgentError::from(aura_core::AuraError::Internal {
                message: "original signed cancellation notice already retained".into(),
                source: Some(Arc::new(RejectedOriginalCancelledNotice {
                    retained: returned,
                })),
            })),
        }
    }
    pub(super) fn retain_original_retirement(
        &self,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        issued: &super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        writes: Result<
            crate::runtime::effects::AcknowledgedEnrollmentRetirement,
            aura_core::AuraError,
        >,
    ) -> Result<(), RejectedOriginalRetirementPublication> {
        match writes {
            Err(source) => {
                self.retain_original_retirement_failure(source.clone());
                Err(RejectedOriginalRetirementPublication::StoredFailure(source))
            }
            Ok(writes) => self.retain_original_retirement_receipt(
                original.acknowledge_original_retirement(writes, issued),
            ),
        }
    }
    pub(crate) fn retain_original_retirement_receipt(
        &self,
        incoming: Result<
            crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
            crate::runtime::services::ceremony_tracker::RejectedOriginalEnrollmentRetirement,
        >,
    ) -> Result<(), RejectedOriginalRetirementPublication> {
        let mut state = self.retirement.lock();
        match incoming {
            Ok(receipt) if state.acknowledged.is_none() => state.acknowledged = Some(receipt),
            Ok(receipt) => {
                return Err(
                    RejectedOriginalRetirementPublication::UnacceptedAcknowledgment {
                        receipt: Box::new(receipt),
                    },
                )
            }
            Err(rejected) if state.refused.is_none() => {
                state
                    .first_failure
                    .get_or_insert_with(|| rejected.source.clone());
                state.refused = Some(rejected);
            }
            Err(rejected) => {
                return Err(RejectedOriginalRetirementPublication::UnacceptedRefusal {
                    source: rejected.source.clone(),
                    rejected: Box::new(rejected),
                })
            }
        }
        let outcome = match state.first_failure.as_ref() {
            Some(source) => Err(RejectedOriginalRetirementPublication::StoredFailure(
                source.clone(),
            )),
            None => Ok(()),
        };
        drop(state);
        self.retirement_changed.notify_waiters();
        outcome
    }
    pub(crate) fn require_original_retirement(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        issued: &super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<bool, aura_core::AuraError> {
        let state = self.retirement.lock();
        if let Some(source) = state.first_failure.as_ref() {
            return Err(source.clone());
        }
        if let Some(receipt) = state.acknowledged.as_ref() {
            receipt.require_original(original, issued)?;
            return Ok(true);
        }
        Ok(false)
    }
    pub(crate) async fn await_original_retirement(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        issued: &super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), aura_core::AuraError> {
        loop {
            let changed = self.retirement_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.require_original_retirement(original, issued)? {
                return Ok(());
            }
            changed.await;
        }
    }
    fn publish(
        &self,
        result: AgentResult<super::invitation::IssuerEnrollmentTerminalOutcome>,
    ) -> Result<(), aura_core::AuraError> {
        #[cfg(test)]
        let result = match result {
            Ok(super::invitation::IssuerEnrollmentTerminalOutcome::Committed(receipt)) => {
                let hook = self.receipt_test_hook.lock().take();
                let receipt = match hook {
                    Some(hook) => hook(receipt),
                    None => receipt,
                };
                Ok(super::invitation::IssuerEnrollmentTerminalOutcome::Committed(receipt))
            }
            other => other,
        };
        #[allow(clippy::disallowed_types)]
        // Atomic producer deposit into a synchronous custody slot.
        let result = parking_lot::Mutex::new(Some(result));
        self.result
            .set(result)
            .map_err(|returned| aura_core::AuraError::PermissionDenied {
                message: "original issuer terminal destination was already published".into(),
                // Keep the original slot immutable. An impossible second producer
                // result is also retained in the concrete refusal, never substituted.
                source: Some(Arc::new(UndeliveredIssuerTerminal { retained: returned })),
            })
    }
}
fn capture_original_issuer_terminal(
    destination: &OriginalIssuerTerminalDestination,
    state: &mut OriginalIssuerTerminalState,
) -> Result<(), aura_core::AuraError> {
    let terminal = &mut state.terminal;
    let failure = &mut state.protocol_failure;
    if terminal.is_some() {
        return Ok(());
    }
    // The real negative decision is installed before signing, retirement or
    // delivery yields. The protocol result is only a later observation.
    if let Some(cancelled) = destination.cancelled.get() {
        state.cancelled = Some(Arc::clone(cancelled));
    }
    if destination.result.get().is_none() && state.cancelled.is_some() {
        return Ok(());
    }
    let result =
        destination
            .result
            .get()
            .ok_or_else(|| aura_core::AuraError::PermissionDenied {
                message: "original issuer terminal destination has not been published".into(),
                source: Some(Arc::new(
                    super::invitation::EnrollmentVmAdmissionError::TerminalObserverLost,
                )),
            })?;
    let received = result.lock().take();
    match received {
        Some(Ok(super::invitation::IssuerEnrollmentTerminalOutcome::Committed(received))) => {
            *terminal = Some(received);
            Ok(())
        }
        Some(Ok(super::invitation::IssuerEnrollmentTerminalOutcome::Cancelled(received))) => {
            state.cancelled = Some(received);
            Ok(())
        }
        Some(Err(source)) => {
            let source = aura_core::AuraError::Internal {
                message: "original issuer protocol failed".into(),
                source: Some(Arc::new(source)),
            };
            failure.get_or_insert_with(|| source.clone());
            Err(source)
        }
        None if failure.is_some() => Err(failure
            .as_ref()
            .expect("retained original protocol failure")
            .clone()),
        None if state.cancelled.is_some() => Ok(()),
        None => {
            let source = aura_core::AuraError::PermissionDenied {
                message: "original issuer has no signed terminal receipt".into(),
                source: Some(Arc::new(
                    super::invitation::EnrollmentVmAdmissionError::TerminalObserverLost,
                )),
            };
            failure.get_or_insert_with(|| source.clone());
            Err(source)
        }
    }
}
#[derive(thiserror::Error)]
#[error("original issuer terminal destination rejected a duplicate producer")]
struct UndeliveredIssuerTerminal {
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    retained:
        parking_lot::Mutex<Option<AgentResult<super::invitation::IssuerEnrollmentTerminalOutcome>>>,
}
impl std::fmt::Debug for UndeliveredIssuerTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UndeliveredIssuerTerminal")
    }
}

/// Exercise custody with the actual receipt from the existing native FROST
/// fixture. This helper cannot construct a receipt or change its signed frame.
#[cfg(test)]
pub(crate) fn assert_original_terminal_capture_after_primary_failure_for_test(
    receipt: super::invitation::IssuerEnrollmentTerminalReceipt,
    effects: &AuraEffectSystem,
) -> super::invitation::IssuerEnrollmentTerminalReceipt {
    #[derive(Debug, thiserror::Error)]
    #[error("original issuer primary validation failure")]
    struct OriginalPrimaryFailure;
    let original_source = Arc::new(OriginalPrimaryFailure);
    let mut state = OriginalIssuerTerminalState {
        execution_failure: Some(aura_core::AuraError::Internal {
            message: "original issuer primary validation failure".into(),
            source: Some(original_source.clone()),
        }),
        ..Default::default()
    };
    let original_invitation = receipt.retained().manifest().invitation.clone();
    let original_device = receipt.retained().manifest().invitee_device;
    let original_positive = receipt.positive();
    receipt
        .require_runtime_owner(effects)
        .expect("actual original receipt provider");
    let destination = OriginalIssuerTerminalDestination::default();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    drop(receiver);
    destination
        .publish(Ok(
            super::invitation::IssuerEnrollmentTerminalOutcome::Committed(receipt),
        ))
        .expect("original signed publication");
    assert!(sender.send(()).is_err(), "lost readiness observer is real");
    // Neither an earlier domain failure nor a lost notification may hide the
    // actual receipt retained directly by the same original custodian.
    capture_original_issuer_terminal(&destination, &mut state)
        .expect("late actual receipt remains accessible");
    match state
        .execution_failure
        .as_ref()
        .expect("primary failure retained")
    {
        aura_core::AuraError::Internal {
            source: Some(source),
            ..
        } => {
            assert!(source.is::<OriginalPrimaryFailure>());
            assert!(std::ptr::eq(
                source
                    .as_ref()
                    .downcast_ref::<OriginalPrimaryFailure>()
                    .unwrap(),
                original_source.as_ref()
            ));
        }
        _ => panic!("original primary source was changed"),
    }
    assert!(state.protocol_failure.is_none());
    let receipt = state
        .terminal
        .take()
        .expect("same move-owned signed receipt");
    receipt
        .require_runtime_owner(effects)
        .expect("original provider preserved");
    assert_eq!(
        receipt.retained().manifest().invitation,
        original_invitation
    );
    assert_eq!(
        receipt.retained().manifest().invitee_device,
        original_device
    );
    assert_eq!(receipt.positive(), original_positive);
    receipt
}

/// Required bootstrap lookup custody, transferred once into the existing owner.
pub(crate) struct CancelledEnrollmentNoticeRecoveryCapability {
    effects: Arc<AuraEffectSystem>,
    issued: super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    registered: crate::runtime::services::ceremony_tracker::RegisteredCancelledNoticeCapability,
}

/// Invitation service API
///
/// Provides invitation operations through a clean public API.
#[derive(Clone)]
pub struct InvitationServiceApi {
    handler: InvitationHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
    tasks: Arc<TaskSupervisor>,
}

#[derive(Debug, thiserror::Error)]
enum EnrollmentManifestExportValidationError {
    #[error("issued enrollment manifest signature validation failed")]
    Signature(#[source] aura_invitation::enrollment_manifest::EnrollmentManifestError),
    #[error("issued enrollment manifest baseline validation failed")]
    Baseline(#[source] aura_invitation::enrollment_manifest::EnrollmentManifestError),
}

/// Actual issuer-owned signed binding. No Clone/Deserialize/raw constructor.
pub(crate) struct IssuedEnrollmentManifestBinding {
    runtime_owner: Arc<AuraEffectSystem>,
    manifest: aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    digest: [u8; 32],
    signed_code: String,
    confirmation_verifier: Vec<u8>,
}
impl IssuedEnrollmentManifestBinding {
    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        if !std::ptr::eq(self.runtime_owner.as_ref(), effects) {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RuntimeOwner.into(),
            );
        }
        Ok(())
    }
    pub(crate) fn manifest(
        &self,
    ) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        &self.manifest
    }
    pub(crate) fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub(crate) fn signed_code(&self) -> &str {
        &self.signed_code
    }
    pub(crate) fn confirmation_verifier(&self) -> &[u8] {
        &self.confirmation_verifier
    }
}

enum EnrollmentCodeExportOwner<'a> {
    Issued(&'a IssuedEnrollmentManifestBinding),
    Retained(&'a super::invitation::enrollment_trust::RetainedEnrollmentVmControl),
}
impl EnrollmentCodeExportOwner<'_> {
    fn require_effects(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        match self {
            Self::Issued(issued) => issued.require_effects(effects),
            Self::Retained(retained) => retained
                .require_runtime_owner(effects)
                .map_err(AgentError::from),
        }
    }
    fn manifest(&self) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        match self {
            Self::Issued(issued) => issued.manifest(),
            Self::Retained(retained) => retained.manifest(),
        }
    }
}

impl std::fmt::Debug for InvitationServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvitationServiceApi")
            .finish_non_exhaustive()
    }
}

/// Bounds selector lookup and historical negative publication only. This local
/// ingress scope cannot be used as an original enrollment execution window.
struct CancellationIngressWindowCapability {
    effects: Arc<AuraEffectSystem>,
    budget: aura_core::TimeoutBudget,
}
impl CancellationIngressWindowCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "cancellation_ingress_window",
        capability_type = CancellationIngressWindowCapability,
        family = "runtime_helper"
    )]
    async fn acquire(
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<CancellationIngressWindowCapability> {
        let now = effects.physical_time().await.map_err(|source| {
            AgentError::from(aura_core::AuraError::from(
                aura_core::TimeoutBudgetError::time_source_failure(source),
            ))
        })?;
        let budget = aura_core::TimeoutBudget::from_start_and_timeout(
            &now,
            std::time::Duration::from_secs(5),
        )
        .map_err(aura_core::AuraError::from)?;
        Ok(Self { effects, budget })
    }
    async fn execute<F, Fut, T>(&self, operation: F) -> AgentResult<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = AgentResult<T>>,
    {
        aura_core::time::timeout::execute_with_timeout_budget(
            self.effects.as_ref(),
            &self.budget,
            operation,
        )
        .await
        .map_err(|source| {
            crate::runtime::services::enrollment_window::map_enrollment_run_error(
                "cancellation ingress",
                &self.budget,
                source,
            )
        })
    }
}

impl InvitationServiceApi {
    /// Create a new invitation service with shared runtime-owned supervisors.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        tasks: Arc<TaskSupervisor>,
    ) -> AgentResult<Self> {
        Self::new_with_runner_optional_signing(
            effects,
            authority_context,
            ceremony_runner,
            tasks,
            None,
        )
    }

    pub(crate) fn new_with_runner_and_signing(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        tasks: Arc<TaskSupervisor>,
        signing: crate::runtime::services::ThresholdSigningService,
    ) -> AgentResult<Self> {
        signing.require_effects(effects.as_ref())?;
        Self::new_with_runner_optional_signing(
            effects,
            authority_context,
            ceremony_runner,
            tasks,
            Some(signing),
        )
    }

    fn new_with_runner_optional_signing(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        tasks: Arc<TaskSupervisor>,
        signing: Option<crate::runtime::services::ThresholdSigningService>,
    ) -> AgentResult<Self> {
        let mut handler = InvitationHandler::new(authority_context)?;
        if let Some(signing) = signing {
            handler = handler.with_threshold_signing(signing);
        }
        {
            // Peers this authority verified before a restart stay reachable.
            let handler = handler.clone();
            let effects = effects.clone();
            let fut = async move {
                handler
                    .restore_verified_peer_descriptors(effects.as_ref())
                    .await;
            };
            let restore_tasks = tasks.group("invitation_service.restore_peer_hints");
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    let _task_handle = restore_tasks.spawn_local_named("restore_peer_hints", fut);
                } else {
                    let _task_handle = restore_tasks.spawn_named("restore_peer_hints", fut);
                }
            }
        }
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
            tasks,
        })
    }

    fn spawn_guardian_invitation_principal(&self, invitation: &Invitation) {
        let invitation = invitation.clone();
        let handler = self.handler.clone();
        let service = self.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.guardian_principal.{}",
            invitation.invitation_id
        ));
        let fut = async move {
            let result = handler
                .execute_guardian_invitation_principal(effects, &invitation)
                .await;
            let ceremony = service.ensure_invitation_ceremony(&invitation).await;
            match (result, ceremony) {
                (Ok(()), Ok(Some(ceremony_id))) => {
                    let participant =
                        aura_core::threshold::ParticipantIdentity::guardian(invitation.receiver_id);
                    let settle = async {
                        service
                            .ceremony_runner
                            .record_local_response(&ceremony_id, participant)
                            .await?;
                        service
                            .ceremony_runner
                            .commit(&ceremony_id, CeremonyCommitMetadata::default())
                            .await
                    }
                    .await;
                    if let Err(error) = settle {
                        tracing::warn!(ceremony_id = %ceremony_id, error = %error, "guardian ceremony completion failed");
                    }
                }
                (Err(error), Ok(Some(ceremony_id))) => {
                    let reason = if error.is_timeout() {
                        aura_app::runtime_bridge::CeremonyFailureReason::TimedOut
                    } else {
                        aura_app::runtime_bridge::CeremonyFailureReason::ChoreographyFailed
                    };
                    if let Err(settle_error) = service
                        .ceremony_runner
                        .fail_with_reason(&ceremony_id, reason, Some(error.to_string()))
                        .await
                    {
                        tracing::warn!(ceremony_id = %ceremony_id, error = %settle_error, "guardian ceremony failure publication failed");
                    }
                }
                (Err(error), _) => tracing::warn!(
                    invitation_id = %invitation.invitation_id,
                    error = %error,
                    "guardian principal choreography did not complete"
                ),
                (Ok(()), _) => tracing::warn!(
                    invitation_id = %invitation.invitation_id,
                    "guardian choreography completed without registered ceremony"
                ),
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("guardian_principal", fut);
            } else {
                let _task_handle = tasks.spawn_named("guardian_principal", fut);
            }
        }
    }

    /// Transfers the original registered root synchronously into the caller's
    /// retained custodian. Publication uses that caller's existing required task.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentExecutionRoot",
        capability_type = crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,>,
        family = "runtime_helper"
    )]
    pub(crate) fn prepare_registered_device_enrollment_custody(
        &self,
        registered: &crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'_>,
        finalizer: crate::handlers::device_epoch_rotation::DeviceEpochRotationService,
        pending_root: &mut Option<
            crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
                crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
            >,
        >,
        pending_custody: &mut Option<OriginalIssuerTaskCustody>,
    ) -> Result<PreparedOriginalIssuerHandoff, aura_core::AuraError> {
        registered.require_effects(self.effects.as_ref())?;
        if pending_custody.is_some() {
            return Err(aura_core::AuraError::invalid(
                "original registered custody slot is occupied",
            ));
        }
        let original = pending_root
            .as_ref()
            .ok_or_else(|| aura_core::AuraError::invalid("original issuer root is missing"))?;
        let terminal_destination = Arc::new(OriginalIssuerTerminalDestination::default());
        let running = original.prepare_running_observation(
            self.effects.as_ref(),
            registered,
            Arc::clone(&terminal_destination),
        )?;
        let admission_observer = original.original_issuer_observer(self.effects.as_ref())?;
        let invitation = registered.canonical_invitation();
        let InvitationType::DeviceEnrollment {
            initiator_device_id,
            ..
        } = &invitation.invitation_type
        else {
            return Err(aura_core::AuraError::invalid(
                "expected device enrollment invitation",
            ));
        };
        if invitation.sender_id != self.handler.authority_context().authority_id()
            || *initiator_device_id != self.effects.device_id()
        {
            return Err(aura_core::AuraError::permission_denied(
                "registered issuer identity differs",
            ));
        }
        if invitation.receiver_id == invitation.sender_id {
            return Err(aura_core::AuraError::invalid(
                "device enrollment requires a distinct invitee",
            ));
        }

        let invitation = invitation.clone();
        let handler = self.handler.clone();
        let ceremony_runner = self.ceremony_runner.clone();
        let effects = self.effects.clone();
        let (publication_sender, publication_receiver) = tokio::sync::oneshot::channel();
        let (initialized_sender, initialized_receiver) = tokio::sync::oneshot::channel();
        pending_root
            .as_mut()
            .ok_or_else(|| aura_core::AuraError::invalid("original registered root missing"))?
            .bind_original_registered_generation(registered)?;
        *pending_custody = Some(OriginalIssuerTaskCustody {
            issuer_resource_cleanup: None,
            conditional_signing: None,
            registration_custody: None,
            held_negative: None,
            finalizer: Some(finalizer),
            issued: None,
            original: pending_root.take(),
            negative: None,
            negative_acknowledged: None,
            handler,
            effects,
            ceremony_runner: Some(ceremony_runner),
            invitation,
            terminal_receiver: None,
            terminal_destination,
            terminal_state: OriginalIssuerTerminalState::default(),
            acknowledged: None,
            admission: Some(publication_receiver),
            initialized: Some(initialized_sender),
            entered: false,
            acknowledgment_retained: false,
        });
        Ok(PreparedOriginalIssuerHandoff {
            running,
            publication_sender,
            initialized_receiver,
            admission_observer,
        })
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RecoveredEnrollmentNoticeWindowCapability",
        receiver_type = InvitationServiceApi,
        family = "runtime_helper"
    )]
    pub(crate) async fn recover_original_enrollment_notice_receivers(&self) -> AgentResult<()> {
        let admitted =
            super::invitation::enrollment_manifest_admission::load_recoverable_negative_admissions(
                self.effects.as_ref(),
            )
            .await?;
        let tasks = self
            .tasks
            .group("invitation_service.recovered_negative_notice");
        for admitted in admitted {
            let admitted = Arc::new(admitted);
            let window = crate::runtime::services::enrollment_window::RecoveredEnrollmentNoticeWindowCapability::recover(self.effects.clone(), Arc::clone(&admitted)).await?;
            self.handler
                .launch_recovered_negative_notice(self.effects.clone(), admitted, window, &tasks)
                .await?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn install_recovered_notice_crash_gate_for_test(
        &self,
        invitation: InvitationId,
        phase: super::invitation::RecoveredNoticeCrashPhase,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.handler
            .install_recovered_notice_crash_gate_for_test(invitation, phase)
    }
    #[cfg(test)]
    pub(crate) async fn recovered_enrollment_notice_disposal_for_test(
        &self,
        id: &InvitationId,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, aura_core::AuraError>
    {
        self.handler
            .recovered_negative_notice_disposal_for_test(id)
            .await
    }

    pub(crate) async fn prepare_cancelled_enrollment_notice_recovery(
        &self,
        ceremony: &aura_core::CeremonyId,
    ) -> AgentResult<Option<CancelledEnrollmentNoticeRecoveryCapability>> {
        let ingress = CancellationIngressWindowCapability::acquire(self.effects.clone()).await?;
        ingress
            .execute(|| async {
                let (_, issued) = self
                    .handler
                    .created_enrollment_for_ceremony_required(self.effects.clone(), ceremony)
                    .await?;
                self.prepare_cancelled_notice_from_issued(issued).await
            })
            .await
    }

    /// Retain the strongest issued control across live cancellation and restart.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "CancelledEnrollmentNoticeRecoveryCapability",
        family = "runtime_helper"
    )]
    async fn prepare_cancelled_notice_from_issued(
        &self,
        issued: super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> AgentResult<Option<CancelledEnrollmentNoticeRecoveryCapability>> {
        let registered = match self
            .ceremony_runner
            .prepare_cancelled_notice(&issued, self.effects.as_ref())
            .await
        {
            Ok(owner) => owner,
            Err(cause) => return Err(AgentError::from(cause)),
        };
        Ok(Some(CancelledEnrollmentNoticeRecoveryCapability {
            effects: self.effects.clone(),
            issued,
            registered,
        }))
    }

    /// Restart recovery: one owned task signs, releases the cancelled pending
    /// generation and then retries delivery of the signed bytes.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "CancelledEnrollmentNoticeRecoveryCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn start_cancelled_enrollment_notice_recovery(
        &self,
        capability: CancelledEnrollmentNoticeRecoveryCapability,
    ) -> AgentResult<()> {
        self.admit_cancelled_notice_custody(capability, None)
    }

    fn spawn_deferred_invitation_delivery(
        &self,
        invitation: &Invitation,
        deferred_network_effects: DeferredInvitationNetworkEffects,
    ) {
        if deferred_network_effects.is_empty() {
            return;
        }

        let authority = self.handler.authority_context().clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.delivery.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let command_count = deferred_network_effects.commands().len();
        let commands = deferred_network_effects.into_commands();
        tracing::info!(
            invitation_id = %invitation_id,
            sender_id = %sender_id,
            receiver_id = %receiver_id,
            command_count,
            "Scheduling deferred invitation delivery side effects"
        );
        let fut = async move {
            tracing::debug!(
                invitation_id = %invitation_id,
                sender_id = %sender_id,
                receiver_id = %receiver_id,
                command_count,
                "Executing deferred invitation delivery side effects"
            );
            for attempt in 0..DEFERRED_INVITATION_DELIVERY_ATTEMPTS {
                match execute_invitation_effect_commands(
                    commands.clone(),
                    &authority,
                    effects.as_ref(),
                    true,
                )
                .await
                {
                    Ok(()) => {
                        if attempt > 0 {
                            tracing::info!(
                                invitation_id = %invitation_id,
                                sender_id = %sender_id,
                                receiver_id = %receiver_id,
                                attempts = attempt + 1,
                                "Deferred invitation delivery succeeded after retry"
                            );
                        }
                        return;
                    }
                    Err(error) => {
                        let final_attempt = attempt + 1 == DEFERRED_INVITATION_DELIVERY_ATTEMPTS;
                        if final_attempt {
                            tracing::warn!(
                                invitation_id = %invitation_id,
                                sender_id = %sender_id,
                                receiver_id = %receiver_id,
                                attempts = attempt + 1,
                                error = %error,
                                "Deferred invitation delivery side effects failed after retries"
                            );
                            return;
                        }

                        tracing::warn!(
                            invitation_id = %invitation_id,
                            sender_id = %sender_id,
                            receiver_id = %receiver_id,
                            attempt = attempt + 1,
                            retry_in_ms = DEFERRED_INVITATION_DELIVERY_BACKOFF_MS,
                            error = %error,
                            "Deferred invitation delivery failed; retrying"
                        );
                        let _ = effects
                            .sleep_ms(DEFERRED_INVITATION_DELIVERY_BACKOFF_MS)
                            .await;
                    }
                }
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("delivery", fut);
            } else {
                let _task_handle = tasks.spawn_named("delivery", fut);
            }
        }
    }

    fn spawn_channel_acceptance_notification(&self, invitation_id: InvitationId) {
        let handler = self.handler.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.channel_acceptance.{}",
            invitation_id
        ));
        let task_name = format!("notify.{}", invitation_id);
        let invitation_id_for_log = invitation_id.clone();
        let fut = async move {
            if let Err(error) = handler
                .notify_channel_invitation_acceptance(effects.as_ref(), &invitation_id)
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id_for_log,
                    error = %error,
                    "Channel acceptance notification failed; continuing"
                );
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            let _task_handle = tasks.spawn_local_named(task_name, fut);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _task_handle = tasks.spawn_named(task_name, fut);
        }
    }

    fn spawn_invitation_ceremony_registration(&self, invitation: &Invitation) {
        if !Self::should_track_ceremony(&invitation.invitation_type) {
            return;
        }

        let invitation = invitation.clone();
        let service = self.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.ceremony_registration.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            if let Err(error) = service.ensure_invitation_ceremony(&invitation).await {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    sender_id = %sender_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "Invitation ceremony registration failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("register", fut);
            } else {
                let _task_handle = tasks.spawn_named("register", fut);
            }
        }
    }

    fn spawn_invitation_acceptance_ceremony_progress(
        &self,
        ceremony_id: CeremonyId,
        invitation: &Invitation,
    ) {
        let ceremony_runner = self.ceremony_runner.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.accept_ceremony.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            let participant = aura_core::threshold::ParticipantIdentity::guardian(receiver_id);
            if let Err(error) = ceremony_runner
                .record_local_response(&ceremony_id, participant)
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    ceremony_id = %ceremony_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "Invitation acceptance ceremony response registration failed"
                );
                return;
            }
            if let Err(error) = ceremony_runner
                .commit(&ceremony_id, CeremonyCommitMetadata::default())
                .await
            {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    ceremony_id = %ceremony_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "Invitation acceptance ceremony commit failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("accept_commit", fut);
            } else {
                let _task_handle = tasks.spawn_named("accept_commit", fut);
            }
        }
    }

    fn should_track_ceremony(invitation_type: &InvitationType) -> bool {
        matches!(
            invitation_type,
            InvitationType::Guardian { .. } | InvitationType::Channel { .. }
        )
    }

    async fn ensure_invitation_ceremony(
        &self,
        invitation: &Invitation,
    ) -> AgentResult<Option<CeremonyId>> {
        if !Self::should_track_ceremony(&invitation.invitation_type) {
            return Ok(None);
        }

        let ceremony_id = CeremonyId::new(invitation.invitation_id.to_string());
        if self.ceremony_runner.status(&ceremony_id).await.is_ok() {
            return Ok(Some(ceremony_id));
        }

        let prestate_hash = Hash32(hash(invitation.invitation_id.as_str().as_bytes()));
        let participants = vec![aura_core::threshold::ParticipantIdentity::guardian(
            invitation.receiver_id,
        )];

        self.ceremony_runner
            .start(CeremonyInitRequest {
                ceremony_id: ceremony_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::Invitation,
                initiator_id: invitation.sender_id,
                threshold_k: 1,
                total_n: 1,
                participants,
                new_epoch: 0,
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash,
            })
            .await
            .map_err(|e| AgentError::internal(format!("Failed to register ceremony: {e}")))?;

        Ok(Some(ceremony_id))
    }

    /// Whether `channel_id` is a home this authority created (its committed
    /// `SocialFact::HomeCreated`).
    async fn is_own_home(&self, channel_id: ChannelId) -> bool {
        let own = self.handler.authority_context().authority_id();
        let Ok(facts) = self.effects.load_committed_facts(own).await else {
            return false;
        };
        facts.iter().any(|fact| {
            let aura_journal::fact::FactContent::Relational(
                aura_journal::fact::RelationalFact::Generic { envelope, .. },
            ) = &fact.content
            else {
                return false;
            };
            envelope.type_id.as_str() == aura_social::SOCIAL_FACT_TYPE_ID
                && matches!(
                    <aura_social::SocialFact as aura_journal::DomainFact>::from_envelope(envelope),
                    Some(aura_social::SocialFact::HomeCreated { home_id, creator_id, .. })
                        if home_id.as_bytes() == channel_id.as_bytes() && creator_id == own
                )
        })
    }

    /// Create an invitation to a channel/home
    ///
    /// # Arguments
    /// * `receiver_id` - Authority to invite
    /// * `home_id` - Home/channel ID to invite to
    /// * `message` - Optional message
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The created invitation
    pub async fn invite_to_channel(
        &self,
        receiver_id: AuthorityId,
        home_id: String,
        context_id: Option<ContextId>,
        nickname_suggestion: Option<String>,
        bootstrap: Option<ChannelBootstrapPackage>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let _operation = self.effects.admit_public_operation()?;
        let home_id = ChannelId::from_str(&home_id).map_err(|e| {
            AgentError::invalid(format!(
                "invalid channel/home id `{home_id}`: expected canonical ChannelId format ({e})"
            ))
        })?;
        // Inviting into a channel that is one of our own fact-backed homes is a
        // home invitation: the recipient joins the home, not only the channel.
        let home = self.is_own_home(home_id).await;

        let prepared = self
            .handler
            .prepare_invitation_with_context(
                self.effects.clone(),
                receiver_id,
                InvitationType::Channel {
                    home_id,
                    nickname_suggestion,
                    bootstrap,
                    home,
                },
                None,
                context_id,
                message,
                expires_in_ms,
            )
            .await?;
        let invitation = prepared.invitation;
        let deferred_network_effects = prepared.deferred_network_effects;
        if self.effects.harness_mode_enabled() {
            execute_invitation_effect_commands(
                deferred_network_effects.into_commands(),
                self.handler.authority_context(),
                self.effects.as_ref(),
                false,
            )
            .await
            .map_err(|error| {
                tracing::warn!(
                    invitation_id = %invitation.invitation_id,
                    sender_id = %invitation.sender_id,
                    receiver_id = %invitation.receiver_id,
                    error = %error,
                    "Inline harness channel invitation delivery failed"
                );
                error
            })?;
        } else {
            self.spawn_deferred_invitation_delivery(&invitation, deferred_network_effects);
        }
        self.spawn_invitation_ceremony_registration(&invitation);
        Ok(invitation)
    }

    /// Create an invitation to become a guardian
    ///
    /// # Arguments
    /// * `receiver_id` - Authority to invite as guardian
    /// * `subject_authority` - Authority to guard
    /// * `message` - Optional message
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The created invitation
    pub async fn invite_as_guardian(
        &self,
        receiver_id: AuthorityId,
        subject_authority: AuthorityId,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let _operation = self.effects.admit_public_operation()?;
        let prepared = self
            .handler
            .prepare_invitation_with_context(
                self.effects.clone(),
                receiver_id,
                InvitationType::Guardian { subject_authority },
                None,
                None,
                message,
                expires_in_ms,
            )
            .await?;
        let invitation = prepared.invitation;
        self.spawn_invitation_ceremony_registration(&invitation);
        self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);
        self.spawn_guardian_invitation_principal(&invitation);
        Ok(invitation)
    }

    /// Create an invitation to become a contact
    ///
    /// # Arguments
    /// * `receiver_id` - Authority to invite as contact
    /// * `nickname` - Optional nickname for the contact
    /// * `message` - Optional message
    /// * `expires_in_ms` - Optional expiration time in milliseconds
    ///
    /// # Returns
    /// The created invitation
    pub async fn invite_as_contact(
        &self,
        receiver_id: AuthorityId,
        nickname: Option<String>,
        receiver_nickname: Option<String>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let _operation = self.effects.admit_public_operation()?;
        let prepared = self
            .handler
            .prepare_invitation_with_context(
                self.effects.clone(),
                receiver_id,
                InvitationType::Contact { nickname },
                receiver_nickname,
                None,
                message,
                expires_in_ms,
            )
            .await?;
        let invitation = prepared.invitation;
        self.spawn_invitation_ceremony_registration(&invitation);
        #[cfg(target_arch = "wasm32")]
        if self.effects.harness_mode_enabled() {
            if let Err(error) = execute_invitation_effect_commands(
                prepared.deferred_network_effects.into_commands(),
                self.handler.authority_context(),
                self.effects.as_ref(),
                true,
            )
            .await
            {
                tracing::warn!(
                    invitation_id = %invitation.invitation_id,
                    sender_id = %invitation.sender_id,
                    receiver_id = %invitation.receiver_id,
                    error = %error,
                    "Inline harness contact invitation delivery failed"
                );
            }
        } else {
            self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);
        Ok(invitation)
    }

    /// Create an invitation to enroll a new device for the current authority.
    ///
    /// This is intended for out-of-band transfer (copy/paste, QR).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn export_owned_enrollment_manifest<'a>(
        &'a self,
        reserved: &'a super::invitation::ReservedInvitationIssuance,
        selected_setup: &'a aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        selected_identity: &'a crate::handlers::rendezvous_identity::RequiredIdentitySigningContext<
            'a,
        >,
        final_inventory: &'a crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<
            'a,
            'a,
        >,
        manifest: aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> impl std::future::Future<
        Output = AgentResult<(
            aura_app::runtime_bridge::EnrollmentManifestTransferCodes,
            IssuedEnrollmentManifestBinding,
        )>,
    > + 'a {
        // Allocate before the caller embeds this cryptographic preparation in
        // its frame. The original reservation/setup/identity stay borrowed by
        // the same lexical task; there is no spawn or ownership re-resolution.
        Box::pin(self.export_owned_enrollment_manifest_owned(
            reserved,
            selected_setup,
            selected_identity,
            final_inventory,
            manifest,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn export_owned_enrollment_manifest_owned(
        &self,
        reserved: &super::invitation::ReservedInvitationIssuance,
        selected_setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        selected_identity: &crate::handlers::rendezvous_identity::RequiredIdentitySigningContext<
            '_,
        >,
        final_inventory: &crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<
            '_,
            '_,
        >,
        manifest: aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> AgentResult<(
        aura_app::runtime_bridge::EnrollmentManifestTransferCodes,
        IssuedEnrollmentManifestBinding,
    )> {
        use aura_invitation::enrollment_manifest::{
            encode_initiator_verifier_transfer, SignedEnrollmentTrustManifest,
        };
        final_inventory.require_manifest(self.effects.as_ref(), &manifest)?;
        manifest
            .validate_setup_validity(selected_setup.statement())
            .map_err(crate::core::AgentError::EnrollmentManifest)?;
        if manifest.invitation != *reserved.invitation_id()
            || manifest.subject != self.handler.authority_context().authority_id()
            || manifest.initiator_device != self.effects.device_id()
            || manifest.setup.nonce != selected_setup.statement().nonce
            || manifest.setup.digest != selected_setup.digest()
            || manifest.invitee_authority != selected_setup.statement().authority
            || manifest.invitee_device != selected_setup.statement().device
        {
            return Err(crate::core::AgentError::invalid(
                "manifest issuer/reservation/setup mismatch",
            ));
        }
        #[cfg(test)]
        eprintln!("enrollment manifest export stage: validate exact selected identity");
        selected_identity
            .require_effects(self.effects.as_ref())
            .map_err(crate::core::AgentError::EnrollmentManifest)?;
        #[cfg(test)]
        eprintln!("enrollment manifest export stage: load required physical identity keys");
        let (private, public) =
            crate::handlers::rendezvous_identity::require_identity_keys(selected_identity)
                .await
                .map_err(crate::core::AgentError::EnrollmentManifest)?;
        if manifest.initiator_confirmation_verifier != public {
            return Err(crate::core::AgentError::invalid(
                "manifest confirmation key mismatch",
            ));
        }
        #[cfg(test)]
        eprintln!("enrollment manifest export stage: sign exact manifest");
        let signature = sign_ed25519_transcript(self.effects.as_ref(), &manifest, &private)
            .await
            .map_err(|e| {
                crate::core::AgentError::EnrollmentManifest(
                    aura_invitation::enrollment_manifest::EnrollmentManifestError::RequiredTranscript(e),
                )
            })?;
        #[cfg(test)]
        eprintln!("enrollment manifest export stage: export exact baseline");
        let baseline = self
            .effects
            .export_tree_ops()
            .await?
            .iter()
            .map(|op| {
                aura_core::util::serialization::to_vec(op).map_err(|e| {
                    crate::core::AgentError::EnrollmentManifest(
                        aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(
                            Box::new(e),
                        ),
                    )
                })
            })
            .collect::<AgentResult<Vec<_>>>()?;
        #[cfg(test)]
        eprintln!("enrollment manifest export stage: verify own signature and baseline");
        let checked = manifest
            .clone()
            .verify_signature(self.effects.as_ref(), &public, &signature)
            .await
            .map_err(|source| {
                crate::core::AgentError::EnrollmentManifest(
                    aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(
                        Box::new(EnrollmentManifestExportValidationError::Signature(source)),
                    ),
                )
            })?
            .verify_baseline(&baseline)
            .map_err(|source| {
                crate::core::AgentError::EnrollmentManifest(
                    aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(
                        Box::new(EnrollmentManifestExportValidationError::Baseline(source)),
                    ),
                )
            })?;
        let manifest_subject = manifest.subject;
        let manifest_device = manifest.initiator_device;
        let digest = checked.manifest_digest();
        #[cfg(test)]
        eprintln!("enrollment manifest export stage: encode signed manifest");
        let manifest_code = SignedEnrollmentTrustManifest {
            manifest: manifest.clone(),
            signature,
        }
        .encode()
        .map_err(crate::core::AgentError::EnrollmentManifest)?;
        let initiator_verifier_code =
            encode_initiator_verifier_transfer(manifest_subject, manifest_device, &public)
                .map_err(crate::core::AgentError::EnrollmentManifest)?;
        #[cfg(test)]
        eprintln!("enrollment manifest export stage: construct sealed issued manifest binding");
        let binding = IssuedEnrollmentManifestBinding {
            runtime_owner: self.effects.clone(),
            manifest,
            digest,
            signed_code: manifest_code.clone(),
            confirmation_verifier: public.to_vec(),
        };
        Ok((
            aura_app::runtime_bridge::EnrollmentManifestTransferCodes {
                manifest_code,
                initiator_verifier_code,
            },
            binding,
        ))
    }

    #[cfg(test)]
    pub(crate) async fn commit_reserved_contact_for_retirement_test(
        &self,
        reserved: super::invitation::ReservedInvitationIssuance,
    ) -> AgentResult<Invitation> {
        let prepared = self
            .handler
            .prepare_reserved_invitation_with_context(
                self.effects.clone(),
                reserved,
                self.handler.authority_context().authority_id(),
                InvitationType::Contact { nickname: None },
                None,
                Some(self.handler.authority_context().default_context_id()),
                None,
                None,
            )
            .await?;
        Ok(prepared.invitation)
    }

    pub(crate) async fn reserve_device_enrollment_invitation(
        &self,
    ) -> AgentResult<super::invitation::ReservedInvitationIssuance> {
        let reserved = self
            .handler
            .reserve_invitation_issuance(&self.effects)
            .await?;
        self.effects
            .retain_original_enrollment_reservation(&reserved)
            .await?;
        Ok(reserved)
    }

    pub(crate) fn invite_device_enrollment(
        &self,
        reserved: super::invitation::ReservedInvitationIssuance,
        receiver_id: AuthorityId,
        subject_authority: AuthorityId,
        initiator_device_id: DeviceId,
        device_id: DeviceId,
        nickname_suggestion: Option<String>,
        ceremony_id: CeremonyId,
        pending_epoch: u64,
        key_package: Vec<u8>,
        threshold_config: Vec<u8>,
        public_key_package: Vec<u8>,
        baseline_tree_ops: Vec<Vec<u8>>,
        setup_binding: aura_invitation::enrollment_setup::DeviceEnrollmentSetupBinding,
        expires_in_ms: Option<u64>,
    ) -> impl std::future::Future<Output = AgentResult<Invitation>> + '_ {
        // The original reserved issuance and secret payload move into this
        // lexical future; no new task or weaker selector replaces their owner.
        Box::pin(self.invite_device_enrollment_owned(
            reserved,
            receiver_id,
            subject_authority,
            initiator_device_id,
            device_id,
            nickname_suggestion,
            ceremony_id,
            pending_epoch,
            key_package,
            threshold_config,
            public_key_package,
            baseline_tree_ops,
            setup_binding,
            expires_in_ms,
        ))
    }

    async fn invite_device_enrollment_owned(
        &self,
        reserved: super::invitation::ReservedInvitationIssuance,
        receiver_id: AuthorityId,
        subject_authority: AuthorityId,
        initiator_device_id: DeviceId,
        device_id: DeviceId,
        nickname_suggestion: Option<String>,
        ceremony_id: CeremonyId,
        pending_epoch: u64,
        key_package: Vec<u8>,
        threshold_config: Vec<u8>,
        public_key_package: Vec<u8>,
        baseline_tree_ops: Vec<Vec<u8>>,
        setup_binding: aura_invitation::enrollment_setup::DeviceEnrollmentSetupBinding,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let prepared = Box::pin(self.handler.prepare_reserved_invitation_with_context(
            self.effects.clone(),
            reserved,
            receiver_id,
            InvitationType::DeviceEnrollment {
                setup_binding,
                subject_authority,
                invitee_authority: receiver_id,
                initiator_device_id,
                device_id,
                nickname_suggestion,
                ceremony_id,
                pending_epoch,
                key_package,
                threshold_config,
                public_key_package,
                baseline_tree_ops,
            },
            None,
            None,
            None,
            expires_in_ms,
        ))
        .await?;
        let invitation = prepared.invitation;
        self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);

        Ok(invitation)
    }

    /// Accept an invitation
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to accept
    ///
    /// # Returns
    /// Result of the acceptance
    pub async fn accept(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
        let _operation = self.effects.admit_public_operation()?;
        Box::pin(self.accept_owned(invitation_id)).await
    }

    /// Consume the canonical retained enrollment root exactly once.
    pub(crate) async fn take_original_enrollment_execution(
        &self,
        invitation: &InvitationId,
    ) -> Result<
        crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            super::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        >,
        aura_core::AuraError,
    > {
        self.handler
            .take_original_enrollment_execution(invitation)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn return_original_enrollment_execution_for_test(
        &self,
        root: crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            super::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        >,
    ) -> Result<(), aura_core::AuraError> {
        self.handler
            .retain_original_enrollment_execution(root)
            .await
    }

    /// Keep the shared caller future bounded while retaining lexical ownership.
    async fn accept_owned(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
        // Observation selects the operation kind; only consuming its retained
        // original root can authorize the enrollment mutation that follows.
        let observed = self
            .handler
            .get_invitation_with_storage(self.effects.as_ref(), invitation_id)
            .await;
        let enrollment_root = if observed.as_ref().is_some_and(|invitation| {
            matches!(
                invitation.invitation_type,
                InvitationType::DeviceEnrollment { .. }
            )
        }) {
            Some(
                self.take_original_enrollment_execution(invitation_id)
                    .await?,
            )
        } else {
            None
        };
        if let Some(root) = enrollment_root {
            let result = self
                .handler
                .accept_original_enrollment_invitation(self.effects.as_ref(), &root)
                .await?;
            self.handler
                .execute_device_enrollment_invitee(self.effects.clone(), root)
                .await?;
            return Ok(result);
        }
        let result = self
            .handler
            .accept_invitation(self.effects.clone(), invitation_id)
            .await?;

        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(self.effects.as_ref(), invitation_id)
            .await
        {
            if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                self.spawn_channel_acceptance_notification(invitation.invitation_id.clone());
            }
            if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                if let Some(ceremony_id) = self.ensure_invitation_ceremony(&invitation).await? {
                    self.spawn_invitation_acceptance_ceremony_progress(ceremony_id, &invitation);
                }
            }
        }

        Ok(result)
    }

    /// Decline an invitation
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to decline
    ///
    /// # Returns
    /// Result of the decline
    pub async fn decline(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
        let _operation = self.effects.admit_public_operation()?;
        let result = self
            .handler
            .decline_invitation(
                self.effects.clone(),
                invitation_id,
                &self.tasks.group("invitation_service.enrollment_decline"),
            )
            .await?;

        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(self.effects.as_ref(), invitation_id)
            .await
        {
            if let InvitationType::DeviceEnrollment { .. } = &invitation.invitation_type {
                return Ok(result);
            }
            if let Some(ceremony_id) = self.ensure_invitation_ceremony(&invitation).await? {
                self.ceremony_runner
                    .fail_with_reason(
                        &ceremony_id,
                        aura_app::runtime_bridge::CeremonyFailureReason::Rejected,
                        Some("Invitation declined".to_string()),
                    )
                    .await
                    .map_err(AgentError::from)?;
            }
        }

        Ok(result)
    }

    /// Cancel an invitation (sender only)
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to cancel
    ///
    /// # Returns
    /// Result of the cancellation
    pub async fn cancel(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
        let _operation = self.effects.admit_public_operation()?;
        let ingress = CancellationIngressWindowCapability::acquire(self.effects.clone()).await?;
        ingress
            .execute(|| self.cancel_selected_invitation(invitation_id))
            .await
    }

    /// Simulation cleanup retains the original registered owner; the ceremony
    /// identifies the record, while the actual retained control authorizes cancellation.
    #[cfg(feature = "simulation")]
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RegisteredEnrollmentWindowCapability", capability_type = crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability, receiver_type = InvitationServiceApi, family = "runtime_helper")]
    pub(crate) async fn cancel_simulated_original_enrollment(
        &self,
        original: &crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        pending_gate: Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    ) -> AgentResult<OriginalSimulationCleanupOutcome> {
        let (record, issued) = self
            .handler
            .created_enrollment_for_ceremony_required(
                self.effects.clone(),
                original.original_simulation_ceremony(),
            )
            .await?;
        let issued = Arc::new(issued);
        if original.original_committed_decision(self.effects.as_ref(), &issued)? {
            return Ok(OriginalSimulationCleanupOutcome::Committed);
        }
        if let Some((entered, release)) = pending_gate {
            entered.send(()).map_err(|_| {
                aura_core::AuraError::invalid("pending cleanup observer disconnected")
            })?;
            release
                .await
                .map_err(|source| aura_core::AuraError::Internal {
                    message: "pending cleanup gate release disconnected".into(),
                    source: Some(Arc::new(source)),
                })?;
        }
        let cancellation = self
            .cancel_with_original_custody(record, Arc::clone(&issued))
            .await;
        match cancellation {
            Ok(_) => Ok(OriginalSimulationCleanupOutcome::Cancelled),
            Err(source) => {
                let registration = self.ceremony_runner.existing_original_cancellation_task(
                    &issued, crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole::Publication,
                )?.ok_or(source)?;
                let failure = registration
                    .observe_simulated_failure_disposal(self.effects.as_ref())
                    .await?;
                original.original_positive_decision(&issued)?;
                Ok(OriginalSimulationCleanupOutcome::Refused(failure))
            }
        }
    }

    async fn cancel_selected_invitation(
        &self,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        let record = self
            .handler
            .created_invitation_required(self.effects.clone(), invitation_id)
            .await?;
        let invitation = record.invitation().clone();
        if matches!(
            invitation.invitation_type,
            InvitationType::DeviceEnrollment { .. }
        ) {
            let issued = super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(&record)
            .await?;
            return self.cancel_owned_enrollment(record, issued).await;
        }
        let result = self.handler.cancel_required_invitation(record).await?;
        if let Some(ceremony_id) = self.ensure_invitation_ceremony(&invitation).await? {
            self.ceremony_runner
                .fail_with_reason(
                    &ceremony_id,
                    aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
                    Some("Invitation canceled".into()),
                )
                .await
                .map_err(AgentError::from)?;
        }
        Ok(result)
    }
    /// Cancel only the original protected issuer allocation selected by this ID.
    pub(crate) async fn cancel_original_device_enrollment_ceremony(
        &self,
        ceremony: &aura_core::CeremonyId,
    ) -> AgentResult<InvitationResult> {
        let ingress = CancellationIngressWindowCapability::acquire(self.effects.clone()).await?;
        ingress
            .execute(|| async {
                let (record, issued) = self
                    .handler
                    .created_enrollment_for_ceremony_required(self.effects.clone(), ceremony)
                    .await?;
                self.cancel_owned_enrollment(record, issued).await
            })
            .await
    }

    async fn cancel_owned_enrollment(
        &self,
        record: super::invitation::SenderInvitationRecordCapability,
        issued: super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> AgentResult<InvitationResult> {
        self.cancel_with_original_custody(record, Arc::new(issued))
            .await
    }

    /// List pending invitations
    ///
    /// # Returns
    /// List of pending invitations
    pub async fn list_pending(&self) -> Vec<Invitation> {
        self.handler.list_pending_with_storage(&self.effects).await
    }

    /// List cached invitations matching a predicate.
    pub async fn list_cached_matching(
        &self,
        predicate: impl Fn(&Invitation) -> bool,
    ) -> Vec<Invitation> {
        self.handler.list_cached_matching(predicate).await
    }

    /// Read persisted channel invitations for required membership queries.
    pub async fn list_channel_invitations_with_storage_required(
        &self,
    ) -> Result<Vec<Invitation>, aura_core::AuraError> {
        self.handler
            .list_channel_invitations_with_storage_required(&self.effects)
            .await
    }

    /// Authoritative participants of a channel (AMP membership plus accepted
    /// channel invitations).
    pub async fn channel_participants(
        &self,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<std::collections::BTreeSet<AuthorityId>, aura_core::AuraError> {
        self.handler
            .channel_participants(&self.effects, context, channel)
            .await
    }

    /// Observed-only best-effort listing from cache and persisted stores.
    pub async fn list_with_storage(&self) -> Vec<Invitation> {
        self.handler.list_with_storage(&self.effects).await
    }

    /// Get an invitation by ID
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation
    ///
    /// # Returns
    /// The invitation if found
    pub async fn get(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        self.handler.get_invitation(invitation_id).await
    }

    /// Check if an invitation is pending
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation
    ///
    /// # Returns
    /// True if the invitation exists and is pending
    pub async fn is_pending(&self, invitation_id: &InvitationId) -> bool {
        self.handler
            .get_invitation(invitation_id)
            .await
            .map(|inv| inv.status == InvitationStatus::Pending)
            .unwrap_or(false)
    }

    // =========================================================================
    // Sharing Methods (Out-of-Band Transfer)
    // =========================================================================

    fn sender_transport_metadata(&self) -> ShareableInvitationTransportMetadata {
        // Advertise one address per transport type so the importer can dial
        // whichever type it supports (browsers only open WebSockets).
        let sender_hint = self.effects.lan_transport().and_then(|transport| {
            sender_hint_from_addrs(
                transport.advertised_addrs().first().map(String::as_str),
                transport.websocket_addrs().first().map(String::as_str),
            )
        });
        tracing::info!(
            sender_hint = ?sender_hint,
            "export invitation sender transport hint"
        );
        ShareableInvitationTransportMetadata {
            sender_hint,
            sender_device_id: Some(self.effects.device_id()),
        }
    }

    fn append_sender_hint(
        &self,
        mut code: String,
        transport: &ShareableInvitationTransportMetadata,
    ) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let sender_hint_segment = transport
            .sender_hint
            .as_deref()
            .map(|hint| URL_SAFE_NO_PAD.encode(hint.as_bytes()))
            .unwrap_or_else(|| URL_SAFE_NO_PAD.encode("".as_bytes()));
        let encoded_device_id = transport
            .sender_device_id
            .map(|device_id| URL_SAFE_NO_PAD.encode(device_id.to_string().as_bytes()))
            .unwrap_or_else(|| URL_SAFE_NO_PAD.encode("".as_bytes()));
        if transport.sender_hint.is_some() || self.effects.harness_mode_enabled() {
            code = format!("{code}:{sender_hint_segment}:{encoded_device_id}");
        }

        code
    }

    /// Issue one externally addressed Contact/Guardian shortcut only after
    /// its final transfer bytes have a verified native device-quorum proof.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "grant", capability_type = crate::runtime::services::enrollment_window::EnrolledDeviceShortcutAuthority, family = "runtime_helper")]
    pub(crate) async fn issue_signed_original_shortcut(
        &self,
        signing: &crate::runtime::services::threshold_signing::ThresholdSigningService,
        grant: &crate::runtime::services::enrollment_window::EnrolledDeviceShortcutAuthority,
        recipient: AuthorityId,
        recipient_device: DeviceId,
    ) -> AgentResult<IssuedOriginalShortcutDelivery> {
        use crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose;
        let _operation = self.effects.admit_public_operation()?;
        signing.require_effects(self.effects.as_ref())?;
        let issuer = self.handler.authority_context().authority_id();
        if issuer != grant.subject() || recipient == issuer {
            return Err(AgentError::invalid(
                "original shortcut grant or recipient mismatch",
            ));
        }
        let invitation_type = match grant.purpose() {
            EnrolledDeviceShortcutPurpose::Contact => InvitationType::Contact { nickname: None },
            EnrolledDeviceShortcutPurpose::Guardian => InvitationType::Guardian {
                subject_authority: issuer,
            },
        };
        let reserved = self
            .handler
            .reserve_invitation_issuance(&self.effects)
            .await?;
        let context_id = self
            .handler
            .resolve_invitation_context(self.effects.as_ref(), &invitation_type)
            .await?;
        let invitation = Invitation {
            invitation_id: reserved.invitation_id().clone(),
            context_id,
            sender_id: issuer,
            receiver_id: recipient,
            invitation_type: invitation_type.clone(),
            status: InvitationStatus::Pending,
            created_at: reserved.created_at_ms(),
            expires_at: None,
            message: None,
            receiver_nickname: None,
        };
        let transport = self.sender_transport_metadata();
        let verified = super::invitation::issued_identity::VerifiedQuorumInvitationIssuance::sign(
            self.effects.clone(),
            signing,
            &reserved,
            &invitation,
            recipient_device,
            &transport,
        )
        .await?;
        let code = verified.code().to_owned();

        // The reservation fixes id/time and context_override fixes context;
        // guard, fact, storage and cache effects begin only after proof checks.
        let prepared = self
            .handler
            .prepare_reserved_invitation_with_proof(
                self.effects.clone(),
                reserved,
                recipient,
                invitation_type,
                None,
                Some(context_id),
                None,
                None,
                Some(verified),
            )
            .await?;
        if prepared.invitation.invitation_id != invitation.invitation_id
            || prepared.invitation.context_id != invitation.context_id
            || prepared.invitation.sender_id != invitation.sender_id
            || prepared.invitation.receiver_id != invitation.receiver_id
            || prepared.invitation.invitation_type != invitation.invitation_type
            || prepared.invitation.expires_at != invitation.expires_at
        {
            return Err(AgentError::invalid(
                "committed shortcut differs from signed intent",
            ));
        }
        self.spawn_invitation_ceremony_registration(&prepared.invitation);
        if matches!(
            prepared.invitation.invitation_type,
            InvitationType::Guardian { .. }
        ) {
            self.spawn_guardian_invitation_principal(&prepared.invitation);
        }
        Ok(IssuedOriginalShortcutDelivery {
            invitation_id: prepared.invitation.invitation_id,
            issuer,
            recipient,
            recipient_device,
            purpose: grant.purpose(),
            code: self.append_sender_hint(code, &transport),
        })
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "issued",
        capability_type = IssuedEnrollmentManifestBinding,
        family = "runtime_helper"
    )]
    pub(crate) async fn export_owned_enrollment_invitation(
        &self,
        invitation: &Invitation,
        selected_identity: &crate::handlers::rendezvous_identity::RequiredIdentitySigningContext<
            '_,
        >,
        issued: &IssuedEnrollmentManifestBinding,
    ) -> AgentResult<String> {
        self.export_enrollment_invitation_from_owner(
            invitation,
            selected_identity,
            EnrollmentCodeExportOwner::Issued(issued),
        )
        .await
    }

    async fn export_enrollment_invitation_from_owner(
        &self,
        invitation: &Invitation,
        selected_identity: &crate::handlers::rendezvous_identity::RequiredIdentitySigningContext<
            '_,
        >,
        owner: EnrollmentCodeExportOwner<'_>,
    ) -> AgentResult<String> {
        use super::invitation::enrollment_trust::EnrollmentVerifierError;
        use aura_invitation::enrollment_manifest::EnrollmentManifestError;
        owner.require_effects(self.effects.as_ref())?;
        selected_identity
            .require_effects(self.effects.as_ref())
            .map_err(AgentError::EnrollmentManifest)?;
        let manifest = owner.manifest();
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
        } = &invitation.invitation_type
        else {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        };
        let baseline =
            aura_core::util::serialization::to_vec(baseline_tree_ops).map_err(|source| {
                AgentError::EnrollmentManifest(EnrollmentManifestError::Runtime(Box::new(source)))
            })?;
        if invitation.invitation_id != manifest.invitation
            || invitation.sender_id != manifest.subject
            || invitation.receiver_id != manifest.invitee_authority
            || *subject_authority != manifest.subject
            || *invitee_authority != manifest.invitee_authority
            || *initiator_device_id != manifest.initiator_device
            || *device_id != manifest.invitee_device
            || *ceremony_id != manifest.ceremony
            || *pending_epoch != manifest.pending_epoch
            || setup_binding != &manifest.setup
            || aura_core::hash::hash(key_package) != manifest.pending_share_digest
            || aura_core::hash::hash(public_key_package)
                != manifest.pending_public_key_package_digest
            || aura_core::Hash32::from_bytes(threshold_config)
                != manifest.pending_threshold_config_digest
            || baseline_tree_ops.len() != manifest.baseline_count as usize
            || aura_core::hash::hash(&baseline) != manifest.baseline_digest
            || selected_identity.epoch() != manifest.final_epoch
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        let (private, public) =
            crate::handlers::rendezvous_identity::require_identity_keys(selected_identity)
                .await
                .map_err(AgentError::EnrollmentManifest)?;
        if public.as_slice() != manifest.initiator_confirmation_verifier.as_slice() {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        let transport = self.sender_transport_metadata();
        if transport.sender_device_id != Some(manifest.initiator_device) {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        let shareable = ShareableInvitation::from(invitation);
        let signature = sign_ed25519_transcript(
            self.effects.as_ref(),
            &shareable.signing_transcript_with_transport(&transport),
            &private,
        )
        .await
        .map_err(|source| {
            AgentError::EnrollmentManifest(EnrollmentManifestError::RequiredTranscript(source))
        })?;
        let code = shareable
            .to_signed_code_with_transport(
                ShareableInvitationSenderProof {
                    scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
                    public_key: public.to_vec(),
                    signature,
                    sender_device_id: transport.sender_device_id,
                    key_epoch: Some(selected_identity.epoch()),
                },
                transport.clone(),
            )
            .map_err(|source| {
                AgentError::EnrollmentManifest(EnrollmentManifestError::Runtime(Box::new(source)))
            })?;
        Ok(self.append_sender_hint(code, &transport))
    }

    pub(crate) async fn export_signed_invitation_with_transport(
        effects: &AuraEffectSystem,
        invitation: &Invitation,
        transport: &ShareableInvitationTransportMetadata,
    ) -> AgentResult<String> {
        if matches!(
            invitation.invitation_type,
            InvitationType::DeviceEnrollment { .. }
        ) {
            return Err(AgentError::Aura(aura_core::AuraError::Invalid {
                message: "enrollment transfer requires its original owner".into(),
                source: Some(Arc::new(super::invitation::issued_identity::IssuedInvitationIdentityError::EnrollmentOwnerRequired)),
            }));
        }
        let issued = super::invitation::issued_identity::select_original_identity(
            effects,
            &invitation.invitation_id,
        )
        .await?;
        let observed =
            serde_json::to_vec(&ShareableInvitation::from(invitation)).map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Serialization {
                    message: "encode observed invitation export binding".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        let original = serde_json::to_vec(&ShareableInvitation::from(issued.invitation()))
            .map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Serialization {
                    message: "encode original invitation export binding".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        if observed != original {
            return Err(AgentError::invalid(
                "invitation export differs from required original sender record",
            ));
        }
        super::invitation::issued_identity::export_owned_invitation_code(issued, transport).await
    }

    async fn export_signed_invitation(
        &self,
        invitation: &Invitation,
        transport: &ShareableInvitationTransportMetadata,
    ) -> AgentResult<String> {
        let sender = self
            .handler
            .created_invitation_required(self.effects.clone(), &invitation.invitation_id)
            .await?;
        let issued = super::invitation::issued_identity::load_original_identity(sender).await?;
        let observed =
            serde_json::to_vec(&ShareableInvitation::from(invitation)).map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Serialization {
                    message: "encode observed invitation export binding".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        let original = serde_json::to_vec(&ShareableInvitation::from(issued.invitation()))
            .map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Serialization {
                    message: "encode original invitation export binding".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        if observed != original {
            return Err(AgentError::invalid(
                "invitation export differs from required original sender record",
            ));
        }
        super::invitation::issued_identity::export_owned_invitation_code(issued, transport).await
    }

    /// Export an invitation as a shareable code string (compile-time safe)
    ///
    /// This is the preferred method when you already have the `Invitation` object.
    ///
    /// # Arguments
    /// * `invitation` - The invitation to export
    ///
    /// # Returns
    /// A shareable code string (format: `aura:v1:<base64>`)
    pub fn export_invitation(invitation: &Invitation) -> Result<String, ShareableInvitationError> {
        #[cfg(test)]
        {
            ShareableInvitation::from(invitation).to_code()
        }
        #[cfg(not(test))]
        {
            let _ = invitation;
            Err(ShareableInvitationError::MissingSenderProof)
        }
    }

    /// Export an invitation as a shareable code string with transport metadata.
    ///
    /// This should be used for codes that will be imported by another runtime
    /// and may need a direct sender websocket hint for the first return path.
    pub async fn export_invitation_with_sender_hint(
        &self,
        invitation: &Invitation,
    ) -> AgentResult<String> {
        if matches!(
            invitation.invitation_type,
            InvitationType::DeviceEnrollment { .. }
        ) {
            let retained = super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load(
                self.effects.clone(),
                invitation,
            )
            .await?;
            let context =
                crate::handlers::rendezvous_identity::require_retained_identity_signing_context(
                    self.effects.as_ref(),
                    &retained,
                )
                .await
                .map_err(AgentError::EnrollmentManifest)?;
            return self
                .export_enrollment_invitation_from_owner(
                    retained.canonical_invitation(),
                    &context,
                    EnrollmentCodeExportOwner::Retained(&retained),
                )
                .await;
        }
        let transport = self.sender_transport_metadata();
        let code = self
            .export_signed_invitation(invitation, &transport)
            .await?;
        Ok(self.append_sender_hint(code, &transport))
    }

    /// Export an invitation by ID as a shareable code string
    ///
    /// The code can be shared out-of-band (copy/paste, QR code, etc.)
    /// and imported by the receiver using `import_code`.
    ///
    /// **Note**: Prefer `export_invitation(&Invitation)` when you have the
    /// invitation object, as it provides compile-time safety.
    ///
    /// # Arguments
    /// * `invitation_id` - ID of the invitation to export
    ///
    /// # Returns
    /// A shareable code string (format: `aura:v1:<base64>`)
    ///
    /// # Errors
    /// Returns an error if the invitation is not found
    pub async fn export_code(&self, invitation_id: &InvitationId) -> AgentResult<String> {
        let _operation = self.effects.admit_public_operation()?;
        let invitation = self
            .handler
            .get_invitation_with_storage(&self.effects, invitation_id)
            .await
            .ok_or_else(|| {
                aura_core::AuraError::not_found(format!("Invitation not found: {}", invitation_id))
            })?;
        tracing::info!(
            invitation_id = %invitation_id,
            "export invitation sender websocket hint"
        );
        self.export_invitation_with_sender_hint(&invitation).await
    }

    /// Import an invitation from a shareable code string
    ///
    /// Decodes the code and returns the shareable invitation details.
    /// The receiver can then decide whether to accept.
    ///
    /// # Arguments
    /// * `code` - The shareable code string (format: `aura:v1:<base64>`)
    ///
    /// # Returns
    /// The decoded `ShareableInvitation`
    ///
    /// # Errors
    /// Returns an error if the code is invalid
    pub fn import_code(code: &str) -> Result<ShareableInvitation, ShareableInvitationError> {
        ShareableInvitation::from_code(code)
    }

    /// Retain the original explicit enrollment admission and its acknowledged
    /// execution birth in the same bounded invitation owner used by acceptance.
    pub(crate) async fn import_enrollment_and_cache(
        &self,
        code: &str,
        pin: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest,
    ) -> Result<Invitation, aura_invitation::enrollment_manifest::EnrollmentManifestError> {
        use aura_invitation::enrollment_manifest::EnrollmentManifestError;
        let boundary =
            |source: aura_core::AuraError| EnrollmentManifestError::Runtime(Box::new(source));
        let _operation = self
            .effects
            .admit_public_operation()
            .map_err(|source| EnrollmentManifestError::Runtime(Box::new(source)))?;
        if let Some(invitation) = self
            .handler
            .observe_original_enrollment_transfer(self.effects.as_ref(), code, pin)
            .await
            .map_err(boundary)?
        {
            return Ok(invitation);
        }
        let root = super::invitation::enrollment_manifest_admission::admit_user_transfer(
            &self.effects,
            self.effects.runtime_authority_id(),
            code,
            pin,
            &self.tasks.group("invitation_service.enrollment_import"),
        )
        .await?;
        let imported = root
            .child()
            .execute(self.effects.as_ref(), || {
                self.handler
                    .import_invitation_code(self.effects.as_ref(), code)
            })
            .await
            .map_err(|source| {
                EnrollmentManifestError::Runtime(Box::new(
                    root.child()
                        .map_run_error("original enrollment invitation import", source),
                ))
            })?;
        self.handler
            .retain_original_enrollment_execution(root)
            .await
            .map_err(boundary)?;
        Ok(imported)
    }

    /// Import an out-of-band invite code into the local invitation cache.
    ///
    /// This enables follow-up operations (e.g., accept) to look up the invitation
    /// details by `invitation_id` without requiring the original `Sent` fact to
    /// be present in the local journal.
    pub async fn import_and_cache(&self, code: &str) -> AgentResult<Invitation> {
        let _operation = self.effects.admit_public_operation()?;
        self.handler
            .import_invitation_code(&self.effects, code)
            .await
    }
}

/// Builds the invitation sender hint: a comma-separated list with one
/// scheme-tagged address per transport type the sender listens on.
fn sender_hint_from_addrs(tcp: Option<&str>, websocket: Option<&str>) -> Option<String> {
    let websocket = websocket.map(|addr| {
        if addr.starts_with("ws://") || addr.starts_with("wss://") {
            addr.to_string()
        } else {
            format!("ws://{addr}")
        }
    });
    let hints: Vec<String> = tcp
        .map(|addr| format!("tcp://{addr}"))
        .into_iter()
        .chain(websocket)
        .collect();
    (!hints.is_empty()).then(|| hints.join(","))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn original_finalizer_failure_retains_first_source_before_and_after_subscription() {
        use std::sync::Arc;
        for retained_before_subscription in [true, false] {
            let destination = super::OriginalIssuerTerminalDestination::default();
            let source: Arc<dyn std::error::Error + Send + Sync> = Arc::new(std::io::Error::other(
                "actual original finalizer test failure",
            ));
            let primary = aura_core::AuraError::Internal {
                message: "original finalizer failed".into(),
                source: Some(Arc::clone(&source)),
            };
            if retained_before_subscription {
                destination.retain_finalizer_failure(primary.clone());
            }
            let observation = destination.await_finalizer_failure();
            tokio::pin!(observation);
            if !retained_before_subscription {
                assert!(futures::poll!(observation.as_mut()).is_pending());
                destination.retain_finalizer_failure(primary);
            }
            destination.retain_finalizer_failure(aura_core::AuraError::invalid(
                "later failure must not replace original source",
            ));
            let actual = observation.await;
            let aura_core::AuraError::Internal {
                source: Some(actual),
                ..
            } = actual
            else {
                panic!("observer lost original concrete failure");
            };
            assert!(Arc::ptr_eq(&actual, &source));
            let aura_core::AuraError::Internal {
                source: Some(actual),
                ..
            } = destination.retained_finalizer_failure().unwrap()
            else {
                panic!("destination replaced original concrete failure");
            };
            assert!(Arc::ptr_eq(&actual, &source));
        }
    }
    use super::*;
    use crate::core::AgentConfig;

    #[test]
    fn unpolled_owned_manifest_export_caller_frame_is_bounded() {
        fn frame_bytes<A, F: std::future::Future>(_: impl FnOnce(A) -> F) -> usize {
            std::mem::size_of::<F>()
        }
        // Only infer the exact production future type. No fake owner or setup
        // is constructed, no function is invoked and no future is polled.
        let bytes = frame_bytes(
            |(service, reserved, setup, identity, final_inventory, manifest): (
                &'static InvitationServiceApi,
                &'static super::super::invitation::ReservedInvitationIssuance,
                &'static aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
                &'static crate::handlers::rendezvous_identity::RequiredIdentitySigningContext<
                    'static,
                >,
                &'static crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<
                    'static,
                    'static,
                >,
                aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
            )| {
                service.export_owned_enrollment_manifest(
                    reserved,
                    setup,
                    identity,
                    final_inventory,
                    manifest,
                )
            },
        );
        assert!(bytes <= 16 * 1024,
            "owned manifest export caller frame is {bytes} bytes; bounded lexical delegation is required");
    }

    #[test]
    fn sender_hint_lists_every_transport_type() {
        assert_eq!(
            sender_hint_from_addrs(Some("tcp-endpoint"), Some("ws-endpoint")).as_deref(),
            Some("tcp://tcp-endpoint,ws://ws-endpoint")
        );
        assert_eq!(
            sender_hint_from_addrs(None, Some("wss://ws-endpoint")).as_deref(),
            Some("wss://ws-endpoint")
        );
        assert_eq!(sender_hint_from_addrs(None, None), None);
    }
    use crate::runtime::services::ceremony_runner::CeremonyRunner;
    use crate::runtime::services::CeremonyTracker;
    use crate::runtime::TaskSupervisor;
    use aura_core::effects::amp::ChannelCreateParams;
    use aura_core::effects::time::PhysicalTimeEffects;
    use aura_effects::AmpChannelEffects;
    use std::future::Future;

    #[track_caller]
    fn run_async_test_on_large_stack<F>(future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        std::thread::Builder::new()
            .name("invitation-service-large-stack".to_string())
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap_or_else(|error| panic!("test runtime should build: {error}"));
                runtime.block_on(future);
            })
            .unwrap_or_else(|error| panic!("large-stack test thread should spawn: {error}"))
            .join()
            .unwrap_or_else(|error| panic!("large-stack test thread should complete: {error:?}"));
    }

    #[tokio::test]
    async fn retained_invitation_service_clone_rejects_import_after_admission_closes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let authority = create_test_authority(239);
        let profile = tempfile::tempdir()?;
        let mut config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        config.storage.base_path = profile.path().join("original-profile");
        let effects = Arc::new(AuraEffectSystem::simulation_for_test_for_authority(
            &config,
            authority.authority_id(),
        )?);
        let time: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
        let service = InvitationServiceApi::new_with_runner(
            effects.clone(),
            authority,
            CeremonyRunner::new(CeremonyTracker::new(time)),
            Arc::new(TaskSupervisor::new()),
        )?;
        let retained = service.clone();
        assert_eq!(
            effects.public_operation_activity().begin_shutdown(),
            crate::runtime::system::RuntimeActivityState::Running
        );
        let error = match retained.import_and_cache("unparsed-untrusted-input").await {
            Err(error) => error,
            Ok(_) => panic!("closed original runtime admitted a retained service clone"),
        };
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut found = false;
        while let Some(cause) = source {
            if let Some(native) =
                cause.downcast_ref::<crate::runtime::system::RuntimePublicOperationError>()
            {
                assert!(matches!(
                    native,
                    crate::runtime::system::RuntimePublicOperationError::NotAccepting {
                        state: crate::runtime::system::RuntimeActivityState::Stopping
                    }
                ));
                found = true;
                break;
            }
            source = cause.source();
        }
        assert!(
            found,
            "original typed admission failure must remain in source chain"
        );
        effects
            .public_operation_activity()
            .wait_for_operations()
            .await;
        Ok(())
    }

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
    }

    #[track_caller]
    fn effects_for(authority: &AuthorityContext) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        crate::testing::simulation_effect_system_for_authority_arc(
            &config,
            authority.authority_id(),
        )
    }

    #[track_caller]
    fn effects_for_simulation(authority: &AuthorityContext, seed: u64) -> Arc<AuraEffectSystem> {
        let config = AgentConfig {
            device_id: authority.device_id(),
            ..Default::default()
        };
        Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority_with_salt(
                &config,
                authority.authority_id(),
                seed,
            )
            .unwrap(),
        )
    }

    fn service_for(
        authority_context: AuthorityContext,
        effects: Arc<AuraEffectSystem>,
    ) -> InvitationServiceApi {
        let time_effects: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
        let ceremony_runner = CeremonyRunner::new(CeremonyTracker::new(time_effects));
        InvitationServiceApi::new_with_runner(
            effects,
            authority_context,
            ceremony_runner,
            Arc::new(TaskSupervisor::new()),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_invitation_service_creation() {
        let authority_context = create_test_authority(110);
        let effects = effects_for(&authority_context);
        let expected_authority = authority_context.authority_id();

        let service = service_for(authority_context, effects);
        assert_eq!(
            service.handler.authority_context().authority_id(),
            expected_authority
        );
    }

    #[tokio::test]
    async fn test_invite_as_contact() {
        let authority_context = create_test_authority(111);
        let effects = effects_for(&authority_context);
        aura_core::effects::ThresholdSigningEffects::bootstrap_authority(
            &crate::runtime::services::ThresholdSigningService::new(effects.clone()),
            &authority_context.authority_id(),
        )
        .await
        .expect("canonical signing authority should bootstrap");
        let service = service_for(authority_context, effects);

        let receiver_id = AuthorityId::new_from_entropy([112u8; 32]);
        let invitation = service
            .invite_as_contact(
                receiver_id,
                Some("bob".to_string()),
                None,
                Some("Hey Bob!".to_string()),
                None,
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
        assert_eq!(invitation.receiver_id, receiver_id);
        assert_eq!(invitation.status, InvitationStatus::Pending);
    }

    #[tokio::test]
    async fn test_invite_as_contact_self_out_of_band_does_not_require_peer() {
        let authority_context = create_test_authority(141);
        let effects = effects_for_simulation(&authority_context, 141);
        aura_core::effects::ThresholdSigningEffects::bootstrap_authority(
            &crate::runtime::services::ThresholdSigningService::new(effects.clone()),
            &authority_context.authority_id(),
        )
        .await
        .expect("canonical signing authority should bootstrap");
        let service = service_for(authority_context.clone(), effects);

        let receiver_id = authority_context.authority_id();
        let result = service
            .invite_as_contact(
                receiver_id,
                None,
                None,
                Some("Out-of-band invite".to_string()),
                None,
            )
            .await;

        assert!(
            result.is_ok(),
            "contact invite to self should succeed for out-of-band sharing, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_invite_as_guardian() {
        let authority_context = create_test_authority(113);
        let effects = effects_for(&authority_context);
        aura_core::effects::ThresholdSigningEffects::bootstrap_authority(
            &crate::runtime::services::ThresholdSigningService::new(effects.clone()),
            &authority_context.authority_id(),
        )
        .await
        .expect("canonical signing authority should bootstrap");
        let service = service_for(authority_context.clone(), effects);

        let receiver_id = AuthorityId::new_from_entropy([114u8; 32]);
        let invitation = service
            .invite_as_guardian(
                receiver_id,
                authority_context.authority_id(),
                Some("Please guard my identity".to_string()),
                Some(604800000), // 1 week
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
        assert!(invitation.expires_at.is_some());
    }

    #[tokio::test]
    async fn test_invite_to_channel() {
        let authority_context = create_test_authority(115);
        let effects = effects_for(&authority_context);
        aura_core::effects::ThresholdSigningEffects::bootstrap_authority(
            &crate::runtime::services::ThresholdSigningService::new(effects.clone()),
            &authority_context.authority_id(),
        )
        .await
        .expect("canonical signing authority should bootstrap");
        let service = service_for(authority_context, effects.clone());

        let receiver_id = AuthorityId::new_from_entropy([116u8; 32]);
        let context_id = ContextId::new_from_entropy([117u8; 32]);
        let home_id = ChannelId::from_bytes([116u8; 32]);
        effects
            .create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(home_id),
                skip_window: None,
                topic: None,
            })
            .await
            .unwrap();
        let invitation = service
            .invite_to_channel(
                receiver_id,
                home_id.to_string(),
                Some(context_id),
                Some("shared-parity-lab".to_string()),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert!(invitation.invitation_id.as_str().starts_with("inv-"));
        match &invitation.invitation_type {
            InvitationType::Channel {
                nickname_suggestion,
                ..
            } => assert_eq!(nickname_suggestion.as_deref(), Some("shared-parity-lab")),
            _ => panic!("expected channel invitation"),
        }
    }

    #[test]
    fn invite_to_channel_defers_best_effort_delivery() {
        let source = include_str!("invitation_service.rs");
        let start = source
            .find("pub async fn invite_to_channel(")
            .expect("invite_to_channel definition");
        let body = &source[start..];
        assert!(
            body.contains(
                "self.spawn_deferred_invitation_delivery(&invitation, prepared.deferred_network_effects);"
            ) || body.contains(
                "self.spawn_deferred_invitation_delivery(&invitation, deferred_network_effects);"
            ),
            "channel invites must keep non-harness delivery on the deferred best-effort path"
        );
        assert!(
            body.contains("if self.effects.harness_mode_enabled()"),
            "channel invites must reserve the inline delivery shortcut for harness mode only"
        );
        assert!(
            body.contains("execute_invitation_effect_commands("),
            "channel invites must support inline harness delivery for deterministic harness-mode propagation"
        );
    }

    #[test]
    fn deferred_invitation_delivery_retries_after_failure() {
        let source = include_str!("invitation_service.rs");
        let start = source
            .find("fn spawn_deferred_invitation_delivery(")
            .expect("deferred delivery definition");
        let body = &source[start..];
        assert!(
            body.contains("for attempt in 0..DEFERRED_INVITATION_DELIVERY_ATTEMPTS"),
            "deferred invitation delivery must retry bounded background delivery attempts"
        );
        assert!(
            body.contains("Deferred invitation delivery failed; retrying"),
            "deferred invitation delivery should log retryable failures explicitly"
        );
    }

    #[test]
    fn test_accept_decline_flow() {
        run_async_test_on_large_stack(async move {
            // Contact acceptance needs an inviter that confirms it.
            let pair = crate::handlers::invitation::tests::contact_pair(117).await;
            let sender_service = service_for(
                AuthorityContext::new(pair.sender_id),
                pair.sender_effects.clone(),
            );
            let receiver_service = service_for(
                AuthorityContext::new(pair.receiver_id),
                pair.receiver_effects.clone(),
            );
            let receiver_id = pair.receiver_id;

            // Create two invitations
            let inv1 = sender_service
                .invite_as_contact(receiver_id, None, None, None, None)
                .await
                .unwrap();
            let inv2 = sender_service
                .invite_as_contact(receiver_id, None, None, None, None)
                .await
                .unwrap();
            let imported1 = receiver_service
                .import_and_cache(
                    &sender_service
                        .export_invitation_with_sender_hint(&inv1)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();
            let imported2 = receiver_service
                .import_and_cache(
                    &sender_service
                        .export_invitation_with_sender_hint(&inv2)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();

            // Accept one
            let accept_result = pair
                .respond_while(Box::pin(receiver_service.accept(&imported1.invitation_id)))
                .await
                .unwrap();
            assert_eq!(accept_result.new_status, InvitationStatus::Accepted);

            // Decline the other
            let decline_result = receiver_service
                .decline(&imported2.invitation_id)
                .await
                .unwrap();
            assert_eq!(decline_result.new_status, InvitationStatus::Declined);

            // Check pending is empty
            let pending = receiver_service.list_pending().await;
            assert!(pending.is_empty());
        });
    }

    #[test]
    fn test_is_pending() {
        run_async_test_on_large_stack(async move {
            // Contact acceptance needs an inviter that confirms it.
            let pair = crate::handlers::invitation::tests::contact_pair(234).await;
            let sender_service = service_for(
                AuthorityContext::new(pair.sender_id),
                pair.sender_effects.clone(),
            );
            let receiver_service = service_for(
                AuthorityContext::new(pair.receiver_id),
                pair.receiver_effects.clone(),
            );
            let receiver_id = pair.receiver_id;

            let invitation = sender_service
                .invite_as_contact(receiver_id, None, None, None, None)
                .await
                .unwrap();
            let imported = receiver_service
                .import_and_cache(
                    &sender_service
                        .export_invitation_with_sender_hint(&invitation)
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();

            assert!(receiver_service.is_pending(&imported.invitation_id).await);

            pair.respond_while(Box::pin(receiver_service.accept(&imported.invitation_id)))
                .await
                .unwrap();

            assert!(!receiver_service.is_pending(&imported.invitation_id).await);
        });
    }
}

#[cfg(test)]
mod required_enrollment_task_tests {
    use super::*;
    use crate::runtime::services::CeremonyTracker;
    use std::error::Error;

    async fn cancellation_fixture(
        label: &str,
    ) -> (
        Arc<crate::AuraAgent>,
        aura_testkit::time::ManualPhysicalClock,
        super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        aura_app::runtime_bridge::DeviceEnrollmentStart,
    ) {
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let (issuer, _invitee, invitation, start, _accept, _witness) =
            super::super::invitation::tests::actual_pinned_device_enrollment_fixture_with_clock(
                label,
                Arc::new(clock.clone()),
            )
            .await;
        let service = issuer
            .invitations()
            .expect("actual original issuer service");
        let record = service
            .handler
            .created_invitation_required(
                issuer.runtime().effects().clone(),
                &invitation.invitation_id,
            )
            .await
            .expect("required exact sender custody");
        let issued = super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(
            &record,
        ).await.expect("original independently bound issuer control");
        (issuer, clock, issued, start)
    }

    /// Task 35: an invitee that never accepts ends the initiator's tracked
    /// completion as TimedOut at the original window, and the app-owned
    /// completion operation publishes a typed Failed outcome.
    #[tokio::test]
    async fn unaccepted_enrollment_completion_times_out_with_typed_failure() {
        use aura_app::runtime_bridge::{CeremonyFailureReason, CeremonyTerminalOutcome};
        use aura_app::ui_contract::{
            OperationInstanceId, SemanticFailureCode, SemanticOperationPhase,
        };
        let (issuer, clock, issued, start) =
            Box::pin(cancellation_fixture("completion-times-out")).await;
        let runner = issuer.ceremony_runner().await;
        assert_eq!(
            runner.terminal_outcome(&start.ceremony_id).await.unwrap(),
            None,
            "starting the enrollment is not its completion"
        );
        let deadline = issuer
            .ceremony_tracker()
            .await
            .get(&start.ceremony_id)
            .await
            .expect("original registration")
            .timeout_budget
            .deadline_at_ms()
            .min(issued.manifest().expires_at_ms);
        // Bounded by attempts (600 x 50ms) rather than ambient wall-clock reads.
        let mut attempts_left = 600_u32;
        let outcome = loop {
            clock.set_time(deadline + 1);
            if let Some(outcome) = runner.terminal_outcome(&start.ceremony_id).await.unwrap() {
                break outcome;
            }
            attempts_left = attempts_left.saturating_sub(1);
            assert!(
                attempts_left > 0,
                "initiator owner must terminate the tracked ceremony at its window"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        assert_eq!(
            outcome,
            CeremonyTerminalOutcome::Failed(CeremonyFailureReason::TimedOut)
        );
        let app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                Arc::new(crate::runtime_bridge::AgentRuntimeBridge::new(
                    issuer.clone(),
                )),
            )
            .expect("issuer app owner"),
        ));
        let observed =
            aura_app::ui::workflows::ceremonies::observe_device_enrollment_completion_with_terminal_status(
                &app,
                &start.ceremony_id,
                OperationInstanceId("completion-times-out".into()),
            )
            .await;
        assert_eq!(observed.result.expect("observation"), Some(outcome));
        let terminal = observed.terminal.expect("typed terminal publication");
        assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
        assert_eq!(
            terminal.status.error.map(|error| error.code),
            Some(SemanticFailureCode::OperationTimedOut)
        );
    }

    // Supervision retains the earliest child failure, which may precede the
    // required-task settlement wrapper. Compare the original concrete allocation
    // retained by both outcomes, rather than requiring the later wrapper to win.
    fn assert_original_cancelled_shutdown_source(
        shutdown: &(dyn std::error::Error + 'static),
        disposed: &aura_core::effects::task::RequiredTaskExit,
        finalizer: &aura_core::AuraError,
    ) {
        fn cause<'a, T: std::error::Error + 'static>(
            mut error: &'a (dyn std::error::Error + 'static),
        ) -> Option<&'a T> {
            loop {
                if let Some(actual) = error.downcast_ref::<T>() {
                    return Some(actual);
                }
                error = error.source()?;
            }
        }
        let aura_core::effects::task::RequiredTaskExit::Failed(original) = disposed else {
            panic!("original cancelled task must retain its failed domain outcome");
        };
        let terminal = cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(original)
            .expect("actual disposed issuer retains its typed terminal failure");
        type Admission = super::super::invitation::EnrollmentVmAdmissionError;
        let retained = cause::<Admission>(finalizer)
            .expect("original finalizer retains its concrete enrollment failure");
        assert!(matches!(
            retained,
            Admission::TerminalFailed(aura_app::runtime_bridge::CeremonyFailureReason::Cancelled)
        ));
        assert!(
            matches!(
                cause::<crate::runtime::system::RuntimeShutdownError>(shutdown),
                Some(crate::runtime::system::RuntimeShutdownError::TaskTree(
                    crate::task_registry::TaskSupervisionError::TaskFailed { .. }
                ))
            ),
            "shutdown must retain an actual supervised task failure"
        );
        let same_finalizer =
            cause::<Admission>(shutdown).is_some_and(|actual| std::ptr::eq(actual, retained));
        let same_terminal =
            cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(shutdown)
                .is_some_and(|actual| std::ptr::eq(actual, terminal));
        assert!(
            same_finalizer || same_terminal,
            "shutdown must retain the exact original finalizer or terminal cause allocation"
        );
    }

    #[tokio::test]
    async fn original_cancelled_notice_observer_drop_delivers_once() {
        use crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole;
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let transport = crate::SharedTransport::new();
        let ((issuer, invitee, invitation, start, _accept, _witness), original) = Box::pin(
            super::super::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                "notice-observer-drop-once", Some(Arc::new(clock.clone())), Some(transport.clone()),
            ),
        ).await;
        let service = issuer.invitations().expect("actual issuer service");
        let effects = issuer.runtime().effects();
        let tracker = issuer.ceremony_tracker().await;
        let runner = issuer.ceremony_runner().await;
        let record = service
            .handler
            .created_invitation_required(effects.clone(), &invitation.invitation_id)
            .await
            .expect("actual original sender record");
        let issued = super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(
            &record,
        ).await.expect("strongest original issued control");
        let expected_device = issued.manifest().invitee_device;
        let expected_subject = issued.manifest().subject;
        settle_live_notice_original(&issuer, &issued, &original).await;
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original negative allocation");
        // The observation starts after the original issuer's terminal protocol
        // has genuinely disposed. It records this one Notice operation only.
        transport.observe_routed_envelopes_for_test();
        let capability = service
            .prepare_cancelled_notice_from_issued(issued)
            .await
            .expect("original negative notice producer")
            .expect("original negative ACK makes same-cell Notice admission available");
        let registration = runner
            .prepare_original_cancellation_task(
                &capability.issued,
                OriginalCancellationTaskRole::Notice,
            )
            .expect("producer-issued same-entry Notice registration");
        let (delivered, resume) = registration
            .observe_notice_delivery_for_test()
            .expect("install on exact original registration before admission");
        let (completion, observer) = tokio::sync::oneshot::channel();
        service
            .admit_cancelled_notice_with_original_registration(
                capability,
                Some(completion),
                Arc::clone(&registration),
            )
            .expect("canonical actual required-custody admission");
        // Only the observation is dropped. Actual required task/control stays
        // in its original entry; no task cancellation or execution-Drop claim.
        drop(observer);
        let disposal = registration
            .disposal_observer()
            .await
            .expect("borrow same original admitted control disposal");
        let disposed = disposal.await_disposal();
        tokio::pin!(disposed);
        let delivered = tokio::select! {
            delivered = delivered => delivered.expect("actual send reached postdelivery gate"),
            early = &mut disposed => panic!("original Notice disposed before delivery gate: {:?}", early.outcome()),
        };
        assert!(
            delivered.first_failure.is_none(),
            "no native fault injected"
        );
        eprintln!(
            "actual Notice delivery VM {:?}, session {}",
            delivered.vm, delivered.session
        );
        let session = delivered.session.to_string();
        let sent: Vec<_> = transport
            .routed_envelopes_for_test()
            .into_iter()
            .filter(|envelope| envelope.metadata.get("session-id") == Some(&session))
            .collect();
        assert_eq!(
            sent.len(),
            1,
            "exact one original Notice envelope actually pushed"
        );
        let sent = &sent[0];
        assert_eq!(sent.payload, delivered.bytes);
        assert_eq!(sent.source, issuer.authority_id());
        assert_eq!(sent.destination, expected_subject);
        assert_eq!(
            sent.metadata.get("aura-destination-device-id"),
            Some(&expected_device.to_string())
        );
        let receipt = sent.receipt.as_ref().expect("actual guarded send receipt");
        assert_eq!(receipt.context, sent.context);
        assert_eq!(receipt.src, sent.source);
        assert_eq!(receipt.dst, sent.destination);
        assert_eq!(
            super::super::invitation::tests::verify_original_cancelled_notice_for_test(
                invitee.as_ref(),
                &invitation,
                &delivered.bytes,
            )
            .await
            .expect("production typed terminal verifier accepts original signed wire"),
            aura_app::runtime_bridge::CeremonyFailureReason::Cancelled
        );
        assert!(original
            .observes_retained_retirement_acknowledgment()
            .expect("original negative proof survives observer Drop and send"));
        resume
            .send(())
            .expect("resume same original postdelivery operation");
        let disposed = disposed.await;
        assert!(
            matches!(
                disposed.outcome(),
                aura_core::effects::task::RequiredTaskExit::Completed
            ),
            "actual Notice task and required cleanup complete despite lost observer"
        );
        let after: Vec<_> = transport
            .routed_envelopes_for_test()
            .into_iter()
            .filter(|envelope| envelope.metadata.get("session-id") == Some(&session))
            .collect();
        assert_eq!(
            after.len(),
            1,
            "actual cleanup must not queue/send a duplicate"
        );
        assert_eq!(
            aura_core::util::serialization::to_vec(&after[0]).unwrap(),
            aura_core::util::serialization::to_vec(sent).unwrap(),
            "same exact original envelope/receipt"
        );
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original allocation retained");
        assert_eq!(
            before.timeout_budget.started_at_ms(),
            after.timeout_budget.started_at_ms()
        );
        assert_eq!(
            before.timeout_budget.deadline_at_ms(),
            after.timeout_budget.deadline_at_ms()
        );
        assert_eq!(before.terminal_outcome, after.terminal_outcome);
        assert_eq!(
            clock.now_ms(),
            5_000,
            "selected Manual provider never advances itself"
        );
        assert!(original
            .observes_retained_retirement_acknowledgment()
            .expect("same original proof remains after Notice disposal"));
        // The original issuer's actual negative failure remains distinct from
        // this successfully completed subsidiary Notice operation.
        let original_disposal = original
            .original_disposal_for_test()
            .expect("observe same original issuer after Notice disposal");
        let original_disposed = original_disposal.await_disposal().await;
        assert!(
            matches!(
                original_disposed.outcome(),
                aura_core::effects::task::RequiredTaskExit::Failed(_)
            ),
            "actual original cancelled domain remains failed"
        );
        let original_finalizer_failure = original
            .original_finalizer_failure_for_test()
            .expect("same original entry remains available")
            .expect("actual negative original finalizer has completed");
        drop((record, service, tracker, runner, original, registration));
        let context = aura_core::context::EffectContext::new(
            issuer.authority_id(),
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"notice-observer-drop-shutdown",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let issuer = Arc::try_unwrap(issuer)
            .unwrap_or_else(|_| panic!("actual runtime owner remains unique"));
        if let Err(source) = issuer.shutdown(&context).await {
            assert_original_cancelled_shutdown_source(
                &source,
                &original_disposed.outcome(),
                &original_finalizer_failure,
            );
        }
        drop((invitee, original_disposed, disposed));
    }

    #[tokio::test]
    async fn same_entry_cancelled_notice_admission_requires_original_negative_acknowledgment() {
        use crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole;
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let ((issuer, invitee, invitation, start, _accept, _witness), continuation) =
            Box::pin(super::super::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                "same-entry-notice-negative-ack", Some(Arc::new(clock.clone())), None,
            )).await;
        let tracker = issuer.ceremony_tracker().await;
        let runner = issuer.ceremony_runner().await;
        let effects = issuer.runtime().effects();
        let service = issuer.invitations().expect("actual issuer service");
        let record = service
            .handler
            .created_invitation_required(effects.clone(), &invitation.invitation_id)
            .await
            .expect("actual original sender record");
        let issued = super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(
            &record,
        ).await.expect("original runtime-bound issuer control");
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original entry");
        assert!(!continuation
            .observes_retained_retirement_acknowledgment()
            .expect("observe original receipt absence"));
        assert!(
            runner
                .prepare_cancelled_notice(&issued, effects.as_ref())
                .await
                .is_err(),
            "active original execution cannot authorize a negative notice"
        );
        assert!(
            runner
                .prepare_cancelled_notice(&issued, invitee.runtime().effects().as_ref())
                .await
                .is_err(),
            "foreign runtime cannot acquire the original notice lease"
        );
        let issuer_disposal = continuation
            .original_disposal_for_test()
            .expect("same issued task disposal observation");
        service
            .cancel(&invitation.invitation_id)
            .await
            .expect("actual original cancellation and retirement");
        let registration = tracker
            .original_cancellation_task_for_test(&issued, OriginalCancellationTaskRole::Publication)
            .expect("same entry cancellation registration");
        let cancellation_disposal = registration
            .disposal_observer()
            .await
            .expect("same admitted cancellation control");
        let disposed_cancellation = cancellation_disposal.await_disposal().await;
        let disposed_issuer = issuer_disposal.await_disposal().await;
        assert!(
            matches!(
                &disposed_issuer.outcome(),
                aura_core::effects::task::RequiredTaskExit::Failed(_)
            ),
            "real negative domain result remains failure after actual resource disposal"
        );
        assert!(continuation
            .observes_retained_retirement_acknowledgment()
            .expect("same retained original negative receipt"));
        assert!(
            runner
                .prepare_cancelled_notice(&issued, invitee.runtime().effects().as_ref())
                .await
                .is_err(),
            "negative ACK does not make a foreign runtime authoritative"
        );
        let notice = runner
            .prepare_cancelled_notice(&issued, effects.as_ref())
            .await
            .expect("real negative ACK releases the same cell for original Notice admission");
        assert!(
            runner
                .prepare_cancelled_notice(&issued, effects.as_ref())
                .await
                .is_err(),
            "one actual notice owner excludes a second acquisition"
        );
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("same original entry retained");
        assert_eq!(
            after.timeout_budget.started_at_ms(),
            before.timeout_budget.started_at_ms()
        );
        assert_eq!(
            after.timeout_budget.deadline_at_ms(),
            before.timeout_budget.deadline_at_ms()
        );
        assert!(continuation
            .observes_retained_retirement_acknowledgment()
            .expect("notice acquisition must retain original negative proof"));
        // Only the acquired notice capability is dropped. The original receipt
        // stays retained in the continuation until after all assertions.
        let original_finalizer_failure = continuation
            .original_finalizer_failure_for_test()
            .expect("same original entry remains available")
            .expect("actual negative original finalizer has completed");
        drop(notice);

        drop((
            runner,
            tracker,
            service,
            registration,
            record,
            issued,
            continuation,
        ));
        let shutdown_context = aura_core::context::EffectContext::new(
            issuer.authority_id(),
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"same-entry-notice-ack-shutdown",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let issuer = Arc::try_unwrap(issuer)
            .unwrap_or_else(|_| panic!("actual original runtime must remain uniquely owned"));
        if let Err(error) = issuer.shutdown(&shutdown_context).await {
            assert_original_cancelled_shutdown_source(
                &error,
                &disposed_issuer.outcome(),
                &original_finalizer_failure,
            );
        }
        drop((disposed_cancellation, disposed_issuer, invitee));
    }

    #[tokio::test]
    async fn settled_original_cancellation_custody_does_not_keep_tracker_alive() {
        use crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole;
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let ((issuer, invitee, invitation, _start, _accept, _witness), continuation) =
            Box::pin(super::super::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                "settled-original-custody-releases-tracker", Some(Arc::new(clock.clone())), None,
            )).await;
        let tracker = issuer.ceremony_tracker().await;
        let actor_alive = tracker.original_actor_alive_for_test();
        let issuer_disposal = continuation
            .original_disposal_for_test()
            .expect("borrow disposal observation from the actual issued continuation");
        let service = issuer
            .invitations()
            .expect("actual original issuer service");
        let record = service
            .handler
            .created_invitation_required(
                issuer.runtime().effects().clone(),
                &invitation.invitation_id,
            )
            .await
            .expect("actual original sender record");
        let issued = super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(
            &record,
        ).await.expect("actual strongest original issued control");
        service
            .cancel(&invitation.invitation_id)
            .await
            .expect("actual original cancellation and required retirement publication");
        let registration = tracker
            .original_cancellation_task_for_test(&issued, OriginalCancellationTaskRole::Publication)
            .expect("observe the exact entry-retained cancellation control");
        let cancellation_disposal = registration
            .disposal_observer()
            .await
            .expect("borrow exact original task disposal observation");
        let disposed_cancellation = cancellation_disposal.await_disposal().await;
        let disposed_issuer = issuer_disposal.await_disposal().await;
        let aura_core::effects::task::RequiredTaskExit::Failed(_) = disposed_issuer.outcome()
        else {
            panic!("disposed original cancellation must retain its negative domain failure");
        };

        assert!(continuation
            .observes_retained_retirement_acknowledgment()
            .expect("actual original retirement receipt remains observable"));
        assert!(actor_alive(), "fixture still holds the original actor");
        // Keep both real task custodians and the original registration observable
        // across actor release. Their settled custody must not pin its tracker.
        drop(service);
        drop(tracker);
        let original_finalizer_failure = continuation
            .original_finalizer_failure_for_test()
            .expect("same original entry remains available")
            .expect("actual negative original finalizer has completed");
        drop(continuation);

        // Running service actors have their own legitimate tracker references.
        // Settle their canonical runtime owner before testing completed custody.
        let activity = issuer.runtime().activity_gate();
        let shutdown_context = aura_core::context::EffectContext::new(
            issuer.authority_id(),
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"settled-original-custody-actual-shutdown",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let issuer = Arc::try_unwrap(issuer)
            .unwrap_or_else(|_| panic!("fixture must retain the original unique runtime owner"));
        let shutdown = issuer.shutdown(&shutdown_context).await;
        if let Err(error) = shutdown {
            assert_eq!(
                activity.state(),
                crate::runtime::system::RuntimeActivityState::Stopping,
                "retained domain failure prevents successful runtime termination",
            );
            assert_original_cancelled_shutdown_source(
                &error,
                &disposed_issuer.outcome(),
                &original_finalizer_failure,
            );
        } else {
            assert_eq!(
                activity.state(),
                crate::runtime::system::RuntimeActivityState::Stopped,
                "successful shutdown requires all original stage acknowledgments",
            );
        }
        drop(invitee);
        assert!(
            !actor_alive(),
            "settled custody must release strong tracker back-references"
        );
        assert!(
            registration.disposal_observer().await.is_err(),
            "retained original registration cannot recreate a dropped actor"
        );
        drop((
            registration,
            cancellation_disposal,
            disposed_cancellation,
            issuer_disposal,
            disposed_issuer,
        ));
    }

    #[tokio::test]
    async fn cancellation_preparation_observes_original_owner_without_execution_reacquisition() {
        use crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability;
        let (issuer, _clock, issued, start) =
            Box::pin(cancellation_fixture("cancellation-observation-only")).await;
        let runner = issuer.ceremony_runner().await;
        let tracker = issuer.ceremony_tracker().await;
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original registration");
        // Keep the actual registered execution permit occupied synchronously if
        // its genuine initiator has not acquired it yet. No VM is admitted by
        // this test-only lease, and availability cannot race an awaited lookup.
        let _execution = before
            .enrollment_window_lease
            .clone()
            .try_acquire_owned()
            .ok();
        assert_eq!(before.enrollment_window_lease.available_permits(), 0);
        let preparation = runner
            .original_cancellation_preparation(&issued, issuer.runtime().effects().as_ref())
            .await
            .expect("observation sibling does not reacquire execution permit");
        let EnrollmentCancellationPreparationCapability::Active(mut window) = preparation else {
            panic!("original active fixture must retain preparation eligibility");
        };
        window
            .admit_original(issuer.runtime().effects().as_ref())
            .await;
        assert!(matches!(
            window.admitted_observation(),
            Some(aura_core::time::timeout::CheckpointExecution::Finished {
                operation: Ok(()),
                ..
            })
        ));
        let actual = window
            .execute(issuer.runtime().effects().as_ref(), || async {
                Ok::<(), AgentError>(())
            })
            .await
            .expect("required original checkpoint is acknowledged");
        assert!(matches!(
            actual,
            aura_core::time::timeout::CheckpointExecution::Finished {
                operation: Ok(()),
                ..
            }
        ));
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("same allocation");
        assert_eq!(
            before.timeout_budget.deadline_at_ms(),
            after.timeout_budget.deadline_at_ms()
        );
        assert_eq!(after.enrollment_window_lease.available_permits(), 0);
        assert!(after.terminal_outcome.is_none());
    }

    #[tokio::test]
    async fn cancellation_preparation_pending_operation_expires_at_original_deadline() {
        use crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability;
        let (issuer, clock, issued, start) =
            Box::pin(cancellation_fixture("cancellation-original-deadline")).await;
        let tracker = issuer.ceremony_tracker().await;
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original registration");
        let deadline = before
            .timeout_budget
            .deadline_at_ms()
            .min(issued.manifest().expires_at_ms);
        clock.set_time(deadline - 1);
        let preparation = issuer
            .ceremony_runner()
            .await
            .original_cancellation_preparation(&issued, issuer.runtime().effects().as_ref())
            .await
            .expect("one millisecond of original admission remains");
        let EnrollmentCancellationPreparationCapability::Active(mut window) = preparation else {
            panic!("active original window expected");
        };
        window
            .admit_original(issuer.runtime().effects().as_ref())
            .await;
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let effects = issuer.runtime().effects();
        let pending = window.execute(effects.as_ref(), || async move {
            entered_tx
                .send(())
                .expect("operation observer remains live");
            std::future::pending::<AgentResult<()>>().await
        });
        tokio::pin!(pending);
        tokio::select! {
            _outcome = &mut pending => panic!("original operation must first enter"),
            entered = entered_rx => entered.expect("required operation actually entered"),
        }
        clock.set_time(deadline);
        let actual = pending
            .await
            .expect("original owner retains deadline disposition");
        assert!(
            matches!(actual, aura_core::time::timeout::CheckpointExecution::Negative {
            source: aura_core::TimeoutBudgetError::DeadlineExceeded { deadline_at_ms, .. },
            operation: None, ..
        } if deadline_at_ms == deadline)
        );
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original registration remains");
        assert_eq!(
            after.timeout_budget.deadline_at_ms(),
            before.timeout_budget.deadline_at_ms()
        );
        assert_ne!(
            after.terminal_outcome,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            )),
            "an expired pending preparation never publishes a cancelled decision"
        );
    }

    #[tokio::test]
    async fn cancellation_preparation_checkpoint_failure_retains_storage_cause_before_operation() {
        use crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability;
        let (issuer, _clock, issued, start) =
            Box::pin(cancellation_fixture("cancellation-required-checkpoint")).await;
        let tracker = issuer.ceremony_tracker().await;
        let preparation = issuer
            .ceremony_runner()
            .await
            .original_cancellation_preparation(&issued, issuer.runtime().effects().as_ref())
            .await
            .expect("actual original observation capability");
        let EnrollmentCancellationPreparationCapability::Active(mut window) = preparation else {
            panic!("active original window expected");
        };
        window
            .admit_original(issuer.runtime().effects().as_ref())
            .await;
        tracker
            .fail_next_cancellation_checkpoint_for_test(aura_core::AuraError::Storage {
                message: "injected required cancellation checkpoint write".into(),
                source: Some(Arc::new(aura_core::effects::StorageError::WriteFailed(
                    "controlled original owner checkpoint failure".into(),
                ))),
            })
            .await;
        let entered = std::cell::Cell::new(false);
        let actual = window
            .execute(issuer.runtime().effects().as_ref(), || async {
                entered.set(true);
                Ok::<(), AgentError>(())
            })
            .await
            .expect("actual checkpoint disposition remains owned");
        assert!(!entered.get());
        let aura_core::time::timeout::CheckpointExecution::Negative {
            source,
            operation: None,
            ..
        } = &actual
        else {
            panic!("actual required checkpoint failure blocks preparation");
        };
        let mut current: &(dyn Error + 'static) = source;
        loop {
            if matches!(
                current.downcast_ref::<aura_core::effects::StorageError>(),
                Some(aura_core::effects::StorageError::WriteFailed(_))
            ) {
                break;
            }
            current = current
                .source()
                .expect("original storage cause survives each boundary");
        }
        assert!(tracker
            .get(&start.ceremony_id)
            .await
            .expect("original registration")
            .terminal_outcome
            .is_none());
    }

    #[tokio::test]
    async fn required_window_rejection_retains_execution_and_settlement_sources() {
        let ((issuer, invitee, invitation, start, _, _), original) = Box::pin(super::super::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
    "required-window-original-owner-rejection", Some(Arc::new(aura_testkit::time::ManualPhysicalClock::new(5_000))), None,
)).await;
        let effects = issuer.runtime().effects();
        let tracker = CeremonyTracker::new_with_storage(effects.clone());
        let rejection = original
            .observe_original_running_owner_for_test(effects.as_ref(), &tracker)
            .err()
            .expect("unrelated tracker cannot recover original execution authority");
        crate::runtime::services::ceremony_tracker::assert_original_running_owner_refusal_for_test(
            &rejection, false,
        );
        let runner = CeremonyRunner::new(tracker);
        let missing = start.ceremony_id;
        let execution = AgentError::from(rejection);
        let supervisor = TaskSupervisor::new();
        let _handle = supervisor.spawn_try_named("required-enrollment-rejection", async move {
            Err(settle_required_enrollment_initiator_failure(&runner, &missing, execution).await)
        });
        let drained = supervisor
            .wait_for_idle(std::time::Duration::from_secs(1))
            .await
            .expect_err("required failure must fail drain");
        let retained = supervisor
            .terminal_failure()
            .expect("health retains required failure");
        for failure in [&drained, &retained] {
            let mut cause = failure.source();
            let mut found = false;
            while let Some(current) = cause {
                if let Some(enrollment) = current.downcast_ref::<EnrollmentInitiatorTaskFailure>() {
                    assert!(enrollment
                        .source()
                        .expect("original execution")
                        .is::<AgentError>());
                    assert!(
                        enrollment.terminal_publication_error().is_some(),
                        "unregistered terminal publication independently fails"
                    );
                    found = true;
                }
                cause = current.source();
            }
            assert!(
                found,
                "standard supervision source chain retains actual admission failure"
            );
        }
        invitee
            .invitations()
            .unwrap()
            .accept(&invitation.invitation_id)
            .await
            .expect("same original issuer positive completion");
        let delivery = original.original_disposal_for_test().unwrap();
        let disposal = delivery.await_disposal().await;
        delivery.require_positive(&disposal).unwrap();
        original
            .require_original_issuer_disposal_for_test(&disposal)
            .unwrap();
        for agent in [issuer, invitee] {
            match agent
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
            {
                Ok(()) => {}
                Err(crate::task_registry::TaskSupervisionError::TaskFailed { source, .. }) => {
                    eprintln!("actual owner rejection fixture domain failure: {source:?}");
                }
                Err(source) => {
                    panic!("original owner rejection resource cleanup failed: {source:?}")
                }
            }
        }
    }
    #[tokio::test]
    async fn canonical_cancellation_negative_ack_preserves_rollback_on_same_profile_reopen() {
        let diagnostics = tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .with_env_filter(
                    "aura_agent::runtime::system=info,aura_agent::runtime::effects=info",
                )
                .without_time()
                .with_test_writer()
                .finish(),
        );
        let _diagnostics = tracing::dispatcher::set_default(&diagnostics);
        use crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole;
        use crate::runtime::{EffectContext, EffectSystemBuilder};
        use aura_core::effects::{
            ExecutionMode, PhysicalTimeEffects, SecureStorageCapability, SecureStorageEffects,
            SecureStorageLocation,
        };
        #[derive(serde::Deserialize)]
        struct ClockObservation {
            budget: aura_core::TimeoutBudget,
        }
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let (config, authority, context_id, ceremony, started, deadline): (
            crate::core::AgentConfig,
            AuthorityId,
            ContextId,
            CeremonyId,
            u64,
            u64,
        ) = {
            let (issuer, invitee, selected_clock, issued, start, original) =
                Box::pin(live_notice_fixture_with_original_continuation(
                    "canonical-negative-ack-rollback-reopen",
                ))
                .await;
            eprintln!("task396 stage: genuine original fixture ready");
            let tracker = issuer.ceremony_tracker().await;
            let before = tracker
                .get(&start.ceremony_id)
                .await
                .expect("actual original owner fixture state");
            let config = issuer.runtime().effects().config().clone();
            let authority = issuer.authority_id();
            let context_id = issuer.context().default_context_id();
            let disposal = original
                .original_disposal_for_test()
                .expect("actual original owner fixture state");
            let (mut negative_entered, negative_release) = original
                .install_negative_cleanup_first_observation_gate_for_test()
                .expect("install on exact original continuation");
            selected_clock.set_time(6_000);
            assert!(!tracker
                .is_timed_out(&start.ceremony_id)
                .await
                .expect("actual original owner fixture state"));
            let mut cleanup_attempt = tracker.observe_cancellation_cleanup_for_test().await;
            let mut cleanup_observed = false;
            let (checkpointed, release) = tracker
                .observe_cancellation_postcheckpoint_for_test(Some(Box::new({
                    let selected_clock = selected_clock.clone();
                    move || {
                        selected_clock
                            .script_next_observation(5_500)
                            .expect("script exact next physical observation at checkpoint return")
                    }
                })))
                .await;
            let service = issuer
                .invitations()
                .expect("actual original issuer service");
            let (final_read_observed, final_read_result) = tokio::sync::oneshot::channel();
            let cancellation = async {
                let actual = service.cancel(&issued.manifest().invitation).await;
                final_read_observed
                    .send(())
                    .expect("observe actual canonical cancellation result");
                actual
            };
            let rollback_after_write = async {
                checkpointed
                    .await
                    .expect("actual canonical post-publication checkpoint written");
                eprintln!("task396 stage: actual cancellation checkpoint written");
                release
                    .send(())
                    .expect("release same original checkpoint callback");
                final_read_result
                    .await
                    .expect("actual canonical cancellation final-read result");
                eprintln!("task396 stage: canonical caller returned after final read");
                selected_clock.set_time(7_000);
                tokio::select! {
                    entered = &mut negative_entered => {
                        entered.expect("actual negative owner held before its first clock read");
                    }
                    cleanup = &mut cleanup_attempt => {
                        cleanup.expect("actual canonical cancellation cleanup event")
                            .expect("canonical cancellation cleanup refused before issuer gate");
                        cleanup_observed = true;
                        negative_entered.await.expect("actual issuer negative owner first observation after cancellation cleanup");
                    }
                }
                negative_release
                    .send(())
                    .expect("release exact negative owner after observed final-read failure");
                eprintln!("task396 stage: actual issuer negative cleanup gate released");
            };
            let (cancelled, ()) = tokio::join!(cancellation, rollback_after_write);
            eprintln!("task396 stage: cancellation and clock steering joined");
            let failure = cancelled
                .expect_err("final read rollback refuses positive cancellation completion");
            let mut cause: Option<&(dyn Error + 'static)> = Some(&failure);
            let mut rollback = false;
            while let Some(source) = cause {
                rollback |= matches!(
                    source.downcast_ref::<aura_core::TimeoutBudgetError>(),
                    Some(aura_core::TimeoutBudgetError::ClockRollback { .. })
                );
                cause = source.source();
            }
            assert!(
                rollback,
                "canonical caller preserves original rollback source"
            );
            if !cleanup_observed {
                cleanup_attempt.await.expect("actual original cancellation cleanup attempt observed")
                .expect("canonical negative cleanup must settle under its original cleanup resource window");
            }
            let registration = tracker
                .original_cancellation_task_for_test(
                    &issued,
                    OriginalCancellationTaskRole::Publication,
                )
                .expect("actual original owner fixture state");
            let cancellation_disposal = registration
                .disposal_observer()
                .await
                .expect("actual original owner fixture state");
            let cancellation_terminal = cancellation_disposal.await_disposal().await;
            eprintln!("task396 stage: actual cancellation disposal observed");
            assert!(
                matches!(
                    cancellation_terminal.outcome(),
                    aura_core::effects::task::RequiredTaskExit::Failed(_)
                ),
                "terminal resource ACK retains negative domain failure"
            );
            let issuer_terminal = disposal.await_disposal().await;
            eprintln!("task396 stage: actual issuer disposal observed");
            assert!(matches!(
                issuer_terminal.outcome(),
                aura_core::effects::task::RequiredTaskExit::Failed(_)
            ));
            assert!(original.observes_retained_retirement_acknowledgment().expect("actual original owner fixture state"),
                "early Negative is insufficient; actual original required cleanup ACK completes first");
            for agent in [&issuer, &invitee] {
                if let Err(source) = agent
                    .runtime()
                    .tasks()
                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                    .await
                {
                    assert!(
                        matches!(
                            source,
                            crate::task_registry::TaskSupervisionError::TaskFailed { .. }
                        ),
                        "resource drain must finish before releasing profile: {source:?}"
                    );
                }
                assert!(agent.runtime().tasks().active_tasks().is_empty());
            }
            (
                config,
                authority,
                context_id,
                start.ceremony_id,
                before.timeout_budget.started_at_ms(),
                before.timeout_budget.deadline_at_ms(),
            )
        };
        // Every original runtime, continuation, registration and observer is
        // released before acquiring the same physical selected-profile lease.
        eprintln!("task396 stage: original runtimes drained before same-profile reopen");
        clock.set_time(7_000);
        let profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
            .expect("actual same-profile reopen after required owner disposal");
        eprintln!("task396 stage: same-profile lease reacquired");
        let startup_admitted = clock
            .observe_next_relative_sleep_of(1_000)
            .await
            .expect("observe actual reopened runtime startup timer");
        let reopen_context = EffectContext::new(authority, context_id, ExecutionMode::Testing);
        let build = EffectSystemBuilder::testing_with_owned_profile(profile)
            .with_authority(authority)
            .with_config(config)
            .with_physical_time_provider(Arc::new(clock.clone()))
            .build(&reopen_context);
        let drive_startup = async {
            let target = startup_admitted
                .await
                .expect("actual original startup timer admitted");
            assert_eq!(target.value(), 8_000, "selected startup timer endpoint");
            clock.set_time(target.value());
        };
        let (runtime, ()) = tokio::join!(build, drive_startup);
        let runtime = runtime.expect("actual original owner fixture state");
        let forward_time = clock
            .physical_time()
            .await
            .expect("actual forward selected clock");
        eprintln!("task396 stage: reopened effect system built");
        let bytes = runtime
            .effects()
            .secure_retrieve(
                &SecureStorageLocation::new("enrollment_clock_v1", ceremony.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await
            .expect("actual original owner fixture state");
        eprintln!("task396 stage: durable original clock retrieved");
        let restored: ClockObservation =
            serde_json::from_slice(&bytes).expect("actual original owner fixture state");
        let reopened = Arc::new(crate::AuraAgent::new(runtime, authority));
        let acquisition = crate::runtime::services::RuntimeServiceContext::test_original(
            reopened.runtime().tasks(),
            reopened.runtime().effects(),
        )
        .await;
        eprintln!("task396 stage: original startup window acquired");
        let bridge = crate::runtime_bridge::AgentRuntimeBridge::new(reopened.clone());
        use aura_app::runtime_bridge::RuntimeBridge;
        bridge
            .bootstrap_signing_keys(acquisition.startup_window())
            .await
            .expect("canonical restored negative-only observation under original startup owner");
        eprintln!("task396 stage: canonical signing bootstrap restored observations");
        bridge
            .original_enrollment_finalizer_service()
            .recover_prepared_enrollment(&acquisition, &ceremony)
            .await
            .expect_err("negative terminal checkpoint cannot recreate a positive original owner");
        eprintln!("task396 stage: canonical recovery refused positive owner");
        let restored_state = reopened
            .runtime()
            .ceremony_tracker()
            .get(&ceremony)
            .await
            .expect("canonical recovered negative registration observation");
        assert_eq!(
            restored_state.kind,
            aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
        );
        assert_eq!(
            restored_state.terminal_outcome,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            )),
            "canonical startup preserves the cancelled terminal registration",
        );
        let restored_budget = restored_state.timeout_budget;
        assert_eq!(restored_budget.started_at_ms(), started);
        assert_eq!(restored_budget.deadline_at_ms(), deadline);
        eprintln!("task396 stage: canonical recovered original budget observed");
        assert!(
            matches!(
                restored_budget.remaining_at(&forward_time),
                Err(aura_core::TimeoutBudgetError::ClockRollback { .. })
            ),
            "canonical recovered observation preserves rollback without minting execution"
        );
        assert_eq!(restored.budget.started_at_ms(), started);
        assert_eq!(restored.budget.deadline_at_ms(), deadline);
        assert!(
            matches!(
                restored.budget.remaining_at(&forward_time),
                Err(aura_core::TimeoutBudgetError::ClockRollback { .. })
            ),
            "terminal required ACK must persist the original rollback latch before reopen"
        );
    }

    #[tokio::test]
    async fn canonical_cancellation_negative_checkpoint_write_failure_retains_original_custody_without_ack(
    ) {
        use crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole;
        use futures::FutureExt;
        let (issuer, invitee, clock, issued, start, original) =
            Box::pin(live_notice_fixture_with_original_continuation(
                "canonical-negative-checkpoint-write-failure",
            ))
            .await;
        let tracker = issuer.ceremony_tracker().await;
        let cleanup_attempt = tracker.observe_cancellation_cleanup_for_test().await;
        let original_disposal = original
            .original_disposal_for_test()
            .expect("same original issuer disposal observer");
        let (negative_entered, negative_release) = original
            .install_negative_cleanup_first_observation_gate_for_test()
            .expect("hold exact original issuer before negative observation");
        clock.set_time(6_000);
        assert!(!tracker
            .is_timed_out(&start.ceremony_id)
            .await
            .expect("original positive window remains active"));
        let (checkpointed, release) = tracker
            .observe_cancellation_postcheckpoint_for_test(Some(Box::new({
                let clock = clock.clone();
                move || {
                    clock
                        .script_next_observation(5_500)
                        .expect("script selected provider's next original observation")
                }
            })))
            .await;
        let storage = Arc::new(aura_core::effects::StorageError::WriteFailed(
            "controlled actual terminal negative checkpoint write failure".into(),
        ));
        let negative_failure = tracker
            .fail_issuer_negative_checkpoint_for_test(aura_core::AuraError::Storage {
                message: "original terminal negative checkpoint storage provider failed".into(),
                source: Some(storage.clone()),
            })
            .await;
        let service = issuer.invitations().expect("actual issuer service");
        let cancellation = service.cancel(&issued.manifest().invitation);
        let steer = async {
            checkpointed
                .await
                .expect("actual canonical cancellation checkpoint written");
            release
                .send(())
                .expect("release exact original postcheckpoint observation");
            negative_entered
                .await
                .expect("actual original issuer reached negative cleanup observation");
            clock.set_time(7_000);
            negative_release
                .send(())
                .expect("release exact original issuer at forward selected clock");
        };
        let (result, ()) = tokio::join!(cancellation, steer);
        let checkpoint_failure = negative_failure
            .await
            .expect("actual original issuer consumed injected checkpoint failure");
        let _generation_cleanup = cleanup_attempt.now_or_never();
        let source = result.expect_err("canonical cancellation retains original rollback");
        let mut cause: Option<&(dyn Error + 'static)> = Some(&source);
        let mut exact_rollback = false;
        while let Some(actual) = cause {
            exact_rollback |= matches!(
                actual.downcast_ref::<aura_core::TimeoutBudgetError>(),
                Some(aura_core::TimeoutBudgetError::ClockRollback { .. })
            );
            cause = actual.source();
        }
        let mut producer_cause: Option<&(dyn Error + 'static)> = Some(&checkpoint_failure);
        let mut exact_storage = false;
        while let Some(actual) = producer_cause {
            exact_storage |= actual
                .downcast_ref::<aura_core::effects::StorageError>()
                .is_some_and(|actual| std::ptr::eq(actual, storage.as_ref()));
            producer_cause = actual.source();
        }
        assert!(
            exact_rollback,
            "canonical primary retains its original positive-budget rollback independently of storage fault"
        );
        assert!(
            exact_storage,
            "actual original negative checkpoint producer retains the same storage source"
        );
        assert!(
            original
                .observes_retained_retirement_acknowledgment()
                .expect("same original retirement state observation"),
            "actual generation retirement remains acknowledged independently of the failed issuer checkpoint"
        );
        assert!(
            original_disposal.await_disposal().now_or_never().is_none(),
            "failed required negative write retains same issuer custody unresolved"
        );
        let registration = tracker
            .original_cancellation_task_for_test(&issued, OriginalCancellationTaskRole::Publication)
            .expect("same original cancellation admission remains registered");
        let cancellation_disposal = registration
            .disposal_observer()
            .await
            .expect("same admitted cancellation observer");
        assert!(
            cancellation_disposal
                .await_disposal()
                .now_or_never()
                .is_none(),
            "original cancellation remains in flight while required issuer storage fails"
        );
        let retained = tracker
            .original_cancellation_task_for_test(&issued, OriginalCancellationTaskRole::Publication)
            .expect("same unresolved original task is retained");
        assert!(
            Arc::ptr_eq(&registration, &retained),
            "failure cannot replace original task registration"
        );
        for agent in [issuer, invitee] {
            let shutdown = agent
                .runtime()
                .tasks()
                .shutdown_with_timeout(std::time::Duration::from_secs(2))
                .await;
            if let Err(source) = shutdown {
                assert!(agent.runtime().tasks().terminal_failure().is_some(),
                    "actual failed required cleanup remains observable during fixture teardown: {source}");
            }
        }
    }

    #[tokio::test]
    async fn canonical_original_negative_cleanup_clock_rollback_retains_custody_without_ack() {
        use crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole;
        use futures::FutureExt;
        let (issuer, invitee, clock, issued, _start, original) =
            Box::pin(live_notice_fixture_with_original_continuation(
                "actual-negative-cleanup-own-clock-rollback",
            ))
            .await;
        let tracker = issuer.ceremony_tracker().await;
        let cleanup_attempt = tracker.observe_cancellation_cleanup_for_test().await;
        let disposal = original
            .original_disposal_for_test()
            .expect("same original issued task");
        let (observed, release, refused) = original
            .install_negative_cleanup_after_observation_gate_for_test()
            .expect("gate installed through exact retained original continuation");
        clock.set_time(6_000);
        let service = issuer
            .invitations()
            .expect("actual original issuer service");
        let cancellation = service.cancel(&issued.manifest().invitation);
        let actual_cleanup_rollback = async {
            observed
                .await
                .expect("actual original negative cleanup first read6000 completed");
            clock.set_time(5_500);
            release
                .send(())
                .expect("release same negative owner after selected-provider rollback");
            let actual = refused
                .await
                .expect("actual original negative validation refusal observed");
            let direct = matches!(
                actual,
                aura_core::TimeoutBudgetError::ClockRollback {
                    previous_observed_at_ms: 6_000,
                    observed_at_ms: 5_500,
                }
            );
            let native = if matches!(
                actual,
                aura_core::TimeoutBudgetError::TimeSourceUnavailable { .. }
            ) {
                let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&actual);
                let mut exact = false;
                while let Some(source) = cause {
                    exact |= matches!(
                        source.downcast_ref::<aura_core::effects::TimeError>(),
                        Some(aura_core::effects::TimeError::PhysicalClockRollback {
                            previous_ms: 6_000,
                            observed_ms: 5_500
                        })
                    );
                    cause = source.source();
                }
                exact
            } else {
                false
            };
            assert!(
                direct || native,
                "actual cleanup rollback retains exact budget or native provider cause: {actual}"
            );
            clock.set_time(7_000);
            assert!(
                matches!(
                    original.observe_negative_cleanup_clock_for_test(
                        &aura_core::time::PhysicalTime {
                            ts_ms: 7_000,
                            uncertainty: None,
                        }
                    ),
                    Err(aura_core::TimeoutBudgetError::ClockRollback {
                        previous_observed_at_ms: 6_000,
                        observed_at_ms: 5_500,
                    })
                ),
                "forward clock cannot revive the same original cleanup budget"
            );
        };
        let (cancelled, ()) = tokio::join!(cancellation, actual_cleanup_rollback);
        cancelled.expect_err("canonical cancellation cannot settle rolled-back cleanup owner");
        // This event belongs to cancellation-generation cleanup. Its success
        // can precede the separately observed issuer negative validation failure;
        // it is not the issuer's durable checkpoint or disposal acknowledgment.
        let _generation_cleanup = cleanup_attempt
            .await
            .expect("actual canonical cancellation-generation cleanup observed");
        assert!(original
            .observes_retained_retirement_acknowledgment()
            .expect("real generation retirement receipt stays retained"));
        assert!(
            disposal.await_disposal().now_or_never().is_none(),
            "original negative cleanup provider rollback cannot grant terminal resource ACK"
        );
        let registration = tracker
            .original_cancellation_task_for_test(&issued, OriginalCancellationTaskRole::Publication)
            .expect("actual original cancellation registration retained");
        let cancellation_terminal = registration
            .disposal_observer()
            .await
            .expect("same original cancellation task control")
            .await_disposal()
            .await;
        assert!(
            matches!(
                cancellation_terminal.outcome(),
                aura_core::effects::task::RequiredTaskExit::Completed
            ),
            "separate cancellation generation records completed cleanup"
        );
        for agent in [issuer, invitee] {
            if let Err(source) = agent
                .runtime()
                .tasks()
                .shutdown_with_timeout(std::time::Duration::from_secs(2))
                .await
            {
                assert!(agent.runtime().tasks().terminal_failure().is_some(),
                    "actual unresolved required cleanup remains observable during teardown: {source}");
            }
        }
    }

    #[tokio::test]
    async fn public_live_cancellation_retry_preserves_primary_and_original_window() {
        let (issuer, _clock, issued, start) = Box::pin(cancellation_fixture(
            "public-live-cancellation-original-notice",
        ))
        .await;
        let service = issuer.invitations().expect("actual issuer service");
        let tracker = issuer.ceremony_tracker().await;
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original registration");
        let first = Box::pin(service.cancel(&issued.manifest().invitation)).await;
        let retry = Box::pin(service.cancel(&issued.manifest().invitation)).await;
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("retained terminal registration");
        // Drain the genuine original/finite notice owners before assertions,
        // including when either public request fails unexpectedly.
        let drained = issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await;
        first.expect("live required negative publication succeeds");
        retry.expect("negative publication retry remains primary success");
        assert_eq!(
            before.timeout_budget.started_at_ms(),
            after.timeout_budget.started_at_ms()
        );
        assert_eq!(
            before.timeout_budget.deadline_at_ms(),
            after.timeout_budget.deadline_at_ms()
        );
        assert_eq!(
            after.terminal_outcome,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            ),)
        );
        assert!(
            issuer.runtime().tasks().active_tasks().is_empty(),
            "actual notice owners drain"
        );
        if let Err(source) = drained {
            assert!(
                issuer.runtime().tasks().terminal_failure().is_some(),
                "required subsidiary failure remains in health: {source}"
            );
        }
    }

    async fn live_notice_fixture_with_original_continuation(
        label: &str,
    ) -> (
        Arc<crate::AuraAgent>,
        Arc<crate::AuraAgent>,
        aura_testkit::time::ManualPhysicalClock,
        super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        aura_app::runtime_bridge::DeviceEnrollmentStart,
        crate::runtime_bridge::OriginalIssuedEnrollmentContinuation,
    ) {
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let ((issuer, invitee, invitation, start, _accept, _witness), original) =
            Box::pin(super::super::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                label, Some(Arc::new(clock.clone())), None,
            )).await;
        let service = issuer.invitations().expect("actual issuer service");
        let record = service
            .handler
            .created_invitation_required(
                issuer.runtime().effects().clone(),
                &invitation.invitation_id,
            )
            .await
            .expect("actual original sender record");
        let issued = super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(
            &record,
        ).await.expect("strongest original runtime-bound control");
        (issuer, invitee, clock, issued, start, original)
    }

    async fn settle_live_notice_original(
        issuer: &Arc<crate::AuraAgent>,
        issued: &super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &crate::runtime_bridge::OriginalIssuedEnrollmentContinuation,
    ) {
        use crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole;
        let disposal = original
            .original_disposal_for_test()
            .expect("borrow disposal from same original issued task");
        issuer
            .invitations()
            .expect("actual issuer service")
            .cancel(&issued.manifest().invitation)
            .await
            .expect("real original cancellation and required negative settlement");
        let tracker = issuer.ceremony_tracker().await;
        let registration = tracker
            .original_cancellation_task_for_test(issued, OriginalCancellationTaskRole::Publication)
            .expect("observe existing original cancellation entry");
        let cancellation = registration
            .disposal_observer()
            .await
            .expect("same retained cancellation task control");
        let cancelled = cancellation.await_disposal().await;
        assert!(
            matches!(
                cancelled.outcome(),
                aura_core::effects::task::RequiredTaskExit::Completed
            ),
            "actual cancellation publication and cleanup must complete, not ForcedAbort"
        );
        let disposed = disposal.await_disposal().await;
        let aura_core::effects::task::RequiredTaskExit::Failed(source) = disposed.outcome() else {
            panic!("disposed issuer must retain its actual negative domain failure");
        };
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(&source);
        let mut retained_domain = false;
        while let Some(actual) = next {
            retained_domain |= actual
                .downcast_ref::<aura_core::effects::task::RequiredTaskTerminalFailure>()
                .is_some();
            next = actual.source();
        }
        assert!(
            retained_domain,
            "real disposal preserves typed original domain failure"
        );
        assert!(original
            .observes_retained_retirement_acknowledgment()
            .expect("retain original actual negative receipt through notice admission"));
        assert_eq!(
            tracker
                .get(&issued.manifest().ceremony)
                .await
                .expect("same original entry")
                .terminal_outcome,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            ))
        );
    }

    #[tokio::test]
    async fn cancelled_notice_owner_rejects_active_and_preserves_original_interval() {
        let (issuer, _invitee, _clock, issued, start, original) = Box::pin(
            live_notice_fixture_with_original_continuation("cancelled-notice-original-owner"),
        )
        .await;
        let runner = issuer.ceremony_runner().await;
        let tracker = issuer.ceremony_tracker().await;
        let effects = issuer.runtime().effects();
        assert!(runner
            .prepare_cancelled_notice(&issued, effects.as_ref())
            .await
            .is_err());
        let before = tracker
            .get(&start.ceremony_id)
            .await
            .expect("original registered allocation");
        settle_live_notice_original(&issuer, &issued, &original).await;
        let capability = runner
            .prepare_cancelled_notice(&issued, effects.as_ref())
            .await
            .expect("original negative decision mints finite notice custody");
        assert!(
            runner
                .prepare_cancelled_notice(&issued, effects.as_ref())
                .await
                .is_err(),
            "same original semaphore admits only one notice sender"
        );
        let mut window = crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability::registered(
            Arc::new(capability), effects.as_ref(),
        ).expect("original negative owner retained before admission");
        async fn observed_phase(
            effects: &AuraEffectSystem,
            ceremony: &CeremonyId,
        ) -> serde_json::Value {
            use aura_core::effects::{
                SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
            };
            let bytes = effects
                .secure_retrieve(
                    &SecureStorageLocation::new("enrollment_clock_v1", ceremony.to_string()),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("observe actual protected checkpoint; not execution authority");
            serde_json::from_slice::<serde_json::Value>(&bytes).expect("actual checkpoint encoding")
                ["execution"]
                .clone()
        }
        assert_eq!(
            observed_phase(effects.as_ref(), &start.ceremony_id).await,
            serde_json::json!("Refused")
        );
        assert!(
            tracker
                .checkpoint_enrollment_clock(&start.ceremony_id)
                .await
                .is_err(),
            "ordinary positive checkpoint must still refuse Refused"
        );
        window.admit_original(effects.as_ref()).await;
        assert!(matches!(
            window.admitted_observation(),
            Some(aura_core::time::timeout::CheckpointExecution::Finished {
                operation: Ok(()),
                ..
            })
        ));
        let actual = window
            .execute(effects.as_ref(), || async { Ok(()) })
            .await
            .expect("negative observation requires actual checkpoint ACK");
        assert!(matches!(
            actual,
            aura_core::time::timeout::CheckpointExecution::Finished {
                operation: Ok(()),
                ..
            }
        ));
        assert_eq!(
            observed_phase(effects.as_ref(), &start.ceremony_id).await,
            serde_json::json!("Refused")
        );
        let after = tracker
            .get(&start.ceremony_id)
            .await
            .expect("retained negative allocation");
        assert_eq!(
            before.timeout_budget.started_at_ms(),
            after.timeout_budget.started_at_ms()
        );
        assert_eq!(
            before.timeout_budget.deadline_at_ms(),
            after.timeout_budget.deadline_at_ms()
        );
        assert_eq!(
            after.terminal_outcome,
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            ))
        );
        assert!(
            runner
                .enrollment_window_budget(&start.ceremony_id)
                .await
                .is_err(),
            "negative notice custody never reopens ordinary enrollment admission"
        );
    }

    #[tokio::test]
    async fn cancelled_notice_owner_expiry_preserves_cancelled_without_new_window() {
        let (issuer, _invitee, clock, issued, start, original) = Box::pin(
            live_notice_fixture_with_original_continuation("cancelled-notice-original-expiry"),
        )
        .await;
        let runner = issuer.ceremony_runner().await;
        let effects = issuer.runtime().effects();
        settle_live_notice_original(&issuer, &issued, &original).await;
        let state = issuer
            .ceremony_tracker()
            .await
            .get(&start.ceremony_id)
            .await
            .expect("original cancelled allocation");
        let deadline = state
            .timeout_budget
            .deadline_at_ms()
            .min(issued.manifest().expires_at_ms);
        let capability = runner
            .prepare_cancelled_notice(&issued, effects.as_ref())
            .await
            .expect("only original cancelled authority");
        clock.set_time(deadline);
        let mut window = crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability::registered(
            Arc::new(capability), effects.as_ref(),
        ).expect("original owner retained even when admission expires");
        window.admit_original(effects.as_ref()).await;
        let error = match window.admitted_observation() {
            Some(aura_core::time::timeout::CheckpointExecution::Negative { source, .. }) => source,
            _ => panic!("expiry must retain negative admission without sending eligibility"),
        };
        let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
        let mut deadline_source = false;
        while let Some(source) = cause {
            deadline_source |= matches!(
                source
                    .downcast_ref::<aura_invitation::enrollment_manifest::EnrollmentManifestError>(
                    ),
                Some(aura_invitation::enrollment_manifest::EnrollmentManifestError::Expired)
            );
            deadline_source |= matches!(
                source.downcast_ref::<aura_core::TimeoutBudgetError>(),
                Some(aura_core::TimeoutBudgetError::DeadlineExceeded { .. })
            );
            cause = source.source();
        }
        assert!(
            deadline_source,
            "actual original deadline remains in source chain"
        );
        let after = issuer
            .ceremony_tracker()
            .await
            .get(&start.ceremony_id)
            .await
            .expect("original negative readout survives expiry");
        assert_eq!(state.terminal_outcome, after.terminal_outcome);
        assert_eq!(
            state.timeout_budget.deadline_at_ms(),
            after.timeout_budget.deadline_at_ms()
        );
    }
    #[tokio::test]
    async fn cancelled_notice_owner_requires_original_checkpoint_and_retains_storage_failure() {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let (issuer, _invitee, _clock, issued, start, original) = Box::pin(
            live_notice_fixture_with_original_continuation("cancelled-notice-required-checkpoint"),
        )
        .await;
        let runner = issuer.ceremony_runner().await;
        let tracker = issuer.ceremony_tracker().await;
        let effects = issuer.runtime().effects();
        settle_live_notice_original(&issuer, &issued, &original).await;
        let capability = runner
            .prepare_cancelled_notice(&issued, effects.as_ref())
            .await
            .expect("exact original negative allocation");
        tracker
            .fail_next_cancellation_checkpoint_for_test(aura_core::AuraError::Storage {
                message: "controlled negative-owner checkpoint failure".into(),
                source: Some(Arc::new(aura_core::effects::StorageError::WriteFailed(
                    "controlled required secure checkpoint write".into(),
                ))),
            })
            .await;
        let mut window = crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability::registered(
            Arc::new(capability), effects.as_ref(),
        ).expect("retain original owner before checkpoint admission");
        window.admit_original(effects.as_ref()).await;
        let error = match window.admitted_observation() {
            Some(aura_core::time::timeout::CheckpointExecution::Negative { source, .. }) => source,
            _ => panic!("required checkpoint failure cannot mint sending window"),
        };
        assert!(
            !crate::runtime::services::enrollment_window::cancelled_notice_eligibility_ended(error)
        );
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(error);
        let mut storage = false;
        while let Some(source) = next {
            storage |= matches!(
                source.downcast_ref::<aura_core::effects::StorageError>(),
                Some(aura_core::effects::StorageError::WriteFailed(_))
            );
            next = source.source();
        }
        assert!(storage, "required native storage cause remains traversable");
        drop(window);
        effects
            .secure_delete(
                &SecureStorageLocation::new("enrollment_clock_v1", start.ceremony_id.to_string()),
                &[SecureStorageCapability::Delete],
            )
            .await
            .expect("inject actual missing once-live checkpoint");
        assert!(
            runner
                .prepare_cancelled_notice(&issued, effects.as_ref())
                .await
                .is_err(),
            "once-live negative recovery never reconstructs missing original duration"
        );
        assert_eq!(
            runner
                .terminal_outcome(&start.ceremony_id)
                .await
                .expect("required first decision"),
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            ))
        );
    }

    #[tokio::test]
    async fn cancelled_notice_owner_rejects_actual_foreign_runtime() {
        let (issuer, invitee, invitation, start, _, _) = Box::pin(
            super::super::invitation::tests::actual_pinned_device_enrollment_fixture(
                "cancelled-notice-foreign-runtime",
            ),
        )
        .await;
        let service = issuer.invitations().expect("actual issuer service");
        let record = service
            .handler
            .created_invitation_required(issuer.runtime().effects(), &invitation.invitation_id)
            .await
            .expect("actual required issuer custody");
        let issued = super::super::invitation::enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(
            &record,
        ).await.expect("independent original manifest binding");
        let runner = issuer.ceremony_runner().await;
        runner
            .cancel_verified_enrollment(&issued)
            .await
            .expect("actual original negative CAS");
        let error = match runner
            .prepare_cancelled_notice(&issued, invitee.runtime().effects().as_ref())
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("foreign physical runtime cannot receive original negative owner"),
        };
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut owner = false;
        while let Some(source) = next {
            owner |= matches!(source.downcast_ref::<super::super::invitation::enrollment_trust::EnrollmentVerifierError>(),
                Some(super::super::invitation::enrollment_trust::EnrollmentVerifierError::RuntimeOwner));
            next = source.source();
        }
        assert!(owner, "actual runtime binding failure is typed");
        assert_eq!(
            runner
                .terminal_outcome(&start.ceremony_id)
                .await
                .expect("retained negative decision"),
            Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
            ))
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod enrollment_code_owner_tests {
    use super::*;

    #[test]
    fn actual_enrollment_code_uses_original_pinned_signer_and_generic_fallback_refuses_it() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (issuer, invitee, invitation, start, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "required-code-owner",
                ),
            )
            .await;
            let codes = start.manifest_transfer.as_ref().unwrap();
            let selected =
                aura_invitation::enrollment_manifest::decode_initiator_verifier_transfer(
                    &codes.initiator_verifier_code,
                )
                .unwrap();
            let manifest =
                aura_invitation::enrollment_manifest::SignedEnrollmentTrustManifest::decode(
                    &codes.manifest_code,
                )
                .unwrap();
            let (shareable, proof, transport) =
                ShareableInvitation::from_code_with_proof_and_transport(&start.enrollment_code)
                    .unwrap();
            let proof = proof.unwrap();
            assert_eq!(
                proof.public_key.as_slice(),
                selected.verifying_key.as_slice()
            );
            assert_eq!(proof.key_epoch, Some(manifest.manifest.final_epoch));
            assert_eq!(proof.sender_device_id, Some(selected.initiator_device));
            assert!(aura_signature::verify_ed25519_transcript(
                invitee.runtime().effects().as_ref(),
                &shareable.signing_transcript_with_transport(&transport),
                &proof.signature,
                &selected.verifying_key
            )
            .await
            .unwrap());
            let rejected = InvitationServiceApi::export_signed_invitation_with_transport(
                issuer.runtime().effects().as_ref(),
                &invitation,
                &transport,
            )
            .await
            .expect_err("generic export cannot sign an enrollment without its retained owner");
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&rejected);
            let mut missing = false;
            while let Some(cause) = source {
                missing |= matches!(
                    cause.downcast_ref::<super::super::invitation::issued_identity::IssuedInvitationIdentityError>(),
                    Some(super::super::invitation::issued_identity::IssuedInvitationIdentityError::EnrollmentOwnerRequired)
                );
                source = cause.source();
            }
            assert!(
                missing,
                "actual enrollment owner rejection retains typed cause"
            );
            let exported = issuer
                .invitations()
                .unwrap()
                .export_code(&invitation.invitation_id)
                .await
                .unwrap();
            let (reexported, proof, transport) =
                ShareableInvitation::from_code_with_proof_and_transport(&exported).unwrap();
            let proof = proof.unwrap();
            assert_eq!(
                proof.public_key.as_slice(),
                selected.verifying_key.as_slice()
            );
            assert!(aura_signature::verify_ed25519_transcript(
                invitee.runtime().effects().as_ref(),
                &reexported.signing_transcript_with_transport(&transport),
                &proof.signature,
                &selected.verifying_key
            )
            .await
            .unwrap());
        });
    }
}
