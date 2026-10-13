//! Runtime-owned original-device enrollment signing approval.
//! No deserialization, clone or caller-built effect owner can mint consent.
use std::sync::Arc;

use aura_core::AuraError;
use aura_invitation::enrollment_manifest::EnrollmentTrustManifest;

use super::AgentRuntimeBridge;
use crate::runtime::AuraEffectSystem;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginalIssuerLaunchMode {
    SingleSigner,
    UserApprovedQuorum,
}

pub(crate) enum OriginalPreparedIssuerReady {
    UserApproval(aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning),
    Issued,
}
impl OriginalPreparedIssuerReady {
    pub(crate) fn into_observed(
        self,
    ) -> Result<aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning, AuraError> {
        match self {
            Self::UserApproval(observed) => Ok(observed),
            Self::Issued => Err(AuraError::invalid(
                "ordinary issuance cannot supply quorum signing readiness",
            )),
        }
    }
}
enum OriginalIssuerApprovalIngress {
    SingleSigner,
    UserApproved(tokio::sync::oneshot::Receiver<RuntimeApprovedEnrollmentSigningIntent>),
}

// Synchronous readiness transfer: guards only take the sender before any await.
#[allow(clippy::disallowed_types)]
type OriginalIssuerReadyCell = Arc<
    parking_lot::Mutex<
        Option<tokio::sync::oneshot::Sender<Result<OriginalPreparedIssuerReady, AuraError>>>,
    >,
>;

pub(crate) struct OriginalIssuerPreparation {
    pub(crate) ready: OriginalIssuerReadyCell,
    approval: OriginalIssuerApprovalIngress,
    pub(crate) retained: Arc<tokio::sync::Mutex<OriginalPreparedIssuerResources>>,
    pub(crate) registration: Arc<
        std::sync::OnceLock<
            crate::runtime::services::threshold_signing::OriginalPreparedIssuerRegistration,
        >,
    >,
}

/// The required task and its borrowed issuance frame share this one custody
/// cell. The frame deposits original resources; cleanup never reconstructs them.
#[derive(Default)]
pub(crate) struct OriginalPreparedIssuerResources {
    pub(crate) issuer_reservation:
        Option<Arc<crate::runtime::effects::EnrollmentGenerationReservation<'static>>>,
    pub(crate) issuer_inventory: Option<
        Arc<crate::runtime::effects::EnrollmentFinalVerifierInventoryCapability<'static, 'static>>,
    >,
    pub(crate) issuer_signing: Option<
        Arc<
            tokio::sync::Mutex<
                crate::runtime::services::threshold_signing::OriginalIssuerTranscriptSigningCustody,
            >,
        >,
    >,
    pub(crate) allocation_checkpoint:
        crate::runtime::services::enrollment_window::OriginalAllocationCheckpointCustody,
    pub(crate) allocation:
        Option<crate::runtime::effects::OriginalEnrollmentAllocationProducer<'static>>,
    pub(crate) launch:
        Option<crate::runtime::services::enrollment_window::OriginalIssuerLaunchWindow>,
    pub(crate) reservation:
        Option<crate::runtime::effects::EnrollmentGenerationReservation<'static>>,
    pub(crate) root: Option<
        crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
    >,
    pub(crate) registered:
        Option<crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'static>>,
    pub(crate) negative:
        Option<crate::runtime::services::enrollment_window::OriginalHeldIssuerNegativeCleanupOwner>,
    pub(crate) registered_custody:
        Option<crate::handlers::invitation_service::OriginalIssuerTaskCustody>,
    pub(crate) first_failure: Option<AuraError>,
}

