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

const DEFERRED_INVITATION_DELIVERY_ATTEMPTS: usize = 12;
const DEFERRED_INVITATION_DELIVERY_BACKOFF_MS: u64 = 500;

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
        public.require_manifest(manifest).map_err(|source| {
            AgentError::EnrollmentManifest(
                aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(Box::new(
                    source,
                )),
            )
        })?;
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
            || *invitee_authority != Some(manifest.invitee_authority)
            || *initiator_device_id != manifest.initiator_device
            || *device_id != manifest.invitee_device
            || *ceremony_id != manifest.ceremony
            || *pending_epoch != manifest.pending_epoch
            || setup_binding.as_ref() != Some(&manifest.setup)
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
        approved
            .require_manifest(issued.manifest())
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
    let terminal_publication = runner
        .fail_with_reason(ceremony, reason, Some(execution.to_string()))
        .await
        .err();
    let failure = EnrollmentInitiatorTaskFailure {
        execution,
        terminal_publication,
    };
    aura_core::AuraError::Internal {
        message: failure.to_string(),
        source: Some(Arc::new(failure)),
    }
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

/// Admission outcome for the actual original registered execution lease.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DeviceEnrollmentInitiatorStart {
    Started,
    AlreadyRunning,
}

impl InvitationServiceApi {
    /// Create a new invitation service with shared runtime-owned supervisors.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
        tasks: Arc<TaskSupervisor>,
    ) -> AgentResult<Self> {
        let handler = InvitationHandler::new(authority_context)?;
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
            tasks,
        })
    }

    fn spawn_channel_invitation_exchange(&self, invitation: &Invitation) {
        if invitation.receiver_id == invitation.sender_id {
            return;
        }

        let invitation = invitation.clone();
        let handler = self.handler.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.channel_exchange.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let fut = async move {
            if let Err(error) = handler
                .execute_channel_invitation_exchange_sender(effects, &invitation)
                .await
            {
                tracing::error!(
                    invitation_id = %invitation_id,
                    sender_id = %sender_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "channel invitation exchange sender failed"
                );
            }
        };

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_named("sender_exchange", fut);
            } else {
                let _task_handle = tasks.spawn_named("sender_exchange", fut);
            }
        }
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

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentWindowCapability",
        family = "runtime_helper"
    )]
    fn spawn_device_enrollment_initiator(
        &self,
        invitation: &Invitation,
        budget: crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    ) -> AgentResult<()> {
        if invitation.receiver_id == invitation.sender_id {
            return Err(AgentError::internal(
                "device enrollment requires a distinct invitee",
            ));
        }

        let invitation = invitation.clone();
        let handler = self.handler.clone();
        let ceremony_runner = self.ceremony_runner.clone();
        let effects = self.effects.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.device_enrollment.{}",
            invitation.invitation_id
        ));
        let invitation_id = invitation.invitation_id.clone();
        let ceremony_id = match &invitation.invitation_type {
            InvitationType::DeviceEnrollment { ceremony_id, .. } => ceremony_id.clone(),
            _ => {
                return Err(AgentError::internal(
                    "Expected DeviceEnrollment invitation type",
                ))
            }
        };
        let sender_id = invitation.sender_id;
        let receiver_id = invitation.receiver_id;
        let fut = Box::pin(async move {
            if let Err(error) = handler
                .execute_device_enrollment_initiator_owned(
                    effects,
                    &invitation,
                    ceremony_runner.clone(),
                    budget,
                )
                .await
            {
                tracing::error!(
                    invitation_id = %invitation_id,
                    sender_id = %sender_id,
                    receiver_id = %receiver_id,
                    error = %error,
                    "device enrollment initiator choreography failed"
                );
                return Err(settle_required_enrollment_initiator_failure(
                    &ceremony_runner,
                    &ceremony_id,
                    error,
                )
                .await);
            }
            Ok::<(), aura_core::AuraError>(())
        });

        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task_handle = tasks.spawn_local_try_named("device_enrollment_initiator", fut);
            } else {
                let _task_handle = tasks.spawn_try_named("device_enrollment_initiator", fut);
            }
        }
        if let Some(source) = tasks.terminal_failure() {
            return Err(AgentError::from(aura_core::AuraError::Internal {
                message: "admit registered enrollment initiator task".into(),
                source: Some(Arc::new(source)),
            }));
        }
        Ok(())
    }

    /// Bootstrap selector is used once; the task receives only issued control
    /// and a sealed Cancelled original-window owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "CancelledEnrollmentNoticeRecoveryCapability",
        family = "runtime_helper"
    )]
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
        let registered = match self.ceremony_runner.prepare_cancelled_notice(
            &issued, self.effects.as_ref(),
        ).await {
            Ok(owner) => owner,
            Err(cause) if crate::runtime::services::ceremony_tracker::registered_enrollment_window_already_owned(&cause) => {
                return Ok(None);
            }
            Err(cause) => return Err(AgentError::from(cause)),
        };
        Ok(Some(CancelledEnrollmentNoticeRecoveryCapability {
            effects: self.effects.clone(),
            issued,
            registered,
        }))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "CancelledEnrollmentNoticeRecoveryCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn start_cancelled_enrollment_notice_recovery(
        &self,
        capability: CancelledEnrollmentNoticeRecoveryCapability,
    ) -> AgentResult<()> {
        if !Arc::ptr_eq(&capability.effects, &self.effects) {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RuntimeOwner.into(),
            );
        }
        let CancelledEnrollmentNoticeRecoveryCapability {
            effects,
            issued,
            registered,
        } = capability;
        let runner = self.ceremony_runner.clone();
        let tasks = self.tasks.group(format!(
            "invitation_service.cancelled_notice.{}",
            issued.manifest().invitation,
        ));
        let fut = Box::pin(async move {
            let window = match runner.cancelled_notice_window(registered, effects.as_ref()).await? {
                crate::runtime::services::enrollment_window::CancelledNoticeWindowAdmission::Eligible(window) => window,
                crate::runtime::services::enrollment_window::CancelledNoticeWindowAdmission::EligibilityEnded { cause } => {
                    tracing::debug!(error = %cause, "cancelled enrollment notice eligibility ended; no send");
                    return Ok::<(), aura_core::AuraError>(());
                }
            };
            super::invitation::execute_recovered_cancelled_notice(
                effects, issued, runner, window,
            ).await.or_else(|cause| {
                if crate::runtime::services::enrollment_window::cancelled_notice_eligibility_ended(&cause) {
                    tracing::debug!(error = %cause, "cancelled enrollment notice original eligibility ended; no send");
                    Ok(())
                } else {
                    Err(cause)
                }
            }).map_err(|source| match source {
                AgentError::Aura(cause) => cause,
                source => aura_core::AuraError::Internal {
                    message: "required cancelled enrollment notice execution".into(),
                    source: Some(Arc::new(source)),
                },
            })
        });
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _task = tasks.spawn_local_try_named("cancelled_enrollment_notice_recovery", fut);
            } else {
                let _task = tasks.spawn_try_named("cancelled_enrollment_notice_recovery", fut);
            }
        }
        if let Some(source) = tasks.terminal_failure() {
            return Err(AgentError::from(aura_core::AuraError::Internal {
                message: "admit original cancelled enrollment notice task".into(),
                source: Some(Arc::new(source)),
            }));
        }
        Ok(())
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
        self.spawn_channel_invitation_exchange(&invitation);
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
                    aura_invitation::enrollment_manifest::EnrollmentManifestError::Transcript(e),
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

    #[allow(clippy::too_many_arguments)]
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentGenerationCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn start_registered_device_enrollment(
        &self,
        registered: &crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'_>,
    ) -> AgentResult<DeviceEnrollmentInitiatorStart> {
        Box::pin(async move {
        registered
            .require_effects(self.effects.as_ref())
            .map_err(AgentError::from)?;
        let invitation = registered.canonical_invitation();
        let InvitationType::DeviceEnrollment {
            initiator_device_id,
            ..
        } = &invitation.invitation_type
        else {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        };
        if invitation.sender_id != self.handler.authority_context().authority_id()
            || *initiator_device_id != self.effects.device_id()
        {
            return Err(
                super::invitation::enrollment_trust::EnrollmentVerifierError::RecordBinding.into(),
            );
        }
        let budget = match self.ceremony_runner.registered_enrollment_generation_window(registered).await {
            Ok(budget) => budget,
            Err(source) if crate::runtime::services::ceremony_tracker::registered_enrollment_window_already_owned(&source) => return Ok(DeviceEnrollmentInitiatorStart::AlreadyRunning),
            Err(source) => return Err(AgentError::from(source)),
        };
        self.spawn_device_enrollment_initiator(registered.canonical_invitation(), budget)?;
        Ok(DeviceEnrollmentInitiatorStart::Started)
        }).await
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
                setup_binding: Some(setup_binding),
                subject_authority,
                invitee_authority: Some(receiver_id),
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

    /// Keep the shared caller future bounded while retaining lexical ownership.
    async fn accept_owned(&self, invitation_id: &InvitationId) -> AgentResult<InvitationResult> {
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
            if matches!(
                invitation.invitation_type,
                InvitationType::DeviceEnrollment { .. }
            ) {
                // The signed acceptance choreography is the authoritative step:
                // enrollment must not report success if the initiator never
                // received and verified it.
                self.handler
                    .execute_device_enrollment_invitee(
                        self.effects.clone(),
                        &invitation,
                        &self.tasks.group("invitation_service.enrollment_receiver"),
                    )
                    .await?;
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
        let preparation = self
            .ceremony_runner
            .original_cancellation_preparation(&issued, self.effects.as_ref())
            .await
            .map_err(AgentError::from)?;
        let result = match preparation {
            crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability::Active(window) => {
                window.execute(self.effects.as_ref(), || Box::pin(async {
                    let prepared = self.handler.prepare_enrollment_cancellation(&issued, record).await?;
                    let cancelled = self.ceremony_runner.cancel_verified_enrollment(&issued)
                        .await.map_err(AgentError::from)?;
                    self.handler.publish_verified_enrollment_cancellation(
                        self.effects.clone(), prepared, cancelled,
                    ).await
                })).await.map_err(|source| window.map_run_error("original cancellation preparation", source))
            }
            crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability::Decided(cancelled) => {
                // This is a persisted negative decision, never a new live
                // window or permission to admit/sign a protocol session.
                let prepared = self.handler.prepare_enrollment_cancellation(&issued, record).await?;
                self.handler.publish_verified_enrollment_cancellation(
                    self.effects.clone(), prepared, cancelled,
                ).await
            }
        }?;
        // Required negative publication completes before notice admission. A
        // retained original execution owner already sending the notice is the
        // only admission fault treated as a normal duplicate disposition.
        let notice = match self.prepare_cancelled_notice_from_issued(issued).await {
            Ok(Some(capability)) => self.start_cancelled_enrollment_notice_recovery(capability),
            Ok(None) => Ok(()),
            Err(source) => Err(source),
        };
        if let Err(source) = notice {
            let cause = match source {
                AgentError::Aura(cause) => cause,
                source => aura_core::AuraError::Internal {
                    message: "required cancelled enrollment notice admission".into(),
                    source: Some(Arc::new(source)),
                },
            };
            self.tasks
                .group("invitation_service.cancelled_notice_admission")
                .record_subsidiary_failure("original-negative-notice", cause);
        }
        Ok(result)
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
            || *invitee_authority != Some(manifest.invitee_authority)
            || *initiator_device_id != manifest.initiator_device
            || *device_id != manifest.invitee_device
            || *ceremony_id != manifest.ceremony
            || *pending_epoch != manifest.pending_epoch
            || setup_binding.as_ref() != Some(&manifest.setup)
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
            AgentError::EnrollmentManifest(EnrollmentManifestError::Transcript(source))
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
        _legacy_allow_ephemeral_fallback: bool,
    ) -> AgentResult<String> {
        if matches!(
            invitation.invitation_type,
            InvitationType::DeviceEnrollment { .. }
        ) {
            return Err(invitation_shareable_failure(
                ShareableInvitationError::MissingEnrollmentSetupBinding,
            ));
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
        _legacy_allow_ephemeral_fallback: bool,
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
            .export_signed_invitation(
                invitation,
                &transport,
                self.effects.is_testing() || self.effects.harness_mode_enabled(),
            )
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

fn invitation_shareable_failure(source: ShareableInvitationError) -> AgentError {
    let cause = Arc::new(source);
    let error = match cause.as_ref() {
        ShareableInvitationError::SerializationFailed => aura_core::AuraError::Serialization {
            message: "serialize invitation transfer".into(),
            source: Some(cause),
        },
        ShareableInvitationError::InvalidSenderProof
        | ShareableInvitationError::VerificationFailed => aura_core::AuraError::Crypto {
            message: "verify invitation transfer".into(),
            source: Some(cause),
        },
        ShareableInvitationError::InvalidFormat
        | ShareableInvitationError::UnsupportedVersion(_)
        | ShareableInvitationError::SizeLimitExceeded(_)
        | ShareableInvitationError::DecodingFailed
        | ShareableInvitationError::ParsingFailed
        | ShareableInvitationError::MissingSenderProof
        | ShareableInvitationError::MissingChannelContext
        | ShareableInvitationError::MissingEnrollmentSetupBinding
        | ShareableInvitationError::Expired => aura_core::AuraError::Invalid {
            message: "validate invitation transfer".into(),
            source: Some(cause),
        },
    };
    AgentError::from(error)
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

    #[cfg(not(target_arch = "wasm32"))]
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
        let EnrollmentCancellationPreparationCapability::Active(window) = preparation else {
            panic!("original active fixture must retain preparation eligibility");
        };
        window
            .execute(issuer.runtime().effects().as_ref(), || async { Ok(()) })
            .await
            .expect("required original checkpoint is acknowledged");
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
        let EnrollmentCancellationPreparationCapability::Active(window) = preparation else {
            panic!("active original window expected");
        };
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
            outcome = &mut pending => panic!("original operation must first enter: {outcome:?}"),
            entered = entered_rx => entered.expect("required operation actually entered"),
        }
        clock.set_time(deadline);
        let error = pending
            .await
            .expect_err("original deadline cancels pending preparation");
        assert!(matches!(error, aura_core::TimeoutRunError::Timeout(
            aura_core::TimeoutBudgetError::DeadlineExceeded { deadline_at_ms, .. }
        ) if deadline_at_ms == deadline));
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
        let EnrollmentCancellationPreparationCapability::Active(window) = preparation else {
            panic!("active original window expected");
        };
        tracker
            .fail_next_cancellation_checkpoint_for_test(aura_core::AuraError::Storage {
                message: "injected required cancellation checkpoint write".into(),
                source: Some(Arc::new(aura_core::effects::StorageError::WriteFailed(
                    "controlled original owner checkpoint failure".into(),
                ))),
            })
            .await;
        let entered = std::cell::Cell::new(false);
        let error = window
            .execute(issuer.runtime().effects().as_ref(), || async {
                entered.set(true);
                Ok(())
            })
            .await
            .expect_err("required checkpoint failure blocks preparation");
        assert!(!entered.get());
        let mapped = window.map_run_error("required cancellation checkpoint", error);
        let mut current: &(dyn Error + 'static) = &mapped;
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
        let config = crate::core::AgentConfig::default();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_named_test(
                &config,
                "required_window_rejection_retains_execution_and_settlement_sources",
            )
            .expect("distinct simulation effects"),
        );
        let time: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
        let tracker = CeremonyTracker::new_with_storage(time, effects.clone());
        let runner = CeremonyRunner::new(tracker.clone());
        let missing = CeremonyId::new("required-window-unregistered".to_owned());
        let rejection = match effects
            .resume_owned_enrollment_registration(
                &tracker,
                effects.runtime_authority_id(),
                0,
                &missing,
                aura_core::Hash32::new([0; 32]),
            )
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("unregistered ceremony must not recover execution authority"),
        };
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

    #[tokio::test]
    async fn cancelled_notice_owner_rejects_active_and_preserves_original_interval() {
        let (issuer, _clock, issued, start) =
            Box::pin(cancellation_fixture("cancelled-notice-original-owner")).await;
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
        runner
            .cancel_verified_enrollment(&issued)
            .await
            .expect("actual durable Cancelled CAS");
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .expect("ordinary cancellation drops actual original VM owner");
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
        let admission = runner
            .cancelled_notice_window(capability, effects.as_ref())
            .await
            .expect("required original eligibility read");
        let crate::runtime::services::enrollment_window::CancelledNoticeWindowAdmission::Eligible(
            window,
        ) = admission
        else {
            panic!("unexpired original signed validity must permit finite notice");
        };
        window
            .execute(effects.as_ref(), || async { Ok(()) })
            .await
            .expect("negative observation requires actual checkpoint ACK");
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
        let (issuer, clock, issued, start) =
            Box::pin(cancellation_fixture("cancelled-notice-original-expiry")).await;
        let runner = issuer.ceremony_runner().await;
        let effects = issuer.runtime().effects();
        runner
            .cancel_verified_enrollment(&issued)
            .await
            .expect("genuine negative first decision");
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .expect("drop actual VM before negative restart observation");
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
        let error = match runner.cancelled_notice_window(capability, effects.as_ref()).await {
            Ok(crate::runtime::services::enrollment_window::CancelledNoticeWindowAdmission::EligibilityEnded { cause }) => cause,
            Ok(_) => panic!("expiry must not allocate negative sending eligibility"),
            Err(cause) => panic!("known expiry is no-send, not required service fault: {cause}"),
        };
        let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
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
        let (issuer, _clock, issued, start) =
            Box::pin(cancellation_fixture("cancelled-notice-required-checkpoint")).await;
        let runner = issuer.ceremony_runner().await;
        let tracker = issuer.ceremony_tracker().await;
        let effects = issuer.runtime().effects();
        runner
            .cancel_verified_enrollment(&issued)
            .await
            .expect("actual Cancelled first decision");
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .expect("actual original execution owner drained");
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
        let error = match runner
            .cancelled_notice_window(capability, effects.as_ref())
            .await
        {
            Err(error) => error,
            Ok(_) => {
                panic!("required checkpoint failure cannot mint sending window or expected expiry")
            }
        };
        assert!(
            !crate::runtime::services::enrollment_window::cancelled_notice_eligibility_ended(
                &error
            )
        );
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut storage = false;
        while let Some(source) = next {
            storage |= matches!(
                source.downcast_ref::<aura_core::effects::StorageError>(),
                Some(aura_core::effects::StorageError::WriteFailed(_))
            );
            next = source.source();
        }
        assert!(storage, "required native storage cause remains traversable");
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
                true,
            )
            .await
            .expect_err("testing fallback cannot sign an enrollment without its retained owner");
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&rejected);
            let mut missing = false;
            while let Some(cause) = source {
                missing |= matches!(
                    cause.downcast_ref::<ShareableInvitationError>(),
                    Some(ShareableInvitationError::MissingEnrollmentSetupBinding)
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

    #[test]
    fn general_invitation_failure_keeps_its_domain_category_and_actual_enum_source() {
        enum Expected {
            Serialization,
            Crypto,
            Invalid,
        }
        for (original, expected) in [
            (
                ShareableInvitationError::SerializationFailed,
                Expected::Serialization,
            ),
            (
                ShareableInvitationError::VerificationFailed,
                Expected::Crypto,
            ),
            (
                ShareableInvitationError::MissingChannelContext,
                Expected::Invalid,
            ),
        ] {
            let retained = original.clone();
            let failure = invitation_shareable_failure(original);
            let AgentError::Aura(error) = failure else {
                panic!("general transfer error cannot become an enrollment admission failure")
            };
            assert!(matches!(
                (&error, expected),
                (
                    aura_core::AuraError::Serialization { .. },
                    Expected::Serialization
                ) | (aura_core::AuraError::Crypto { .. }, Expected::Crypto)
                    | (aura_core::AuraError::Invalid { .. }, Expected::Invalid)
            ));
            assert_eq!(
                std::error::Error::source(&error)
                    .unwrap()
                    .downcast_ref::<ShareableInvitationError>()
                    .unwrap(),
                &retained
            );
        }
    }
}
