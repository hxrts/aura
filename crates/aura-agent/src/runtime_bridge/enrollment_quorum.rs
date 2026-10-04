//! Runtime-owned original-device enrollment signing approval.
//! No deserialization, clone or caller-built effect owner can mint consent.
use std::sync::Arc;

use aura_core::AuraError;
use aura_invitation::enrollment_manifest::EnrollmentTrustManifest;

use super::AgentRuntimeBridge;
use crate::runtime::AuraEffectSystem;

pub(crate) struct OriginalPreparedIssuerReady {
    observed: aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning,
    observer: crate::runtime::services::enrollment_window::HeldIssuerCompletionObserver,
    intent_digest: [u8; 32],
}

impl OriginalPreparedIssuerReady {
    pub(crate) fn into_owned_parts(
        self,
    ) -> (
        aura_app::runtime_bridge::PreparedDeviceEnrollmentSigning,
        crate::runtime::services::enrollment_window::HeldIssuerCompletionObserver,
        [u8; 32],
    ) {
        (self.observed, self.observer, self.intent_digest)
    }
}

pub(crate) struct OriginalIssuerPreparation {
    ready: tokio::sync::oneshot::Sender<OriginalPreparedIssuerReady>,
    approved: tokio::sync::oneshot::Receiver<RuntimeApprovedEnrollmentSigningIntent>,
}

impl OriginalIssuerPreparation {
    pub(crate) fn owned_channels() -> (
        Self,
        tokio::sync::oneshot::Receiver<OriginalPreparedIssuerReady>,
        tokio::sync::oneshot::Sender<RuntimeApprovedEnrollmentSigningIntent>,
    ) {
        let (ready, ready_receiver) = tokio::sync::oneshot::channel();
        let (approved_sender, approved) = tokio::sync::oneshot::channel();
        (Self { ready, approved }, ready_receiver, approved_sender)
    }

    pub(super) async fn require_original_user_approval(
        self,
        effects: Arc<AuraEffectSystem>,
        manifest: &EnrollmentTrustManifest,
        public_transport: &aura_invitation::shareable::PublicEnrollmentTransportSigningIntent,
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        window: &crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    ) -> Result<RuntimeApprovedEnrollmentSigningIntent, AuraError> {
        use base64::Engine;
        let intent = aura_invitation::enrollment_signing_intent::EnrollmentSigningIntent::new(
            manifest.clone(),
            public_transport.clone(),
        )?;
        let canonical = aura_core::util::serialization::to_vec(&intent)?;
        let intent_digest = aura_core::hash::hash(&canonical);
        let code = format!(
            "aura-enrollment-signing-intent:v2:{}",
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
                let observer = window
                    .held_issuer_completion_observer(effects.as_ref())
                    .await?;
                self.ready
                    .send(OriginalPreparedIssuerReady {
                        observed,
                        observer,
                        intent_digest,
                    })
                    .map_err(|_| {
                        AuraError::invalid("original prepared issuer observation was cancelled")
                    })?;
                let approved = self.approved.await.map_err(|source| {
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
        canonical_intent_digest: aura_core::hash::hash(&canonical),
        setup: approval.setup().clone(),
    })
}