impl OriginalIssuerPreparation {
    pub(crate) fn owned_channels(
        mode: OriginalIssuerLaunchMode,
        launch: crate::runtime::services::enrollment_window::OriginalIssuerLaunchWindow,
    ) -> (
        Self,
        tokio::sync::oneshot::Receiver<Result<OriginalPreparedIssuerReady, AuraError>>,
        Option<tokio::sync::oneshot::Sender<RuntimeApprovedEnrollmentSigningIntent>>,
    ) {
        let (ready, ready_receiver) = tokio::sync::oneshot::channel();
        let (approval, approved_sender) = match mode {
            OriginalIssuerLaunchMode::SingleSigner => {
                (OriginalIssuerApprovalIngress::SingleSigner, None)
            }
            OriginalIssuerLaunchMode::UserApprovedQuorum => {
                let (sender, receiver) = tokio::sync::oneshot::channel();
                (
                    OriginalIssuerApprovalIngress::UserApproved(receiver),
                    Some(sender),
                )
            }
        };
        (
            Self {
                // Runtime assembly of the synchronous readiness transfer cell.
                #[allow(clippy::disallowed_types)]
                ready: Arc::new(parking_lot::Mutex::new(Some(ready))),
                approval,
                retained: Arc::new(tokio::sync::Mutex::new(OriginalPreparedIssuerResources {
                    launch: Some(launch),
                    ..Default::default()
                })),
                registration: Arc::new(std::sync::OnceLock::new()),
            },
            ready_receiver,
            approved_sender,
        )
    }

    pub(super) fn requires_user_approval(&self) -> bool {
        matches!(
            self.approval,
            OriginalIssuerApprovalIngress::UserApproved(_)
        )
    }

    pub(super) async fn require_original_user_approval(
        self,
        effects: Arc<AuraEffectSystem>,
        manifest: &EnrollmentTrustManifest,
        public_transport: &aura_invitation::shareable::PublicEnrollmentTransportSigningIntent,
        return_credential_commitment: [u8; 32],
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        window: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<RuntimeApprovedEnrollmentSigningIntent, AuraError> {
        let OriginalIssuerApprovalIngress::UserApproved(approved) = self.approval else {
            return Err(AuraError::permission_denied(
                "ordinary issuance has no quorum approval ingress",
            ));
        };
        use base64::Engine;
        let intent = aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent::new(
            manifest.clone(),
            public_transport.clone(),
            return_credential_commitment,
        )?;
        let canonical = aura_core::util::serialization::to_vec(&intent)?;
        let intent_digest = aura_core::hash::hash(&canonical);
        let code = format!(
            "aura-enrollment-signing-intent:v4:{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&canonical)
        );
        let observed = aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning {
            ceremony_id: manifest.ceremony.clone(),
            signing_intent_code: code,
            setup_transfer_code: setup.transfer_code().map_err(|source| {
                AuraError::crypto_with_source(
                    "transfer originally selected setup proof",
                    Arc::new(source),
                )
            })?,
        };
        window
            .execute(effects.as_ref(), || async {
                self.registration
                    .get()
                    .ok_or_else(|| {
                        AuraError::invalid("original preparation registration is absent")
                    })?
                    .bind_original_approval(&effects, &manifest.ceremony, intent_digest)?;
                #[cfg(test)]
                eprintln!("enrollment initiation stage: original approval registration bound");
                self.ready
                    .lock()
                    .take()
                    .ok_or_else(|| {
                        AuraError::invalid("original preparation was already published")
                    })?
                    .send(Ok(OriginalPreparedIssuerReady::UserApproval(observed)))
                    .map_err(|_| {
                        AuraError::invalid("original prepared issuer observation was cancelled")
                    })?;
                #[cfg(test)]
                eprintln!("enrollment initiation stage: original prepared-ready published");
                let approved = approved.await.map_err(|source| {
                    AuraError::crypto_with_source(
                        "original prepared issuer approval ingress ended",
                        Arc::new(source),
                    )
                })?;
                if !Arc::ptr_eq(&effects, approved.effects())
                    || approved.canonical_intent_digest() != intent_digest
                {
                    return Err(AuraError::permission_denied(
                        "local approval is not the original prepared issuer's exact public intent",
                    ));
                }
                Ok(approved)
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "original held issuer approval window",
                    Arc::new(source),
                )
            })
    }
}

pub(crate) struct RuntimeApprovedEnrollmentSigningIntent {
    effects: Arc<AuraEffectSystem>,
    manifest: EnrollmentTrustManifest,
    transport: aura_invitation::shareable::PublicEnrollmentTransportSigningIntent,
    initial_request:
        aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript,
    issuer_return_mailbox: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnStatement,
    negative_terminal_policy:
        aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalPolicy,
    canonical_intent_digest: [u8; 32],
    setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
}

impl RuntimeApprovedEnrollmentSigningIntent {
    pub(crate) fn effects(&self) -> &Arc<AuraEffectSystem> {
        &self.effects
    }
    pub(crate) fn manifest(&self) -> &EnrollmentTrustManifest {
        &self.manifest
    }
    pub(crate) fn transport(
        &self,
    ) -> &aura_invitation::shareable::PublicEnrollmentTransportSigningIntent {
        &self.transport
    }
    pub(crate) fn initial_request(
        &self,
    ) -> &aura_invitation::enrollment_initial_request::EnrollmentInitialRequestTranscript {
        &self.initial_request
    }
    pub(crate) fn issuer_return_mailbox(
        &self,
    ) -> &aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnStatement {
        &self.issuer_return_mailbox
    }
    pub(crate) fn negative_terminal_policy(
        &self,
    ) -> &aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalPolicy {
        &self.negative_terminal_policy
    }
    pub(crate) fn canonical_intent_digest(&self) -> [u8; 32] {
        self.canonical_intent_digest
    }
    pub(crate) fn setup(
        &self,
    ) -> &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup {
        &self.setup
    }
}

/// Called only by the explicit app approval bridge method. The transferred
/// intent has identification semantics until the original app runtime and
/// protected local signing material have both been checked.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "RuntimeApprovedEnrollmentSigningIntent",
    family = "authorizer"
)]
pub(super) fn admit_original_runtime_approval(
    bridge: &AgentRuntimeBridge,
    approval: aura_app::ui::workflows::ceremonies::UserApprovedEnrollmentSigningIntent,
) -> Result<RuntimeApprovedEnrollmentSigningIntent, AuraError> {
    approval.require_runtime_owner(bridge)?;
    let effects = bridge.agent.runtime().effects();
    let intent = aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent::new(
        approval.manifest().clone(),
        approval.transport().clone(),
        approval
            .issuer_return_mailbox()
            .retrieval_credential_commitment(),
    )?;
    let canonical = aura_core::util::serialization::to_vec(&intent).map_err(|source| {
        AuraError::Serialization {
            message: "bind original runtime enrollment signing approval".into(),
            source: Some(Arc::new(source)),
        }
    })?;
    if canonical != approval.canonical_intent() {
        return Err(AuraError::invalid(
            "original user approval canonical binding changed",
        ));
    }
    Ok(RuntimeApprovedEnrollmentSigningIntent {
        effects,
        manifest: approval.manifest().clone(),
        transport: approval.transport().clone(),
        initial_request: approval.initial_request().clone(),
        issuer_return_mailbox: approval.issuer_return_mailbox().clone(),
        negative_terminal_policy: approval.negative_terminal_policy().clone(),
        canonical_intent_digest: aura_core::hash::hash(&canonical),
        setup: approval.setup().clone(),
    })
}

#[cfg(test)]
pub(crate) fn admit_original_runtime_approval_for_test(
    bridge: &AgentRuntimeBridge,
    approval: aura_app::ui::workflows::ceremonies::UserApprovedEnrollmentSigningIntent,
) -> Result<RuntimeApprovedEnrollmentSigningIntent, AuraError> {
    admit_original_runtime_approval(bridge, approval)
}
