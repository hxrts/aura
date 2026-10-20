//! Original enrollment preparation remains owned by one runtime task.
use super::*;

/// Reserve the original issuer's reverse-mailbox credential before asking the
/// user to approve its public commitment. Retrying the same original manifest
/// reuses the encrypted immutable secret; it never allocates a fresh route.
async fn reserve_original_issuer_return_credential(
    effects: &crate::runtime::AuraEffectSystem,
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
) -> Result<String, aura_core::AuraError> {
    use aura_core::effects::secure::{
        ImmutableSecureStoreOutcome, SecureStorageCapability, SecureStorageEffects,
        SecureStorageLocation,
    };
    use aura_signature::SecurityTranscript;
    let digest = aura_core::hash::hash(&manifest.transcript_bytes().map_err(|source| {
        aura_core::AuraError::crypto_with_source(
            "encode original issuer return mailbox manifest",
            Arc::new(source),
        )
    })?);
    let location = SecureStorageLocation::with_sub_key(
        "issuer_return_mailbox_credential_v1",
        manifest.subject.to_string(),
        format!("{}:{}", manifest.invitation, hex::encode(digest)),
    );
    let candidate = effects.random_bytes(32).await;
    if candidate.len() != 32 {
        return Err(aura_core::AuraError::invalid(
            "issuer return credential source returned invalid length",
        ));
    }
    let encoded = hex::encode(candidate);
    let outcome = effects
        .secure_store_immutable(
            &location,
            encoded.as_bytes(),
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
                SecureStorageCapability::Delete,
            ],
        )
        .await?;
    let selected = match outcome {
        ImmutableSecureStoreOutcome::Created => encoded,
        ImmutableSecureStoreOutcome::AlreadyExists => {
            let bytes = effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await?;
            String::from_utf8(bytes).map_err(|source| aura_core::AuraError::Serialization {
                message: "decode retained issuer return mailbox credential".into(),
                source: Some(Arc::new(source)),
            })?
        }
    };
    if selected.len() != 64 || hex::decode(&selected).is_err() {
        return Err(aura_core::AuraError::invalid(
            "retained issuer return mailbox credential is malformed",
        ));
    }
    Ok(selected)
}

/// The live issuer retains the exact selected provider and signed registration
/// until the broker has consumed the persisted original. This is not a V3
/// protected-birth capability: the selected provider still has no verified
/// cross-layer writer bridge.
struct RetainedOriginalIssuerReturnPublication {
    selected: Arc<crate::runtime::AuraEffectSystem>,
    manifest_digest: [u8; 32],
    encoded: Vec<u8>,
}

fn require_original_return_publication<T>(
    retained: &Arc<T>,
    selected: &Arc<T>,
    retained_digest: [u8; 32],
    issued_digest: [u8; 32],
    retained_bytes: &[u8],
    stored_bytes: &[u8],
) -> Result<(), aura_core::AuraError> {
    if !Arc::ptr_eq(retained, selected)
        || retained_digest != issued_digest
        || retained_bytes != stored_bytes
    {
        return Err(aura_core::AuraError::permission_denied(
            "issuer return publication differs from original selected owner",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod original_return_publication_tests {
    use super::require_original_return_publication;
    use aura_core::AuraError;
    use std::sync::Arc;

    #[test]
    fn original_return_readback_refuses_foreign_owner_digest_and_bytes() {
        let selected = Arc::new(());
        let original = Arc::clone(&selected);
        let foreign = Arc::new(());
        let digest = [7; 32];
        let exact = b"original signed registration";
        assert!(require_original_return_publication(
            &original, &selected, digest, digest, exact, exact,
        )
        .is_ok());
        for result in [
            require_original_return_publication(&original, &foreign, digest, digest, exact, exact),
            require_original_return_publication(
                &original, &selected, digest, [8; 32], exact, exact,
            ),
            require_original_return_publication(
                &original,
                &selected,
                digest,
                digest,
                exact,
                b"substituted signed registration",
            ),
        ] {
            assert!(matches!(result, Err(AuraError::PermissionDenied { .. })));
        }
    }
}

/// Persist public proof only after the original approved four-domain signer
/// has completed. No broker publication occurs at this boundary.
async fn retain_original_issuer_return_registration(
    approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    signature: Vec<u8>,
) -> Result<RetainedOriginalIssuerReturnPublication, aura_core::AuraError> {
    use aura_core::effects::secure::ImmutableSecureStoreOutcome;
    use aura_core::effects::{
        CryptoExtendedEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };
    use aura_signature::SecurityTranscript;
    let effects = approval.effects().as_ref();
    let manifest = approval.manifest();
    let statement = approval.issuer_return_mailbox();
    let policy = manifest
        .final_inventory()
        .map_err(|source| {
            aura_core::AuraError::crypto_with_source(
                "read original issuer return policy",
                Arc::new(source),
            )
        })?
        .get(usize::from(statement.issuer_policy_index()))
        .ok_or_else(|| {
            aura_core::AuraError::permission_denied("original issuer root policy absent")
        })?;
    if policy.signing_node != aura_core::tree::NodeIndex(0)
        || policy.mode != aura_core::effects::crypto::SigningMode::Threshold
        || policy.threshold < 2
        || policy.epoch != manifest.final_epoch
        || aura_core::hash::hash(&policy.public_key_package)
            != statement.issuer_public_key_package_digest()
    {
        return Err(aura_core::AuraError::permission_denied(
            "issuer return policy differs from original root",
        ));
    }
    let bytes = statement.required_transcript_bytes().map_err(|source| {
        aura_core::AuraError::crypto_with_source(
            "encode approved issuer return transcript",
            Arc::new(source),
        )
    })?;
    if !effects
        .frost_verify(
            &bytes,
            &signature,
            &manifest.initiator_confirmation_verifier,
        )
        .await?
    {
        return Err(aura_core::AuraError::permission_denied(
            "original issuer return aggregate is invalid",
        ));
    }
    // The approved transcript round requires one commitment and one share
    // from every retained policy participant before it can aggregate. The
    // registration metadata names precisely that mandatory full roster.
    let signer_count = u16::try_from(policy.participants.len()).map_err(|source| {
        aura_core::AuraError::crypto_with_source("encode original return roster", Arc::new(source))
    })?;
    let registration = aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration {
        statement: statement.clone(),
        mode: aura_core::effects::crypto::SigningMode::Threshold,
        proof: aura_core::threshold::ThresholdSignature::new(
            signature,
            signer_count,
            (1..=signer_count).collect(),
            policy.public_key_package.clone(),
            policy.epoch,
        ),
    };
    let encoded = aura_core::util::serialization::to_vec(&registration)?;
    let location = SecureStorageLocation::new(
        "issuer_return_mailbox_registration_v1",
        format!(
            "{}:{}:{}",
            manifest.subject,
            manifest.invitation,
            hex::encode(statement.manifest_digest())
        ),
    );
    match effects
        .secure_store_immutable(
            &location,
            &encoded,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
                SecureStorageCapability::Delete,
            ],
        )
        .await?
    {
        ImmutableSecureStoreOutcome::Created => {}
        ImmutableSecureStoreOutcome::AlreadyExists => {
            let original = effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await?;
            if original != encoded {
                return Err(aura_core::AuraError::permission_denied(
                    "original issuer return registration was already issued differently",
                ));
            }
        }
    };
    Ok(RetainedOriginalIssuerReturnPublication {
        selected: Arc::clone(approval.effects()),
        manifest_digest: statement.manifest_digest(),
        encoded,
    })
}

/// Publish only the exact proof retained by the original issuance owner.
/// An ambiguous broker failure leaves the immutable registration and secret
/// in protected storage for the same operation; no new route is generated.
async fn publish_original_issuer_return_mailbox(
    effects: &Arc<crate::runtime::AuraEffectSystem>,
    issued: &crate::handlers::invitation_service::IssuedEnrollmentManifestBinding,
    credential: &str,
    retained: Option<RetainedOriginalIssuerReturnPublication>,
) -> Result<(), aura_core::AuraError> {
    use aura_core::effects::{
        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };
    issued.require_effects(effects.as_ref()).map_err(|source| {
        aura_core::AuraError::crypto_with_source(
            "issuer return manifest belongs to another runtime",
            Arc::new(source),
        )
    })?;
    if let Some(original) = retained.as_ref() {
        if !Arc::ptr_eq(&original.selected, effects) || original.manifest_digest != issued.digest()
        {
            return Err(aura_core::AuraError::permission_denied(
                "issuer return publication differs from original selected owner",
            ));
        }
    }
    let Some(manager) = effects.rendezvous_manager() else {
        return Ok(());
    };
    if !manager.config().bootstrap_broker.enabled {
        return Ok(());
    }
    let manifest = issued.manifest();
    let location = SecureStorageLocation::new(
        "issuer_return_mailbox_registration_v1",
        format!(
            "{}:{}:{}",
            manifest.subject,
            manifest.invitation,
            hex::encode(issued.digest())
        ),
    );
    let bytes = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await?;
    if let Some(original) = retained.as_ref() {
        require_original_return_publication(
            &original.selected,
            effects,
            original.manifest_digest,
            issued.digest(),
            &original.encoded,
            &bytes,
        )?;
    }
    let registration: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
        aura_core::util::serialization::from_slice(&bytes)?;
    let statement = &registration.statement;
    if statement.invitation() != &manifest.invitation
        || statement.manifest_digest() != issued.digest()
        || statement.setup_digest() != manifest.setup.digest
        || statement.subject() != manifest.subject
        || statement.provisional() != manifest.invitee_authority
        || statement.issuer_device() != manifest.initiator_device
        || statement.invitee_device() != manifest.invitee_device
        || statement.expires_at_ms() != manifest.expires_at_ms
        || statement.retrieval_credential_commitment()
            != aura_core::hash::hash(credential.as_bytes())
    {
        return Err(aura_core::AuraError::permission_denied(
            "retained issuer return registration differs from original manifest",
        ));
    }
    manager
        .register_original_issuer_return_mailbox(
            crate::runtime::services::bootstrap_broker::BootstrapIssuerReturnMailboxRegistration {
                registration,
                retrieval_credential: credential.to_owned(),
            },
        )
        .await
        .map_err(|source| {
            aura_core::AuraError::crypto_with_source(
                "publish original issuer return mailbox",
                Arc::new(source),
            )
        })
}

async fn retain_original_issuer_return_setup(
    effects: &crate::runtime::AuraEffectSystem,
    issued: &crate::handlers::invitation_service::IssuedEnrollmentManifestBinding,
    setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
) -> Result<(), aura_core::AuraError> {
    use aura_core::effects::{
        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };
    issued.require_effects(effects).map_err(|source| {
        aura_core::AuraError::crypto_with_source(
            "foreign issued enrollment manifest",
            Arc::new(source),
        )
    })?;
    let manifest = issued.manifest();
    if setup.digest() != manifest.setup.digest
        || setup.statement().authority != manifest.invitee_authority
        || setup.statement().device != manifest.invitee_device
    {
        return Err(aura_core::AuraError::permission_denied(
            "selected return setup differs from original issued manifest",
        ));
    }
    let code = setup.transfer_code().map_err(|source| {
        aura_core::AuraError::crypto_with_source("encode selected return setup", Arc::new(source))
    })?;
    let location = SecureStorageLocation::with_sub_key(
        "issuer_return_mailbox_setup_v1",
        manifest.subject.to_string(),
        format!("{}:{}", manifest.invitation, hex::encode(issued.digest())),
    );
    effects
        .secure_store_immutable(
            &location,
            code.as_bytes(),
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
    let original = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await?;
    if original != code.as_bytes() {
        return Err(aura_core::AuraError::permission_denied(
            "retained return setup differs from original selection",
        ));
    }
    Ok(())
}

/// Seal the exact original return records before exposing the broker route.
/// The inventory is immutable evidence for a later profile-owned retirement
/// decision; it grants no deletion capability to this runtime caller.
async fn retain_original_issuer_return_retirement_root(
    effects: &crate::runtime::AuraEffectSystem,
    issued: &crate::handlers::invitation_service::IssuedEnrollmentManifestBinding,
    credential: &str,
    setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
) -> Result<(), aura_core::AuraError> {
    use aura_core::effects::{
        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };
    issued.require_effects(effects).map_err(|source| {
        aura_core::AuraError::crypto_with_source(
            "foreign issued enrollment retirement inventory",
            Arc::new(source),
        )
    })?;
    let manifest = issued.manifest();
    let registration_location = SecureStorageLocation::new(
        "issuer_return_mailbox_registration_v1",
        format!(
            "{}:{}:{}",
            manifest.subject,
            manifest.invitation,
            hex::encode(issued.digest())
        ),
    );
    let registration_bytes = effects
        .secure_retrieve(&registration_location, &[SecureStorageCapability::Read])
        .await?;
    let registration: aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration =
        aura_core::util::serialization::from_slice(&registration_bytes)?;
    let statement = &registration.statement;
    if statement.invitation() != &manifest.invitation
        || statement.manifest_digest() != issued.digest()
        || statement.setup_digest() != manifest.setup.digest
        || statement.subject() != manifest.subject
        || statement.provisional() != manifest.invitee_authority
        || statement.issuer_device() != manifest.initiator_device
        || statement.invitee_device() != manifest.invitee_device
        || statement.expires_at_ms() != manifest.expires_at_ms
        || statement.retrieval_credential_commitment()
            != aura_core::hash::hash(credential.as_bytes())
    {
        return Err(aura_core::AuraError::permission_denied(
            "retirement inventory differs from original issued return registration",
        ));
    }
    let credential_location = SecureStorageLocation::with_sub_key(
        "issuer_return_mailbox_credential_v1",
        manifest.subject.to_string(),
        format!("{}:{}", manifest.invitation, hex::encode(issued.digest())),
    );
    let credential_bytes = effects
        .secure_retrieve(&credential_location, &[SecureStorageCapability::Read])
        .await?;
    let setup_location = SecureStorageLocation::with_sub_key(
        "issuer_return_mailbox_setup_v1",
        manifest.subject.to_string(),
        format!("{}:{}", manifest.invitation, hex::encode(issued.digest())),
    );
    let setup_bytes = effects
        .secure_retrieve(&setup_location, &[SecureStorageCapability::Read])
        .await?;
    let selected_setup = setup.transfer_code().map_err(|source| {
        aura_core::AuraError::crypto_with_source("encode selected return setup", Arc::new(source))
    })?;
    if credential_bytes != credential.as_bytes() || setup_bytes != selected_setup.as_bytes() {
        return Err(aura_core::AuraError::permission_denied(
            "retirement inventory differs from retained return secret or setup",
        ));
    }
    crate::core::agent::issuer_return_mailbox::retain_original_return_retirement_root(
        effects,
        statement,
        &registration_bytes,
        &credential_bytes,
        &setup_bytes,
    )
    .await
}

/// Move-owned continuation issued only by the original enrollment construction.
/// Observed DeviceEnrollmentStart data cannot reconstruct this capability.
pub(crate) struct OriginalIssuedEnrollmentContinuation {
    original: std::sync::Arc<
        crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
    >,
    effects: std::sync::Arc<crate::runtime::AuraEffectSystem>,
}

struct OriginalPostEnrollmentShortcut {
    issuer: std::sync::Arc<crate::AuraAgent>,
    selection: aura_app::runtime_bridge::PostEnrollmentShortcutSelection,
    delivery: OriginalShortcutDeliveryContinuation,
    dispatch_attempted: bool,
    delivered: Option<aura_core::InvitationId>,
}

/// Seal the original operation before a signed invitation can be published or
/// the broker request can become ambiguous. The key covers the original
/// enrollment, while the encrypted bytes retain its exact selected route.
async fn reserve_original_shortcut_dispatch(
    effects: &crate::runtime::AuraEffectSystem,
    grant: &crate::runtime::services::enrollment_window::EnrolledDeviceShortcutAuthority,
    selection: &aura_app::runtime_bridge::PostEnrollmentShortcutSelection,
) -> Result<(), aura_core::AuraError> {
    use aura_core::effects::secure::{
        ImmutableSecureStoreOutcome, SecureStorageCapability, SecureStorageEffects,
        SecureStorageLocation,
    };

    let location = SecureStorageLocation::with_sub_key(
        "original_shortcut_dispatch_v1",
        grant.subject().to_string(),
        grant.original_invitation().to_string(),
    );
    let purpose = match grant.purpose() {
        crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose::Contact => {
            "contact"
        }
        crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose::Guardian => {
            "guardian"
        }
    };
    let binding = format!(
        "v1\nissuer={}\noriginal={}\nenrolled_device={}\nrecipient={}\nrecipient_device={}\npurpose={}\n",
        grant.subject(),
        grant.original_invitation(),
        grant.device(),
        selection.recipient(),
        selection.device(),
        purpose,
    );
    match effects
        .secure_store_immutable(
            &location,
            binding.as_bytes(),
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?
    {
        ImmutableSecureStoreOutcome::Created => Ok(()),
        ImmutableSecureStoreOutcome::AlreadyExists => Err(aura_core::AuraError::permission_denied(
            "original shortcut dispatch was already reserved",
        )),
    }
}
#[cfg(feature = "simulation")]
pub(crate) struct OriginalSimulationFailedDisposal {
    observation: crate::runtime::services::ceremony_tracker::OriginalIssuerDeliveryObservation,
    disposal: crate::task_registry::OriginalRequiredTaskDisposalObservation,
    native: aura_core::AuraError,
    subtree: Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>,
}
#[cfg(feature = "simulation")]
impl OriginalSimulationFailedDisposal {
    pub(crate) fn native(&self) -> &aura_core::AuraError {
        &self.native
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalSimulationFailedDisposal", receiver_type = OriginalSimulationFailedDisposal, family = "runtime_helper")]
    pub(crate) fn require_shutdown_failure(
        &self,
        source: &crate::task_registry::TaskSupervisionError,
    ) -> Result<(), aura_core::AuraError> {
        self.observation.require_failed_disposal(&self.disposal)?;
        if self.disposal.matches_supervision_failure(source)
            || self.subtree.matches_supervision_failure(source)
        {
            Ok(())
        } else {
            Err(aura_core::AuraError::permission_denied(
                "shutdown failure belongs to another original task or native source",
            ))
        }
    }
}
impl OriginalIssuedEnrollmentContinuation {
    #[cfg(feature = "simulation")]
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuedEnrollmentContinuation", receiver_type = OriginalIssuedEnrollmentContinuation, family = "runtime_helper")]
    pub(crate) async fn observe_simulated_failed_disposal(
        &self,
    ) -> Result<OriginalSimulationFailedDisposal, aura_core::AuraError> {
        let (observation, disposal) = self.original.await_original_resource_disposal().await?;
        let native = observation.require_failed_disposal(&disposal)?;
        let subtree = observation.original_simulated_subtree_disposal(&disposal)?;
        Ok(OriginalSimulationFailedDisposal {
            observation,
            disposal,
            native,
            subtree,
        })
    }
    #[cfg(feature = "simulation")]
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuedEnrollmentContinuation", receiver_type = OriginalIssuedEnrollmentContinuation, family = "runtime_helper")]
    pub(crate) async fn cancel_simulated_original(
        &self,
        issuer: &crate::AuraAgent,
        pending_gate: Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    ) -> crate::core::AgentResult<
        crate::handlers::invitation_service::OriginalSimulationCleanupOutcome,
    > {
        issuer
            .invitations()?
            .cancel_simulated_original_enrollment(self.original.as_ref(), pending_gate)
            .await
    }
    #[cfg(feature = "simulation")]
    pub(crate) async fn await_simulated_disposal(
        &self,
        positive: bool,
    ) -> Result<Option<aura_core::AuraError>, aura_core::AuraError> {
        let (observation, disposal) = self.original.await_original_resource_disposal().await?;
        if positive {
            observation.require_positive(&disposal)?;
            return Ok(None);
        }
        match disposal.outcome() {
            aura_core::effects::task::RequiredTaskExit::Failed(source) => Ok(Some(source)),
            _ => Err(aura_core::AuraError::permission_denied(
                "negative simulation requires actual failed original resource disposal",
            )),
        }
    }
    #[cfg(test)]
    pub(crate) fn require_original_rejected_resources_for_test(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), aura_core::AuraError> {
        self.original
            .require_original_rejected_resources_for_test(issued)
    }

    #[cfg(test)]
    pub(crate) fn observe_original_running_owner_for_test(
        &self,
        effects: &crate::runtime::AuraEffectSystem,
        tracker: &crate::runtime::services::ceremony_tracker::CeremonyTracker,
    ) -> Result<
        crate::runtime::services::ceremony_tracker::OriginalIssuerDeliveryObservation,
        aura_core::AuraError,
    > {
        self.original
            .observe_original_running_owner_for_test(effects, tracker)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn install_progress_error_gate_for_test(
        &self,
    ) -> Result<
        crate::runtime::services::issuer_terminal_destination::OriginalProgressErrorObservationGate,
        aura_core::AuraError,
    > {
        self.original.install_progress_error_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn install_cancelled_notice_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        aura_core::AuraError,
    > {
        self.original.install_cancelled_notice_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn install_negative_cleanup_first_observation_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        aura_core::AuraError,
    > {
        self.original
            .install_negative_cleanup_first_observation_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn observe_negative_cleanup_clock_for_test(
        &self,
        now: &aura_core::time::PhysicalTime,
    ) -> aura_core::TimeoutBudgetResult<std::time::Duration> {
        self.original.observe_negative_cleanup_clock_for_test(now)
    }
    #[cfg(test)]
    pub(crate) fn install_negative_cleanup_after_observation_gate_for_test(
        &self,
    ) -> Result<
        crate::handlers::invitation_service::OriginalNegativeCleanupObservationGate,
        aura_core::AuraError,
    > {
        self.original
            .install_negative_cleanup_after_observation_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn install_committed_confirmation_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        aura_core::AuraError,
    > {
        self.original.install_committed_confirmation_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn request_original_issuer_cancel_for_test(
        &self,
    ) -> Result<(), aura_core::AuraError> {
        self.original.request_original_issuer_cancel_for_test()
    }
    #[cfg(test)]
    pub(crate) fn require_original_issuer_disposal_for_test(
        &self,
        actual: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), aura_core::AuraError> {
        self.original
            .require_original_issuer_disposal_for_test(actual)
    }
    #[cfg(test)]
    pub(crate) fn has_original_positive_receipt_for_test(&self) -> bool {
        self.original.has_original_positive_receipt_for_test()
    }

    #[cfg(test)]
    pub(crate) fn install_original_receipt_test_hook(
        &self,
        hook: crate::handlers::invitation_service::OriginalTerminalReceiptTestHook,
    ) -> Result<(), aura_core::AuraError> {
        self.original.install_original_receipt_test_hook(hook)
    }
    #[cfg(test)]
    pub(crate) fn require_original_issuer_subtree_disposal_for_test(
        &self,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), aura_core::AuraError> {
        self.original
            .require_original_issuer_subtree_disposal_for_test(disposal)
    }
    #[cfg(test)]
    pub(crate) fn original_cancellation_registration_for_test(
        &self,
    ) -> Result<
        Arc<crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRegistration>,
        aura_core::AuraError,
    > {
        self.original.original_cancellation_registration_for_test()
    }
    #[cfg(test)]
    pub(crate) async fn original_cancellation_disposal_for_test(
        &self,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, aura_core::AuraError>
    {
        self.original
            .original_cancellation_disposal_for_test()
            .await
    }
    #[cfg(test)]
    pub(crate) fn original_finalizer_failure_for_test(
        &self,
    ) -> Result<Option<aura_core::AuraError>, aura_core::AuraError> {
        self.original.original_finalizer_failure_for_test()
    }
    #[cfg(test)]
    pub(crate) fn original_disposal_for_test(
        &self,
    ) -> Result<
        crate::runtime::services::ceremony_tracker::OriginalIssuerDeliveryObservation,
        aura_core::AuraError,
    > {
        self.original.observe_original_delivery()
    }
    #[cfg(test)]
    pub(crate) fn observes_retained_retirement_acknowledgment(
        &self,
    ) -> Result<bool, aura_core::AuraError> {
        self.original.observes_retained_retirement_acknowledgment()
    }
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) async fn observe_original_prepared_interruption(
        &self,
        receiver: tokio::sync::oneshot::Receiver<
            crate::handlers::device_epoch_rotation::OriginalPreparedActivationInterruption,
        >,
    ) -> crate::core::AgentResult<
        crate::handlers::device_epoch_rotation::OriginalPreparedActivationInterruption,
    > {
        self.original
            .observe_original_prepared_interruption(receiver)
            .await
    }
    #[cfg(test)]
    pub(crate) async fn observe_original_finalizer(&self) -> crate::core::AgentResult<()> {
        self.original.observe_original_finalizer().await
    }
    /// Transfers this original capability into one shortcut operation with the
    /// caller's already selected budget. The endpoint is never renewed on retry.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuedEnrollmentContinuation", receiver_type = OriginalIssuedEnrollmentContinuation, family = "runtime_helper")]
    pub(crate) fn begin_shortcut_delivery(
        self,
        original_budget: aura_core::TimeoutBudget,
        purpose: crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose,
    ) -> OriginalShortcutDeliveryContinuation {
        OriginalShortcutDeliveryContinuation {
            issued: self,
            original_budget,
            purpose,
            observation: None,
            disposal: None,
            authority: None,
            first_failure: None,
        }
    }
}
/// One original shortcut operation; cancellation of a borrowed wait leaves
/// original enrollment evidence, disposal observation and any produced grant here.
pub(crate) struct OriginalShortcutDeliveryContinuation {
    issued: OriginalIssuedEnrollmentContinuation,
    original_budget: aura_core::TimeoutBudget,
    purpose: crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose,
    observation:
        Option<crate::runtime::services::ceremony_tracker::OriginalIssuerDeliveryObservation>,
    disposal: Option<crate::task_registry::OriginalRequiredTaskDisposalObservation>,
    authority: Option<crate::runtime::services::enrollment_window::EnrolledDeviceShortcutAuthority>,
    first_failure: Option<aura_core::AuraError>,
}
impl OriginalShortcutDeliveryContinuation {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalShortcutDeliveryContinuation", receiver_type = OriginalShortcutDeliveryContinuation, family = "runtime_helper")]
    pub(crate) async fn observe_disposal(&mut self) -> Result<(), aura_core::AuraError> {
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        let original = &self.issued.original;
        let observation = &mut self.observation;
        let disposal = &mut self.disposal;
        let result = aura_core::execute_with_timeout_budget(
            self.issued.effects.as_ref(),
            &self.original_budget,
            || async {
                if observation.is_none() {
                    *observation = Some(original.observe_original_delivery()?);
                }
                if disposal.is_none() {
                    // Store before Ready so same-poll timeout cannot consume
                    // the only result into a discarded asynchronous frame.
                    *disposal = Some(
                        observation
                            .as_ref()
                            .expect("original observer")
                            .await_disposal()
                            .await,
                    );
                }
                Ok::<(), aura_core::AuraError>(())
            },
        )
        .await;
        match result {
            Ok(()) => Ok(()),
            Err(aura_core::TimeoutRunError::Operation(source)) => Err(source),
            Err(aura_core::TimeoutRunError::Timeout(source)) => {
                let source = aura_core::AuraError::from(source);
                self.first_failure.get_or_insert_with(|| source.clone());
                Err(source)
            }
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalShortcutDeliveryContinuation", receiver_type = OriginalShortcutDeliveryContinuation, family = "runtime_helper")]
    pub(crate) async fn acknowledge_positive_authority(
        &mut self,
    ) -> Result<(), aura_core::AuraError> {
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        if self.authority.is_some() {
            return Ok(());
        }
        let observation = self.observation.as_ref().ok_or_else(|| {
            aura_core::AuraError::permission_denied("original delivery has not observed its issuer")
        })?;
        let disposal = self.disposal.as_ref().ok_or_else(|| {
            aura_core::AuraError::permission_denied("original delivery has no task disposal proof")
        })?;
        let authority = &mut self.authority;
        let result = aura_core::time::timeout::acknowledge_with_timeout_budget(
            self.issued.effects.as_ref(),
            &self.original_budget,
            || async {
                // Actual domain checkpoint: exact original task disposal and
                // retained positive terminal ACK. No diagnostic/no-op ACK.
                observation
                    .require_positive(disposal)
                    .map_err(aura_core::TimeoutBudgetError::checkpoint_failure)
            },
            |()| {
                // The canonical helper has completed its final selected clock
                // check. No await occurs between consuming the exact ACK and
                // retaining the grant in this same original owner.
                *authority = Some(observation.consume_positive(disposal, self.purpose)?);
                Ok::<(), aura_core::AuraError>(())
            },
        )
        .await;
        match result {
            Ok(result) => result,
            Err(source @ aura_core::TimeoutBudgetError::CheckpointFailure { .. }) => {
                // Actual entry-lock contention may be retried by this SAME
                // retained owner. Immutable negative/domain proof still refuses.
                Err(aura_core::AuraError::from(source))
            }
            Err(source) => {
                let source = aura_core::AuraError::from(source);
                self.first_failure.get_or_insert_with(|| source.clone());
                Err(source)
            }
        }
    }
}

impl AgentRuntimeBridge {
    pub(crate) async fn issue_original_device_enrollment_with_shortcut(
        &self,
        nickname_suggestion: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        selection: aura_app::runtime_bridge::PostEnrollmentShortcutSelection,
    ) -> Result<
        aura_app::runtime_bridge::IssuedDeviceEnrollmentStart,
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    > {
        if selection.recipient() == self.agent.authority_id() {
            return Err(aura_invitation::enrollment_setup::EnrollmentIssuanceError::InvalidPolicy);
        }
        let (start, original) = self
            .issue_original_device_enrollment_owned(nickname_suggestion, setup)
            .await?;
        let budget = original
            .original
            .post_enrollment_budget()
            .ok_or(aura_invitation::enrollment_setup::EnrollmentIssuanceError::InvalidPolicy)?;
        let purpose = match selection.purpose() {
            aura_app::runtime_bridge::PostEnrollmentShortcutPurpose::Contact => {
                crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose::Contact
            }
            aura_app::runtime_bridge::PostEnrollmentShortcutPurpose::Guardian => {
                crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose::Guardian
            }
        };
        Ok(aura_app::runtime_bridge::IssuedDeviceEnrollmentStart {
            start,
            post_completion:
                aura_app::runtime_bridge::OriginalEnrollmentPostCompletion::from_runtime(
                    OriginalPostEnrollmentShortcut {
                        issuer: self.agent.clone(),
                        selection,
                        delivery: original.begin_shortcut_delivery(budget, purpose),
                        dispatch_attempted: false,
                        delivered: None,
                    },
                ),
        })
    }

    pub(crate) async fn deliver_original_post_enrollment_shortcut(
        &self,
        token: &mut aura_app::runtime_bridge::OriginalEnrollmentPostCompletion,
    ) -> Result<aura_core::InvitationId, aura_core::AuraError> {
        let owned = token
            .runtime_mut::<OriginalPostEnrollmentShortcut>()
            .ok_or_else(|| {
                aura_core::AuraError::permission_denied("foreign shortcut continuation")
            })?;
        if !std::sync::Arc::ptr_eq(&owned.issuer, &self.agent) {
            return Err(aura_core::AuraError::permission_denied(
                "shortcut continuation belongs to another runtime",
            ));
        }
        owned.delivery.observe_disposal().await?;
        owned.delivery.acknowledge_positive_authority().await?;
        if let Some(id) = &owned.delivered {
            return Ok(id.clone());
        }
        if owned.dispatch_attempted {
            return Err(aura_core::AuraError::invalid(
                "ambiguous prior shortcut dispatch",
            ));
        }
        let grant = owned.delivery.authority.as_ref().ok_or_else(|| {
            aura_core::AuraError::permission_denied("positive shortcut grant missing")
        })?;
        let selection = &owned.selection;
        let expected_purpose = match selection.purpose() {
            aura_app::runtime_bridge::PostEnrollmentShortcutPurpose::Contact => {
                crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose::Contact
            }
            aura_app::runtime_bridge::PostEnrollmentShortcutPurpose::Guardian => {
                crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose::Guardian
            }
        };
        if grant.purpose() != expected_purpose || selection.recipient() == grant.subject() {
            return Err(aura_core::AuraError::permission_denied(
                "shortcut grant and external selection differ",
            ));
        }
        let effects = owned.delivery.issued.effects.clone();
        owned.dispatch_attempted = true;
        let id = aura_core::execute_with_timeout_budget(
            effects.as_ref(),
            &owned.delivery.original_budget,
            || async {
                reserve_original_shortcut_dispatch(effects.as_ref(), grant, selection).await?;
                let service =
                    self.agent
                        .invitations()
                        .map_err(|source| aura_core::AuraError::Internal {
                            message: "shortcut invitation service unavailable".into(),
                            source: Some(std::sync::Arc::new(source)),
                        })?;
                let issued = service
                    .issue_signed_original_shortcut(
                        &self.agent.threshold_signing(),
                        grant,
                        selection.recipient(),
                        selection.device(),
                    )
                    .await
                    .map_err(|source| aura_core::AuraError::Internal {
                        message: "issue signed post-enrollment shortcut invitation".into(),
                        source: Some(std::sync::Arc::new(source)),
                    })?;
                let rendezvous = self.agent.runtime().rendezvous().ok_or_else(|| {
                    aura_core::AuraError::invalid("post-enrollment broker unavailable")
                })?;
                rendezvous
                    .send_post_enrollment_shortcut(grant, selection, &issued)
                    .await
                    .map_err(|source| aura_core::AuraError::Internal {
                        message: "deliver signed post-enrollment shortcut invitation".into(),
                        source: Some(std::sync::Arc::new(source)),
                    })?;
                Ok(issued.invitation_id().clone())
            },
        )
        .await
        .map_err(|source| match source {
            aura_core::TimeoutRunError::Operation(source) => source,
            aura_core::TimeoutRunError::Timeout(source) => aura_core::AuraError::from(source),
        })?;
        owned.delivered = Some(id.clone());
        Ok(id)
    }

    pub(crate) async fn issue_original_device_enrollment(
        &self,
        nickname_suggestion: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<
        aura_app::runtime_bridge::DeviceEnrollmentStart,
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    > {
        self.issue_original_device_enrollment_owned(nickname_suggestion, setup)
            .await
            .map(|(observed, _original)| observed)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "UserTransferredEnrollmentSetup", capability_type = aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup, family = "runtime_helper")]
    pub(crate) async fn issue_original_device_enrollment_owned(
        &self,
        nickname_suggestion: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<
        (
            aura_app::runtime_bridge::DeviceEnrollmentStart,
            OriginalIssuedEnrollmentContinuation,
        ),
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    > {
        self.agent
            .threshold_signing()
            .issue_original_owned_issuer(self.agent.clone(), nickname_suggestion, setup)
            .await
            .map_err(|source| {
                aura_invitation::enrollment_setup::EnrollmentIssuanceError::at(
                    aura_invitation::enrollment_setup::EnrollmentIssuanceStage::InvitationExport,
                    source,
                )
            })
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "UserTransferredEnrollmentSetup", capability_type = aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup, family = "runtime_helper")]
    pub(crate) async fn execute_original_device_enrollment(
        &self,
        nickname_suggestion: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        preparation: super::enrollment_quorum::OriginalIssuerPreparation,
    ) -> Result<
        (
            aura_app::runtime_bridge::DeviceEnrollmentStart,
            OriginalIssuedEnrollmentContinuation,
        ),
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    > {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        use aura_core::hash::hash;
        use aura_core::threshold::{
            policy_for, CeremonyFlow, KeyGenerationPolicy, ParticipantIdentity,
        };
        use aura_invitation::enrollment_setup::{
            EnrollmentIssuanceError as IssueError, EnrollmentIssuanceStage as Stage,
        };

        let prepared_registration = preparation.registration.clone();
        let requires_user_approval = preparation.requires_user_approval();
        let mut retained_resources = preparation.retained.clone().lock_owned().await;

        let authority_id = self.agent.authority_id();
        let effects = self.agent.runtime().effects();
        let current_device_id = self.agent.context().device_id();
        let statement = setup.statement();
        let now_ms = effects
            .physical_time()
            .await
            .map_err(IssueError::Time)?
            .ts_ms;
        if now_ms < statement.issued_at_ms || now_ms >= statement.expires_at_ms {
            return Err(IssueError::OutsideValidity);
        }
        let invitee_authority_id = statement.authority;
        let new_device_id = statement.device;
        if invitee_authority_id == authority_id || new_device_id == current_device_id {
            return Err(IssueError::CurrentIdentity);
        }

        #[cfg(test)]
        eprintln!("enrollment initiation stage: prepare authenticated rotation");
        let issuance_tracker = self.agent.ceremony_tracker().await;
        let plan = effects
            .prepare_owned_authenticated_enrollment_rotation(&setup, &issuance_tracker)
            .await
            .map_err(|source| IssueError::at(Stage::TreeRead, source))?;
        let participants = plan.participants().to_vec();
        let participant_device_ids: Vec<_> = participants
            .iter()
            .map(|participant| match participant {
                ParticipantIdentity::Device(id) => Ok(*id),
                _ => Err(IssueError::InvalidPolicy),
            })
            .collect::<Result<_, _>>()?;
        let other_device_ids: Vec<_> = participant_device_ids
            .iter()
            .copied()
            .filter(|id| *id != current_device_id && *id != new_device_id)
            .collect();
        let total_n = u16::try_from(participants.len())
            .map_err(|source| IssueError::at(Stage::TreeRead, source))?;
        let threshold_k = plan.threshold();
        let policy = policy_for(CeremonyFlow::DeviceEnrollment);
        if policy.keygen != KeyGenerationPolicy::K2DealerBased {
            return Err(IssueError::InvalidPolicy);
        }
        let issuance_now = effects
            .physical_time()
            .await
            .map_err(IssueError::Time)?
            .ts_ms;
        if issuance_now < statement.issued_at_ms || issuance_now >= statement.expires_at_ms {
            return Err(IssueError::OutsideValidity);
        }

        let invitation_service = self
            .agent
            .invitations()
            .map_err(|e| IssueError::at(Stage::InvitationService, e))?;

        // The owner reserves identity before constructing signed admission or
        // manifest payloads. The complete invitation is then committed once.
        #[cfg(test)]
        eprintln!("enrollment initiation stage: reserve issued invitation");
        let reserved = invitation_service
            .reserve_device_enrollment_invitation()
            .await
            .map_err(|e| IssueError::at(Stage::InvitationCreation, e))?;
        tracing::debug!(invitation_id = %reserved.invitation_id(), created_at_ms = reserved.created_at_ms(),
            "Reserved complete enrollment issuance identity before fact preparation");
        // Bind ceremony to exact prestate/setup before generating pending keys.
        let prestate_hash = plan.prestate();

        let op_input = serde_json::to_vec(&(
            new_device_id,
            setup.digest(),
            threshold_k,
            total_n,
            current_device_id,
        ))
        .map_err(|e| IssueError::at(Stage::OperationEncoding, e))?;
        let op_hash = aura_core::Hash32(hash(&op_input));

        let nonce_bytes = effects.random_bytes(8).await;
        let nonce_bytes: [u8; 8] = nonce_bytes
            .try_into()
            .map_err(|_| IssueError::InvalidPolicy)?;
        let nonce = u64::from_le_bytes(nonce_bytes);
        let mut ceremony_seed = Vec::with_capacity(32 + 32 + 8);
        ceremony_seed.extend_from_slice(prestate_hash.as_bytes());
        ceremony_seed.extend_from_slice(op_hash.as_bytes());
        ceremony_seed.extend_from_slice(&nonce.to_le_bytes());
        let ceremony_hash = aura_core::Hash32(hash(&ceremony_seed));
        let ceremony_id = aura_core::types::identifiers::CeremonyId::new(format!(
            "ceremony:{}",
            hex::encode(ceremony_hash.as_bytes())
        ));

        #[cfg(test)]
        eprintln!("enrollment initiation stage: prepare pinned generation");
        retained_resources.allocation = Some(
            crate::runtime::effects::OriginalEnrollmentAllocationProducer::from_original(plan),
        );
        let super::enrollment_quorum::OriginalPreparedIssuerResources {
            launch,
            allocation,
            reservation,
            ..
        } = &mut *retained_resources;
        let selected = launch.as_ref().ok_or(IssueError::InvalidPolicy)?;
        let producer = allocation.as_mut().ok_or(IssueError::InvalidPolicy)?;
        producer
            .bind_original_launch(effects.as_ref(), selected)
            .map_err(|source| IssueError::at(Stage::Rotation, source))?;
        let (pending_epoch, key_packages, _public_key) = selected
            .execute_original_allocation(&effects, || async {
                #[cfg(all(test, not(target_arch = "wasm32")))]
                effects
                    .await_original_allocation_prewrite_for_test()
                    .await?;
                let (epoch, packages, public, original) = effects
                    .prepare_pinned_enrollment_rotation(&setup, &reserved, &ceremony_id, producer)
                    .await?;
                *reservation = Some(original);
                Ok((epoch, packages, public))
            })
            .await
            .map_err(|source| IssueError::at(Stage::Rotation, source))?;
        retained_resources.allocation = None;
        let super::enrollment_quorum::OriginalPreparedIssuerResources {
            reservation,
            issuer_reservation,
            issuer_inventory,
            issuer_signing,
            root,
            registered,
            registered_custody,
            launch,
            ..
        } = &mut *retained_resources;
        let missing_original = || {
            IssueError::at(
                Stage::CeremonyRegistration,
                aura_core::AuraError::invalid("original issuance resource custody is missing"),
            )
        };
        let pending_epoch = Epoch::new(pending_epoch);

        let pubkey_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority_id),
            format!("{}", pending_epoch.value()),
        );
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            format!("{}", authority_id),
            format!("{}", pending_epoch.value()),
        );

        let public_key_package = effects
            .secure_retrieve(&pubkey_location, &[SecureStorageCapability::Read])
            .await
            .map_err(|error| IssueError::at(Stage::PendingPackageRead, error))?;
        if public_key_package.is_empty() {
            return Err(IssueError::EmptyPendingPackage);
        }
        let threshold_config = effects
            .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
            .await
            .map_err(|error| IssueError::at(Stage::PendingConfigRead, error))?;
        if threshold_config.is_empty() {
            return Err(IssueError::EmptyPendingConfig);
        }
        let mut key_package_by_device: std::collections::HashMap<aura_core::DeviceId, Vec<u8>> =
            std::collections::HashMap::new();
        for (device_id, key_package) in participant_device_ids
            .iter()
            .copied()
            .zip(key_packages.iter())
        {
            key_package_by_device.insert(device_id, key_package.clone());
        }

        let Some(invited_key_package) = key_package_by_device.get(&new_device_id).cloned() else {
            return Err(IssueError::MissingPackage(new_device_id));
        };

        // Register ceremony (acceptance required from all non-initiator devices).
        let acceptor_device_ids: Vec<aura_core::DeviceId> = other_device_ids
            .iter()
            .copied()
            .chain(std::iter::once(new_device_id))
            .collect();
        let acceptors: Vec<ParticipantIdentity> = acceptor_device_ids
            .iter()
            .copied()
            .map(ParticipantIdentity::device)
            .collect();
        let response_policy = reservation
            .as_ref()
            .ok_or_else(&missing_original)?
            .response_policy()
            .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
        let acceptance_n = response_policy.total();
        let acceptance_threshold = response_policy.required();
        if usize::from(acceptance_n) != acceptors.len() {
            return Err(IssueError::at(Stage::CeremonyRegistration, crate::runtime::effects::held_registration_error(crate::runtime::effects::HeldEnrollmentRegistrationError::ResponsePolicyMismatch)));
        }

        #[cfg(test)]
        eprintln!("enrollment initiation stage: acquire ceremony runner");
        let runner = self.agent.ceremony_runner().await;
        let nickname_for_tracker = if nickname_suggestion.is_empty() {
            None
        } else {
            Some(nickname_suggestion.clone())
        };
        let now_ms = effects
            .physical_time()
            .await
            .map_err(IssueError::Time)?
            .ts_ms;
        for old_id in runner
            .check_supersession_candidates(
                aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
                &prestate_hash,
            )
            .await
        {
            runner
                .supersede_owned_device_enrollment(
                    reservation.as_ref().ok_or_else(&missing_original)?,
                    &old_id,
                    &ceremony_id,
                    SupersessionReason::NewerRequest,
                    now_ms,
                )
                .await
                .map_err(|e| IssueError::at(Stage::Supersession, e))?;
        }
        #[cfg(test)]
        eprintln!("enrollment initiation stage: capture retained generation");
        let generation = self
            .agent
            .threshold_signing()
            .capture_retained_pending_generation(&authority_id, pending_epoch.value())
            .await
            .map_err(|error| IssueError::at(Stage::SetupVerifierRetention, error))?;
        #[cfg(test)]
        eprintln!("enrollment initiation stage: retain original generation");
        crate::handlers::invitation::enrollment_trust::retain_pending_signing_generation(
            effects.as_ref(),
            &ceremony_id,
            &generation,
            prestate_hash,
        )
        .await
        .map_err(|error| IssueError::at(Stage::SetupVerifierRetention, error))?;
        crate::handlers::invitation::enrollment_trust::retain_user_transferred_verifier(
            effects.as_ref(),
            authority_id,
            &ceremony_id,
            pending_epoch.value(),
            current_device_id,
            &setup,
        )
        .await
        .map_err(|error| IssueError::at(Stage::SetupVerifierRetention, error))?;
        #[cfg(test)]
        eprintln!("enrollment initiation stage: register held generation");
        runner
            .start_owned_device_enrollment(
                reservation.as_mut().ok_or_else(&missing_original)?,
                launch.as_mut().ok_or_else(&missing_original)?,
                CeremonyInitRequest {
                    ceremony_id: ceremony_id.clone(),
                    kind: aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
                    initiator_id: authority_id,
                    threshold_k: acceptance_threshold,
                    total_n: acceptance_n,
                    participants: acceptors,
                    new_epoch: pending_epoch.value(),
                    enrollment_device_id: Some(new_device_id),
                    enrollment_nickname_suggestion: nickname_for_tracker,
                    prestate_hash,
                },
                &self
                    .agent
                    .runtime()
                    .tasks()
                    .group("original-enrollment-issuance"),
            )
            .await
            .map_err(|e| IssueError::at(Stage::CeremonyRegistration, e))?;

        *root = Some(
            issuance_tracker
                .held_issuer_window(
                    &effects,
                    reservation.as_ref().ok_or_else(&missing_original)?,
                )
                .await
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?,
        );
        let original_window = root.as_ref().ok_or_else(&missing_original)?;
        prepared_registration
            .get()
            .ok_or_else(&missing_original)?
            .publish_original_window(
                &effects,
                ceremony_id.clone(),
                original_window
                    .original_issuer_observer(effects.as_ref())
                    .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?,
            )
            .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;

        let (enrollment_code, manifest_transfer) = original_window
            .child()
            .execute(effects.as_ref(), || async {

        #[cfg(test)]
        eprintln!("enrollment initiation stage: export original baseline");
        let baseline_tree_ops = effects
            .export_tree_ops()
            .await
            .map_err(|e| IssueError::at(Stage::BaselineExport, e))?
            .into_iter()
            .map(|op| {
                aura_core::util::serialization::to_vec(&op)
                    .map_err(|e| IssueError::at(Stage::BaselineEncoding, e))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let baseline_ops: Vec<aura_core::AttestedOp> = baseline_tree_ops
            .iter()
            .map(|b| {
                aura_core::util::serialization::from_slice(b)
                    .map_err(|e| IssueError::at(Stage::BaselineEncoding, e))
            })
            .collect::<Result<_, _>>()?;
        let parents = effects
            .collect_enrollment_parent_inventory(&baseline_ops)
            .await
            .map_err(|e| IssueError::at(Stage::BaselineExport, e))?;
        if issuer_reservation.is_none() {
            *issuer_reservation = Some(Arc::new(reservation.take().ok_or_else(&missing_original)?));
        }
        let actual = issuer_reservation.as_ref().ok_or_else(&missing_original)?;
        *issuer_inventory = Some(Arc::new(effects
            .capture_owned_enrollment_final_inventory(Arc::clone(actual), &baseline_ops)
            .await
            .map_err(|e| IssueError::at(Stage::BaselineExport, e))?));
        let final_inventory = issuer_inventory.as_ref().ok_or_else(&missing_original)?;
        let final_state = aura_journal::commitment_tree::reduce(&baseline_ops)
            .map_err(|e| IssueError::at(Stage::BaselineExport, e))?;
        let identity_context = if !requires_user_approval {
            Some(
                crate::handlers::rendezvous_identity::require_active_identity_signing_context(
                    effects.as_ref(),
                    &authority_id,
                )
                .await
                .map_err(|e| IssueError::at(Stage::InvitationExport, e))?,
            )
        } else {
            None
        };
        let confirmation_key = if let Some(identity_context) = &identity_context {
            crate::handlers::rendezvous_identity::require_identity_keys(identity_context)
                .await
                .map_err(|e| IssueError::at(Stage::InvitationExport, e))?
                .1
                .to_vec()
        } else {
            let root = final_inventory
                .inventory()
                .iter()
                .find(|entry| entry.signing_node == aura_core::tree::NodeIndex(0))
                .ok_or(IssueError::InvalidPolicy)?;
            if root.mode != aura_core::effects::crypto::SigningMode::Threshold || root.threshold < 2
            {
                return Err(IssueError::InvalidPolicy);
            }
            frost_ed25519::keys::PublicKeyPackage::deserialize(&root.public_key_package)
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?
                .verifying_key()
                .serialize()
                .to_vec()
        };
        aura_invitation::enrollment_manifest::EnrollmentTrustManifest::validate_pending_policy(
            &threshold_config,
        )
        .map_err(|e| IssueError::at(Stage::PendingConfigRead, e))?;
        let manifest = aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
            version: aura_invitation::enrollment_manifest::EnrollmentTrustManifest::CURRENT_VERSION,
            subject: authority_id,
            initiator_device: current_device_id,
            invitee_authority: invitee_authority_id,
            invitee_device: new_device_id,
            setup: aura_invitation::enrollment_setup::DeviceEnrollmentSetupBinding {
                nonce: setup.statement().nonce,
                digest: setup.digest(),
            },
            invitation: reserved.invitation_id().clone(),
            ceremony: ceremony_id.clone(),
            expires_at_ms: setup.statement().expires_at_ms,
            baseline_digest: aura_core::hash::hash(
                &aura_core::util::serialization::to_vec(&baseline_tree_ops)
                    .map_err(|e| IssueError::at(Stage::BaselineEncoding, e))?,
            ),
            baseline_count: baseline_tree_ops
                .len()
                .try_into()
                .map_err(|e| IssueError::at(Stage::BaselineEncoding, e))?,
            starting_epoch: 0,
            starting_commitment: [0; 32],
            parents,
            final_inventory: final_inventory.inventory().to_vec(),
            final_epoch: final_state.epoch.value(),
            final_commitment: final_state.root_commitment,
            pending_epoch: pending_epoch.value(),
            pending_share_digest: aura_core::hash::hash(&invited_key_package),
            pending_public_key_package_digest: aura_core::hash::hash(&public_key_package),
            pending_threshold_config_digest: aura_core::Hash32::from_bytes(&threshold_config),
            initiator_confirmation_verifier: confirmation_key.clone(),
        };
        #[cfg(test)]
        eprintln!("enrollment initiation stage: export original manifest");
        let issuer_return_credential = reserve_original_issuer_return_credential(
            effects.as_ref(), &manifest,
        ).await.map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
        #[cfg(test)]
        eprintln!("enrollment initiation stage: original return credential reserved");
        let return_commitment = aura_core::hash::hash(issuer_return_credential.as_bytes());
        let mut quorum_transport = None;
        let (manifest_transfer, issued_manifest, retained_issuer_return) = if requires_user_approval {
            let (preview, public_intent, frozen_metadata) = invitation_service
                .preview_owned_enrollment_transport(
                    &reserved,
                    final_inventory,
                    &manifest,
                    aura_invitation::InvitationType::DeviceEnrollment {
                        setup_binding: manifest.setup.clone(),
                        subject_authority: authority_id,
                        invitee_authority: invitee_authority_id,
                        initiator_device_id: current_device_id,
                        device_id: new_device_id,
                        nickname_suggestion: Some(nickname_suggestion.clone()),
                        ceremony_id: ceremony_id.clone(),
                        pending_epoch: pending_epoch.value(),
                        key_package: invited_key_package.clone(),
                        threshold_config: threshold_config.clone(),
                        public_key_package: public_key_package.clone(),
                        baseline_tree_ops: baseline_tree_ops.clone(),
                    },
                )
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            #[cfg(test)]
            eprintln!("enrollment initiation stage: original transport preview ready");
            let window = original_window.child();
            let approval = Arc::new(preparation
                .require_original_user_approval(
                    effects.clone(),
                    &manifest,
                    &public_intent,
                    return_commitment,
                    &setup,
                    window,
                )
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?);
            #[cfg(test)]
            eprintln!("enrollment initiation stage: original ready published and approval received");
            let [manifest_signature, transport_signature, request_signature, return_signature] = self
                .agent
                .threshold_signing()
                .sign_original_approved_enrollment_domains(Arc::clone(&approval), Arc::clone(final_inventory), window, issuer_signing)
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            let return_statement = approval.issuer_return_mailbox();
            if return_statement.retrieval_credential_commitment() != return_commitment {
                return Err(IssueError::InvalidPolicy);
            }
            let retained =
                retain_original_issuer_return_registration(approval.as_ref(), return_signature)
                    .await
                    .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            quorum_transport = Some((preview, public_intent, frozen_metadata, transport_signature));
            let exported = invitation_service
                .export_quorum_enrollment_manifest(
                    &reserved,
                    &setup,
                    final_inventory,
                    manifest,
                    manifest_signature,
                )
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            invitation_service
                .retain_quorum_initial_request(
                    &exported.1,
                    final_inventory,
                    &approval,
                    request_signature,
                )
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            (exported.0, exported.1, Some(retained))
        } else {
            let exported = invitation_service
                .export_owned_enrollment_manifest(
                    &reserved,
                    &setup,
                    identity_context.as_ref().ok_or(IssueError::InvalidPolicy)?,
                    final_inventory,
                    manifest,
                    return_commitment,
                )
                .await
                .map_err(|e| IssueError::at(Stage::InvitationExport, e))?;
            (exported.0, exported.1, None)
        };
        #[cfg(test)]
        eprintln!("enrollment initiation stage: retain original issued manifest");
        crate::handlers::invitation::enrollment_trust::retain_issued_enrollment_manifest(
            effects.as_ref(),
            &issued_manifest,
        )
        .await
        .map_err(|e| IssueError::at(Stage::SetupVerifierRetention, e))?;
        retain_original_issuer_return_setup(effects.as_ref(), &issued_manifest, &setup)
            .await
            .map_err(|source| IssueError::at(Stage::SetupVerifierRetention, source))?;
        retain_original_issuer_return_retirement_root(
            effects.as_ref(),
            &issued_manifest,
            &issuer_return_credential,
            &setup,
        )
        .await
        .map_err(|source| IssueError::at(Stage::SetupVerifierRetention, source))?;
        publish_original_issuer_return_mailbox(
            &effects,
            &issued_manifest,
            &issuer_return_credential,
            retained_issuer_return,
        )
        .await
        .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
        #[cfg(test)]
        eprintln!("enrollment initiation stage: create original enrollment invitation");
        let invitation = invitation_service
            .invite_device_enrollment(
                reserved,
                invitee_authority_id,
                authority_id,
                current_device_id,
                new_device_id,
                Some(nickname_suggestion),
                ceremony_id.clone(),
                pending_epoch.value(),
                invited_key_package,
                threshold_config.clone(),
                public_key_package.clone(),
                baseline_tree_ops,
                aura_invitation::enrollment_setup::DeviceEnrollmentSetupBinding {
                    nonce: setup.statement().nonce,
                    digest: setup.digest(),
                },
                None,
            )
            .await
            .map_err(|e| IssueError::at(Stage::InvitationCreation, e))?;

        let enrollment_code =
            if let Some((preview, approved, metadata, signature)) = quorum_transport {
                if aura_core::util::serialization::to_vec(&preview)
                    .map_err(|source| IssueError::at(Stage::InvitationExport, source))?
                    != aura_core::util::serialization::to_vec(&invitation)
                        .map_err(|source| IssueError::at(Stage::InvitationExport, source))?
                {
                    return Err(IssueError::InvalidPolicy);
                }
                invitation_service
                    .export_quorum_enrollment_invitation(
                        &invitation,
                        &issued_manifest,
                        &approved,
                        &metadata,
                        signature,
                    )
                    .await
                    .map_err(|source| IssueError::at(Stage::InvitationExport, source))?
            } else {
                invitation_service
                    .export_owned_enrollment_invitation(
                        &invitation,
                        identity_context.as_ref().ok_or(IssueError::InvalidPolicy)?,
                        &issued_manifest,
                    )
                    .await
                    .map_err(|source| IssueError::at(Stage::InvitationExport, source))?
            };
        if requires_user_approval {
            issuer_signing.as_ref().ok_or_else(&missing_original)?.try_lock()
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, aura_core::AuraError::Internal { message: "original signing state is borrowed".into(), source: Some(Arc::new(source)) }))?.require_initial_transfer()
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
        }
        *issuer_inventory = None;
        let actual = issuer_reservation.take().ok_or_else(&missing_original)?;
        match Arc::try_unwrap(actual) {
            Ok(actual) => *reservation = Some(actual),
            Err(actual) => { *issuer_reservation = Some(actual); return Err(missing_original()); }
        }
        let registration = self
            .agent
            .runtime()
            .ceremony_tracker()
            .get(&ceremony_id)
            .await
            .map_err(|error| IssueError::at(Stage::CeremonyRegistration, error))?;
        crate::handlers::invitation::enrollment_trust::persist_pending_enrollment_registration(
            effects.as_ref(),
            reservation.as_ref().ok_or_else(&missing_original)?,
            &registration,
            &invitation,
        )
        .await
        .map_err(|error| IssueError::at(Stage::SetupVerifierRetention, error))?;
        crate::runtime::effects::EnrollmentGenerationReservation::complete_registration_retained(reservation, registered)
            .await
            .map_err(|e| IssueError::at(Stage::CeremonyRegistration, e))?;

        Ok::<_, IssueError>((enrollment_code, manifest_transfer))
            })
            .await
            .map_err(|source| match source {
                aura_core::time::timeout::TimeoutRunError::Operation(source) => source,
                aura_core::time::timeout::TimeoutRunError::Timeout(source) =>
                    IssueError::at(Stage::CeremonyRegistration, aura_core::AuraError::from(source)),
            })?;
        // Preserve the same original execution authority only after all required
        // issuance and registration writes finish under its selected window.
        let original_issued = OriginalIssuedEnrollmentContinuation {
            original: original_window.origin_observation(),
            effects: effects.clone(),
        };
        {
            let registration = &prepared_registration;
            let mut handoff = invitation_service
                .prepare_registered_device_enrollment_custody(
                    registered.as_ref().ok_or_else(&missing_original)?,
                    self.original_enrollment_finalizer_service(),
                    root,
                    registered_custody,
                )
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
            registered_custody
                .as_mut()
                .ok_or_else(&missing_original)?
                .retain_registration_custody(registered)
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
            if let Some(signing) = issuer_signing.as_ref() {
                registered_custody
                    .as_mut()
                    .ok_or_else(&missing_original)?
                    .retain_conditional_signing(Arc::clone(signing))
                    .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
            }
            registration
                .get()
                .ok_or_else(&missing_original)?
                .publish_registered_running(&mut handoff.running)
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
            handoff
                .publication_sender
                .send(())
                .map_err(|_| missing_original())?;
            registered_custody
                .as_mut()
                .ok_or_else(&missing_original)?
                .initialize_original()
                .await
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
            handoff
                .admission_observer
                .observe_registered_admission(effects.as_ref(), handoff.initialized_receiver)
                .await
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
            *registered = None;
        };
        // Launch device-scoped rotation sessions for existing devices so they can
        // stage and commit the new epoch through one protocol path.
        if !other_device_ids.is_empty() {
            for device_id in &other_device_ids {
                let Some(key_package) = key_package_by_device.get(device_id).cloned() else {
                    return Err(IssueError::MissingPackage(*device_id));
                };
                self.spawn_device_epoch_rotation(
                    crate::handlers::device_epoch_rotation::DeviceEpochRotationInitRequest {
                        ceremony_id: ceremony_id.clone(),
                        origin: registered_custody
                            .as_ref()
                            .ok_or_else(&missing_original)?
                            .original_enrollment_rotation_origin()
                            .map_err(|source| {
                                IssueError::at(Stage::CeremonyRegistration, source)
                            })?,
                        pending_epoch: pending_epoch.value(),
                        participant_device_id: *device_id,
                        key_package,
                        threshold_config: threshold_config.clone(),
                        public_key_package: public_key_package.clone(),
                    },
                )
                .map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?;
            }
        }

        tracing::info!(
            authority = %authority_id,
            websocket_addrs = ?effects
                .lan_transport()
                .map(|transport| transport.websocket_addrs().to_vec())
                .unwrap_or_default(),
            "device enrollment export transport state"
        );

        // Use compile-time safe export since we already have the invitation
        Ok((
            aura_app::runtime_bridge::DeviceEnrollmentStart {
                ceremony_id: ceremony_id.clone(),
                enrollment_code,
                pending_epoch,
                device_id: new_device_id,
                manifest_transfer: Some(manifest_transfer),
            },
            original_issued,
        ))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod original_shortcut_delivery_tests {
    use crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose;
    use aura_app::runtime_bridge::RuntimeBridge;
    use aura_core::effects::task::{RequiredTaskCustody, RequiredTaskExit, RequiredTaskSettlement};
    use aura_core::effects::StorageCoreEffects;
    use aura_guards::GuardContextProvider;
    use futures::future::BoxFuture;

    struct ForeignCompletedCustody;
    impl RequiredTaskCustody for ForeignCompletedCustody {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), aura_core::AuraError>> {
            Box::pin(async { Ok(()) })
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, aura_core::AuraError>> {
            Box::pin(async { Ok(RequiredTaskSettlement::Resolved) })
        }
    }

    #[test]
    fn original_shortcut_delivery_retains_issued_origin_across_cancelled_wait_and_rejects_foreign_disposal(
    ) {
        run_original_shortcut_delivery_test(false, EnrolledDeviceShortcutPurpose::Contact);
    }

    #[test]
    fn original_shortcut_delivery_issues_recipient_bound_quorum_code() {
        run_original_shortcut_delivery_test(true, EnrolledDeviceShortcutPurpose::Contact);
    }

    #[test]
    fn original_shortcut_delivery_issues_external_guardian_quorum_code() {
        run_original_shortcut_delivery_test(true, EnrolledDeviceShortcutPurpose::Guardian);
    }

    fn run_original_shortcut_delivery_test(
        include_positive: bool,
        purpose: EnrolledDeviceShortcutPurpose,
    ) {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async move {
            let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let transport = crate::SharedTransport::new();
            let ((issuer, invitee, invitation, start, _, _), original) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation_and_sync(
                    "original-shortcut-delivery-positive", Some(std::sync::Arc::new(clock.clone())), transport.clone(),
                ).await;
            let selected_time = aura_core::effects::PhysicalTimeEffects::physical_time(&clock)
                .await
                .expect("selected manual physical time");
            let budget = aura_core::TimeoutBudget::from_start_and_timeout(
                &selected_time,
                std::time::Duration::from_millis(100),
            )
            .expect("shortcut delivery timeout budget");
            let mut continuation = original.begin_shortcut_delivery(budget, purpose);
            {
                let observation = continuation.observe_disposal();
                futures::pin_mut!(observation);
                assert!(
                    futures::poll!(observation.as_mut()).is_pending(),
                    "unaccepted original issuer has no disposal proof"
                );
            }
            assert!(continuation.disposal.is_none());
            assert!(continuation.authority.is_none());
            assert_eq!(
                clock.now_ms(),
                5_000,
                "observation cannot drive selected manual time"
            );
            let invitee_bridge = std::sync::Arc::new(
                crate::runtime_bridge::AgentRuntimeBridge::new(invitee.clone()),
            );
            invitee_bridge
                .initialize_account("Joiner")
                .await
                .expect("joined device account setup");
            let pending = invitee_bridge
                .try_list_pending_invitations()
                .await
                .expect("joined device pending invitations")
                .into_iter()
                .find(|item| item.invitation_id == invitation.invitation_id)
                .expect("imported enrollment invitation");
            let invitee_app = std::sync::Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    invitee_bridge.clone(),
                )
                .expect("joined app core"),
            ));
            aura_app::ui::workflows::invitation::accept_device_enrollment_invitation(
                &invitee_app,
                &pending,
            )
            .await
            .expect("actual signed app acceptance and activation");
            continuation
                .observe_disposal()
                .await
                .expect("same original task disposed after real signed terminal ACK");
            let foreign_group = issuer.runtime().tasks().group("foreign-shortcut-disposal");
            let foreign = foreign_group
                .spawn_original_required_custody(
                    "foreign-shortcut-disposal".into(),
                    Box::new(ForeignCompletedCustody),
                    None,
                )
                .unwrap_or_else(|_| panic!("actual foreign custody admission"));
            let foreign_observer = foreign.disposal_observer();
            let foreign_proof = foreign_observer.await_disposal().await;
            assert!(matches!(
                foreign_proof.outcome(),
                RequiredTaskExit::Completed
            ));
            let observation = continuation
                .observation
                .as_ref()
                .expect("original continuation has a disposal observation");
            assert!(
                observation
                    .consume_positive(&foreign_proof, purpose)
                    .is_err(),
                "completed foreign task cannot consume actual positive ACK"
            );
            continuation
                .acknowledge_positive_authority()
                .await
                .expect("same issued handle and actual positive receipt");
            let authority = continuation
                .authority
                .as_ref()
                .expect("positive acknowledgment grants original shortcut authority");
            assert_eq!(authority.subject(), issuer.authority_id());
            assert_eq!(authority.device(), start.device_id);
            assert_eq!(authority.original_invitation(), &invitation.invitation_id);
            assert_eq!(authority.purpose(), purpose);
            let external =
                aura_app::runtime_bridge::PostEnrollmentShortcutSelection::select_external(
                    issuer.authority_id(),
                    aura_core::AuthorityId::new_from_entropy([91; 32]),
                    aura_core::DeviceId::from_bytes([92; 32]),
                    aura_app::runtime_bridge::PostEnrollmentShortcutPurpose::Guardian,
                )
                .expect("external guardian selection");
            let contact =
                aura_app::runtime_bridge::PostEnrollmentShortcutSelection::select_external(
                    issuer.authority_id(),
                    aura_core::AuthorityId::new_from_entropy([91; 32]),
                    aura_core::DeviceId::from_bytes([92; 32]),
                    aura_app::runtime_bridge::PostEnrollmentShortcutPurpose::Contact,
                )
                .expect("external contact selection");
            let (selection, mismatch) = if purpose == EnrolledDeviceShortcutPurpose::Contact {
                (&contact, &external)
            } else {
                (&external, &contact)
            };
            let projected: serde_json::Value = serde_json::from_slice(
                &invitee
                    .runtime()
                    .effects()
                    .retrieve("account.json")
                    .await
                    .expect("read adopted account config")
                    .expect("joined device has adopted account config"),
            )
            .expect("adopted account config json");
            let subject: aura_core::AuthorityId =
                serde_json::from_value(projected["authority_id"].clone())
                    .expect("adopted account authority");
            assert_eq!(subject, issuer.authority_id());
            let adopted_context: aura_core::ContextId =
                serde_json::from_value(projected["context_id"].clone())
                    .expect("adopted account context");
            let config = invitee.runtime().effects().config().clone();
            invitee
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
                .expect("joined device tasks stop before profile reopen");
            drop((invitee_app, invitee_bridge, pending));
            drop(invitee);
            let profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
                .expect("exclusive joined profile after teardown");
            let context = aura_core::context::EffectContext::new(
                subject,
                adopted_context,
                aura_core::effects::ExecutionMode::Testing,
            );
            let joined_runtime =
                crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
                    .with_authority(subject)
                    .with_config(config)
                    .with_shared_transport(transport.clone())
                    .build(&context)
                    .await
                    .expect("joined device reopens under adopted account");
            let joined = std::sync::Arc::new(crate::AuraAgent::new(joined_runtime, subject));
            crate::runtime_bridge::AgentRuntimeBridge::new(joined.clone())
                .bootstrap_signing_keys(
                    crate::runtime::services::RuntimeServiceContext::test_original(
                        joined.runtime().tasks(),
                        joined.runtime().effects(),
                    )
                    .await
                    .startup_window(),
                )
                .await
                .expect("joined device restores its native account share");
            let service = issuer
                .invitations()
                .expect("issuer invitation service after account enrollment");
            let co_signer = joined.threshold_signing();
            use aura_core::effects::ThresholdSigningEffects;
            let issuer_state = issuer
                .threshold_signing()
                .threshold_state(&issuer.authority_id())
                .await
                .expect("issuer retains native threshold state");
            let joined_state = co_signer
                .threshold_state(&issuer.authority_id())
                .await
                .expect("joined device retains native threshold state");
            assert_eq!((issuer_state.threshold, joined_state.threshold), (2, 2));
            assert_eq!(issuer_state.epoch, joined_state.epoch);
            assert_eq!(
                issuer
                    .threshold_signing()
                    .public_key_package(&issuer.authority_id())
                    .await,
                co_signer.public_key_package(&issuer.authority_id()).await,
            );
            assert_eq!(
                transport.authority_for_device(joined.context().device_id()),
                Some(issuer.authority_id()),
            );
            assert_eq!(
                co_signer
                    .device_signing_consent()
                    .await
                    .expect("joined device signing consent"),
                aura_app::runtime_bridge::DeviceSigningConsent::EscalateToUser,
            );
            let mut before_invitations: Vec<_> = service
                .list_with_storage()
                .await
                .into_iter()
                .map(|invitation| invitation.invitation_id)
                .collect();
            before_invitations.sort();
            let before_storage = issuer
                .runtime()
                .effects()
                .list_keys(None)
                .await
                .expect("issuer storage keys before shortcut refusal");
            let before_facts = issuer
                .runtime()
                .effects()
                .load_committed_facts(issuer.authority_id())
                .await
                .expect("committed invitation/contact facts before refusal");
            let threshold_signing = issuer.threshold_signing();
            let refusal = async {
                loop {
                    co_signer
                        .process_device_quorum_inbound()
                        .await
                        .expect("joined device admits native quorum request");
                    if let Some(request) = co_signer.pending_signing_requests().await.first() {
                        co_signer
                            .decide_pending_signing_request(&request.id, false)
                            .await
                            .expect("joined device refuses shortcut signing");
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            };
            let denied = tokio::time::timeout(std::time::Duration::from_secs(20), async {
                let issuing = service.issue_signed_original_shortcut(
                    &threshold_signing,
                    authority,
                    selection.recipient(),
                    selection.device(),
                );
                tokio::pin!(issuing);
                tokio::pin!(refusal);
                tokio::select! {
                    issued = &mut issuing => issued,
                    () = &mut refusal => issuing.await,
                }
            })
            .await
            .expect("quorum refusal reaches a terminal");
            assert!(
                denied.is_err(),
                "refused quorum cannot issue a shortcut code"
            );
            let invitation_keys = |keys: Vec<String>| -> Vec<String> {
                keys.into_iter()
                    .filter(|key| key.starts_with("invitation/"))
                    .collect()
            };
            assert_eq!(
                invitation_keys(
                    issuer
                        .runtime()
                        .effects()
                        .list_keys(None)
                        .await
                        .expect("issuer storage keys after shortcut refusal")
                ),
                invitation_keys(before_storage),
                "refused signature cannot persist an invitation"
            );
            assert_eq!(
                issuer
                    .runtime()
                    .effects()
                    .load_committed_facts(issuer.authority_id())
                    .await
                    .expect("committed facts after refusal"),
                before_facts,
                "refused signature cannot commit invitation or contact facts"
            );
            let mut after_invitations: Vec<_> = service
                .list_with_storage()
                .await
                .into_iter()
                .map(|invitation| invitation.invitation_id)
                .collect();
            after_invitations.sort();
            assert_eq!(after_invitations, before_invitations);
            if !include_positive {
                return;
            }
            co_signer
                .set_device_signing_consent(
                    aura_app::runtime_bridge::DeviceSigningConsent::AutoSignVerified,
                )
                .await
                .expect("joined device consent for verified quorum request");
            super::reserve_original_shortcut_dispatch(
                issuer.runtime().effects().as_ref(),
                authority,
                selection,
            )
            .await
            .expect("original signed operation atomically reserves one dispatch");
            let issued = {
                let issuing = service.issue_signed_original_shortcut(
                    &threshold_signing,
                    authority,
                    selection.recipient(),
                    selection.device(),
                );
                tokio::pin!(issuing);
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    loop {
                        tokio::select! {
                            issued = &mut issuing => break issued,
                            inbound = co_signer.process_device_quorum_inbound() => {
                                inbound.expect("joined device processes native quorum rounds");
                                tokio::task::yield_now().await;
                            }
                        }
                    }
                })
                .await
                .expect("verified quorum signing reaches a terminal")
                .expect("actual positive grant can issue an externally addressed quorum code")
            };
            let signed_id = issued.invitation_id().clone();
            let code = issued.code().to_owned();
            let parsed = aura_invitation::shareable::ShareableInvitation::from_code(&code)
                .expect("canonical quorum shortcut code");
            assert_eq!(parsed.invitation_id, signed_id);
            assert_eq!(parsed.sender_id, authority.subject());
            assert_eq!(
                parsed.version,
                aura_invitation::shareable::ShareableInvitation::CONTACT_GUARDIAN_QUORUM_VERSION
            );
            assert_eq!(
                aura_invitation::shareable::ShareableInvitation::quorum_recipient_from_code(&code)
                    .expect("recipient binding"),
                Some(selection.recipient())
            );
            assert_eq!(
                aura_invitation::shareable::ShareableInvitation::recipient_device_from_code(&code)
                    .expect("device binding"),
                Some(selection.device())
            );
            let recipient_config = crate::core::AgentConfig {
                device_id: selection.device(),
                storage: crate::core::config::StorageConfig {
                    base_path: tempfile::Builder::new()
                        .prefix("aura-external-shortcut-recipient-")
                        .tempdir()
                        .expect("external recipient profile")
                        .keep(),
                    ..Default::default()
                },
                ..Default::default()
            };
            let recipient_context = aura_core::context::EffectContext::new(
                selection.recipient(),
                aura_core::ContextId::new_from_entropy([93; 32]),
                aura_core::effects::ExecutionMode::Testing,
            );
            let recipient_profile =
                crate::runtime::builder::TestingOwnedProfileCapability::acquire(&recipient_config)
                    .expect("external recipient owns its profile");
            let recipient_runtime =
                crate::runtime::EffectSystemBuilder::testing_with_owned_profile(recipient_profile)
                    .with_authority(selection.recipient())
                    .with_config(recipient_config)
                    .with_shared_transport(transport.clone())
                    .with_physical_time_provider(std::sync::Arc::new(clock.clone()))
                    .with_sync()
                    .build(&recipient_context)
                    .await
                    .expect("real external recipient runtime");
            let recipient = std::sync::Arc::new(crate::AuraAgent::new(
                recipient_runtime,
                selection.recipient(),
            ));
            let recipient_bridge =
                crate::runtime_bridge::AgentRuntimeBridge::new(recipient.clone());
            recipient_bridge
                .bootstrap_signing_keys(
                    crate::runtime::services::RuntimeServiceContext::test_original(
                        recipient.runtime().tasks(),
                        recipient.runtime().effects(),
                    )
                    .await
                    .startup_window(),
                )
                .await
                .expect("external recipient has its own signing identity");
            recipient_bridge
                .initialize_account("External recipient")
                .await
                .expect("external recipient account");
            let auth_token = "task326-loopback-broker-auth";
            let retrieval_token = "task326-external-recipient-one-time-retrieval";
            let broker_service = crate::runtime::services::bootstrap_broker::LocalBootstrapBrokerService::bind(
                "127.0.0.1:0",
                std::time::Duration::from_secs(60),
                auth_token,
                crate::runtime::services::bootstrap_broker::BootstrapBrokerLanBindPolicy::LoopbackOnly,
            )
            .await
            .expect("authenticated loopback broker binds");
            broker_service.start(&issuer.runtime().tasks().group("shortcut-external-broker"));
            tokio::task::yield_now().await;
            let broker_config =
                crate::runtime::services::bootstrap_broker::BootstrapBrokerConfig::default()
                    .enabled(true)
                    .with_base_url(broker_service.public_url())
                    .with_auth_token(auth_token)
                    .with_invitation_retrieval_token(retrieval_token);
            let recipient_broker =
                crate::runtime::services::RendezvousManager::new_with_default_udp(
                    selection.recipient(),
                    crate::runtime::services::RendezvousManagerConfig::default()
                        .with_bootstrap_broker(broker_config.clone()),
                    std::sync::Arc::new(recipient.runtime().effects().time_effects().clone()),
                );
            recipient_broker
                .register_bootstrap_candidate(
                    selection.device(),
                    format!("shared-transport://{}", selection.device()),
                    None,
                )
                .await
                .expect("external recipient registers one-time broker retrieval");
            let broker = crate::runtime::services::RendezvousManager::new_with_default_udp(
                issuer.authority_id(),
                crate::runtime::services::RendezvousManagerConfig::default()
                    .with_bootstrap_broker(broker_config),
                std::sync::Arc::new(issuer.runtime().effects().time_effects().clone()),
            );
            assert!(
                broker
                    .send_post_enrollment_shortcut(authority, mismatch, &issued)
                    .await
                    .is_err(),
                "original grant cannot dispatch a different shortcut purpose"
            );
            broker
                .send_post_enrollment_shortcut(authority, selection, &issued)
                .await
                .expect("typed shortcut reaches authenticated broker");
            assert!(
                super::reserve_original_shortcut_dispatch(
                    issuer.runtime().effects().as_ref(),
                    authority,
                    selection,
                )
                .await
                .is_err(),
                "unknown broker response cannot reserve a second original dispatch"
            );
            let wrong_token_broker =
                crate::runtime::services::RendezvousManager::new_with_default_udp(
                    selection.recipient(),
                    crate::runtime::services::RendezvousManagerConfig::default()
                        .with_bootstrap_broker(
                        crate::runtime::services::bootstrap_broker::BootstrapBrokerConfig::default(
                        )
                        .enabled(true)
                        .with_base_url(broker_service.public_url())
                        .with_auth_token(auth_token)
                        .with_invitation_retrieval_token("wrong-one-time-token"),
                    ),
                    std::sync::Arc::new(recipient.runtime().effects().time_effects().clone()),
                );
            assert!(
                wrong_token_broker
                    .take_bootstrap_broker_invitations(selection.device())
                    .await
                    .is_err(),
                "foreign retrieval token cannot consume external delivery"
            );
            assert!(
                recipient_broker
                    .take_bootstrap_broker_invitations(aura_core::DeviceId::from_bytes([94; 32]))
                    .await
                    .is_err(),
                "same credential cannot drain a foreign recipient device"
            );
            let delivered = recipient_broker
                .take_bootstrap_broker_invitations(selection.device())
                .await
                .expect("external recipient drains one-time broker delivery");
            assert_eq!(delivered.len(), 1, "one original shortcut delivery");
            assert_eq!(delivered[0].invitation_code, code);
            assert!(
                recipient_broker
                    .take_bootstrap_broker_invitations(selection.device())
                    .await
                    .is_err(),
                "one-time retrieval cannot replay the delivered code"
            );
            let imported = recipient
                .invitations()
                .expect("external recipient invitation owner")
                .import_and_cache(&delivered[0].invitation_code)
                .await
                .expect("external recipient verifies and imports signed shortcut");
            assert_eq!(imported.invitation_id, signed_id);
            crate::handlers::invitation::tests::assert_self_certified_quorum_import(
                recipient.as_ref(),
                &signed_id,
            )
            .await;
            let recipient_invitations = recipient
                .invitations()
                .expect("external recipient invitation owner");
            if purpose == EnrolledDeviceShortcutPurpose::Contact {
                let issuer_handler = crate::handlers::invitation::InvitationHandler::new(
                    crate::core::AuthorityContext::new_with_device(
                        issuer.authority_id(),
                        issuer.runtime().device_id(),
                    ),
                )
                .expect("issuer owns contact acceptance inbox")
                .with_threshold_signing(issuer.threshold_signing());
                let accept = recipient_invitations.accept(&signed_id);
                tokio::pin!(accept);
                let respond = async {
                    let mut saw_acceptance = false;
                    loop {
                        let processed = issuer_handler
                            .process_contact_invitation_acceptances(issuer.runtime().effects())
                            .await
                            .expect("issuer processes actual contact acceptance");
                        saw_acceptance |= processed > 0;
                        if saw_acceptance {
                            let deadline = clock.wait_for_registered_deadline().await;
                            assert!(
                                deadline.value() <= 5_200,
                                "recipient confirmation uses only its first two selected-provider polls"
                            );
                            clock.advance(deadline.value() - clock.now_ms());
                        }
                        tokio::task::yield_now().await;
                    }
                };
                let cosign = async {
                    loop {
                        co_signer
                            .process_device_quorum_inbound()
                            .await
                            .expect("joined device co-signs the real contact confirmation");
                        tokio::task::yield_now().await;
                    }
                };
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    tokio::select! {
                        result = &mut accept => result,
                        () = respond => unreachable!("issuer response driver remains active"),
                        () = cosign => unreachable!("joined quorum response driver remains active"),
                    }
                })
                .await
                .expect("external contact confirmation reaches terminal")
                .expect("external recipient accepts real signed shortcut");
            } else {
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    recipient_invitations.accept(&signed_id),
                )
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "guardian principal did not reach recipient terminal; active={:?} failures={:?}",
                        issuer.runtime().tasks().active_tasks(),
                        issuer.runtime().tasks().task_failures()
                    )
                });
                result.expect("external recipient accepts real signed shortcut");
            }
            assert_eq!(
                parsed.invitation_type.purpose(),
                if purpose == EnrolledDeviceShortcutPurpose::Contact {
                    aura_core::invitation::InvitationPurpose::Contact
                } else {
                    aura_core::invitation::InvitationPurpose::Guardian
                }
            );
            assert!(
                continuation
                    .observation
                    .as_ref()
                    .expect("original continuation retains disposal observation")
                    .consume_positive(
                        continuation
                            .disposal
                            .as_ref()
                            .expect("original continuation retains disposal proof"),
                        if purpose == EnrolledDeviceShortcutPurpose::Contact {
                            EnrolledDeviceShortcutPurpose::Guardian
                        } else {
                            EnrolledDeviceShortcutPurpose::Contact
                        },
                    )
                    .is_err(),
                "original acknowledged capability is consumed exactly once"
            );
            assert!(
                clock.now_ms() <= 5_200,
                "recipient confirmation advances only observed polls"
            );
            if purpose == EnrolledDeviceShortcutPurpose::Contact {
                use aura_core::effects::secure::{
                    ImmutableSecureStoreOutcome, SecureStorageCapability, SecureStorageEffects,
                    SecureStorageLocation,
                };
                let marker = SecureStorageLocation::with_sub_key(
                    "original_shortcut_dispatch_v1",
                    issuer.authority_id().to_string(),
                    invitation.invitation_id.to_string(),
                );
                let expected_binding = format!(
                    "v1\nissuer={}\noriginal={}\nenrolled_device={}\nrecipient={}\nrecipient_device={}\npurpose=contact\n",
                    authority.subject(),
                    authority.original_invitation(),
                    authority.device(),
                    selection.recipient(),
                    selection.device(),
                );
                let config = issuer.runtime().effects().config().clone();
                let context_id = issuer.context().default_context_id();
                let mut before: Vec<_> = issuer
                    .runtime()
                    .effects()
                    .list_keys(None)
                    .await
                    .expect("issuer invitation keys before profile reopen")
                    .into_iter()
                    .filter(|key| key.starts_with("invitation/"))
                    .collect();
                before.sort();
                issuer
                    .runtime()
                    .tasks()
                    .shutdown_gracefully(std::time::Duration::from_secs(5))
                    .await
                    .expect("issuer task root stops after acknowledged broker delivery");
                drop((service, threshold_signing, continuation, issuer));
                let profile =
                    crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
                        .expect("original issuer profile reopens after process disposal");
                let timer = clock
                    .observe_next_relative_sleep_of(1_000)
                    .await
                    .expect("observe selected startup timer admission");
                let context = aura_core::context::EffectContext::new(
                    subject,
                    context_id,
                    aura_core::effects::ExecutionMode::Testing,
                );
                let build =
                    crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
                        .with_authority(subject)
                        .with_config(config)
                        .with_physical_time_provider(std::sync::Arc::new(clock.clone()))
                        .build(&context);
                let advance = async {
                    let endpoint = timer.await.expect("actual startup timer endpoint");
                    clock.set_time(endpoint.value());
                };
                let (reopened, ()) = tokio::join!(build, advance);
                let reopened = reopened.expect("same issuer profile after broker reply is lost");
                let retained = reopened
                    .effects()
                    .secure_retrieve(&marker, &[SecureStorageCapability::Read])
                    .await
                    .expect("immutable original dispatch marker survives reopen");
                assert_eq!(retained, expected_binding.as_bytes());
                assert_eq!(
                    reopened
                        .effects()
                        .secure_store_immutable(
                            &marker,
                            b"attempted replay",
                            &[
                                SecureStorageCapability::Read,
                                SecureStorageCapability::Write
                            ],
                        )
                        .await
                        .expect("replayed original marker is retained"),
                    ImmutableSecureStoreOutcome::AlreadyExists,
                );
                let mut after: Vec<_> = reopened
                    .effects()
                    .list_keys(None)
                    .await
                    .expect("issuer invitation keys after profile reopen")
                    .into_iter()
                    .filter(|key| key.starts_with("invitation/"))
                    .collect();
                after.sort();
                assert_eq!(after, before, "reopen cannot create another invitation");
            }
        });
    }

    #[test]
    fn original_shortcut_delivery_refuses_actual_cancelled_enrollment() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let ((issuer, invitee, invitation, _, _, _), original) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                    "original-shortcut-delivery-cancelled", Some(std::sync::Arc::new(clock.clone())), Some(crate::SharedTransport::new()),
                ).await;
            let selected_time = aura_core::effects::PhysicalTimeEffects::physical_time(&clock)
                .await
                .unwrap();
            let budget = aura_core::TimeoutBudget::from_start_and_timeout(
                &selected_time,
                std::time::Duration::from_millis(100),
            )
            .unwrap();
            let mut continuation =
                original.begin_shortcut_delivery(budget, EnrolledDeviceShortcutPurpose::Contact);
            eprintln!("actual cancelled delivery stage: begin original cancel");
            issuer
                .invitations()
                .unwrap()
                .cancel(&invitation.invitation_id)
                .await
                .expect("actual original protected cancellation decision");
            eprintln!(
                "actual cancelled delivery stage: original cancel completed; begin invitee accept"
            );
            let refused = invitee
                .invitations()
                .unwrap()
                .accept(&invitation.invitation_id)
                .await
                .expect_err("actual invitee consumes signed cancellation instead of activation");
            eprintln!("actual cancelled delivery stage: actual invitee refusal; begin original disposal observation");
            assert!(std::error::Error::source(&refused).is_some());
            match continuation.observe_disposal().await {
                Ok(()) => assert!(
                    continuation.acknowledge_positive_authority().await.is_err(),
                    "negative signed terminal or failed disposal cannot grant a positive shortcut"
                ),
                Err(error) => assert!(
                    std::error::Error::source(&error).is_some(),
                    "unresolved disposal retains actual native cause"
                ),
            }
            eprintln!("actual cancelled delivery stage: original disposal observation completed");
            assert!(continuation.authority.is_none());
            assert_eq!(clock.now_ms(), 5_000);
            // The app receives only this opaque original continuation. A
            // cancelled enrollment cannot turn into a shortcut through that
            // public handoff, even after its observation was interrupted.
            let selection =
                aura_app::runtime_bridge::PostEnrollmentShortcutSelection::select_external(
                    issuer.authority_id(),
                    aura_core::AuthorityId::new_from_entropy([91; 32]),
                    aura_core::DeviceId::from_bytes([92; 32]),
                    aura_app::runtime_bridge::PostEnrollmentShortcutPurpose::Contact,
                )
                .expect("actual external selection");
            let issuer_bridge = crate::runtime_bridge::AgentRuntimeBridge::new(issuer.clone());
            let mut token =
                aura_app::runtime_bridge::OriginalEnrollmentPostCompletion::from_runtime(
                    super::OriginalPostEnrollmentShortcut {
                        issuer: issuer.clone(),
                        selection,
                        delivery: continuation,
                        dispatch_attempted: false,
                        delivered: None,
                    },
                );
            assert!(
                issuer_bridge
                    .deliver_original_post_enrollment_shortcut(&mut token)
                    .await
                    .is_err(),
                "cancelled original cannot publish an external shortcut"
            );
            let retained = token
                .runtime_mut::<super::OriginalPostEnrollmentShortcut>()
                .expect("same original app continuation");
            assert!(!retained.dispatch_attempted);
            assert!(retained.delivered.is_none());
            assert!(retained.delivery.authority.is_none());

            let authority = issuer.authority_id();
            let context_id = issuer.context().default_context_id();
            let config = issuer.runtime().effects().config().clone();
            let invitation_keys = |keys: Vec<String>| -> Vec<String> {
                let mut matching: Vec<_> = keys
                    .into_iter()
                    .filter(|key| key.starts_with("invitation/"))
                    .collect();
                matching.sort();
                matching
            };
            let before = invitation_keys(issuer.runtime().effects().list_keys(None).await.unwrap());
            let shutdown = issuer
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
                .expect_err("cancelled original finalizer retains its supervised failure");
            let crate::task_registry::TaskSupervisionError::TaskFailed {
                group,
                task,
                source,
            } = shutdown
            else {
                panic!("unexpected original issuer shutdown result: {shutdown:?}");
            };
            assert_eq!(
                group,
                "runtime.original-enrollment-issuance.original-issuer-enrollment-children"
            );
            assert_eq!(task, "original_issuer_finalizer");
            let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&source);
            let mut cancelled = false;
            while let Some(current) = cause {
                if matches!(
                    current
                        .downcast_ref::<crate::handlers::invitation::EnrollmentVmAdmissionError>(),
                    Some(
                        crate::handlers::invitation::EnrollmentVmAdmissionError::TerminalFailed(
                            aura_app::runtime_bridge::CeremonyFailureReason::Cancelled
                        )
                    )
                ) {
                    cancelled = true;
                }
                cause = current.source();
            }
            assert!(
                cancelled,
                "shutdown retains actual signed cancelled terminal cause"
            );
            drop((token, issuer_bridge, issuer));
            let profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
                .expect("same physical issuer profile after original disposal");
            let timer = clock
                .observe_next_relative_sleep_of(1_000)
                .await
                .expect("observe actual selected startup timer admission");
            let context = aura_core::context::EffectContext::new(
                authority,
                context_id,
                aura_core::effects::ExecutionMode::Testing,
            );
            let build = crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
                .with_authority(authority)
                .with_config(config)
                .with_physical_time_provider(std::sync::Arc::new(clock.clone()))
                .build(&context);
            let drive_timer = async {
                let endpoint = timer.await.expect("actual startup timer endpoint");
                assert_eq!(endpoint.value(), 6_000);
                clock.set_time(endpoint.value());
            };
            let (reopened, ()) = tokio::join!(build, drive_timer);
            let reopened = reopened.expect("actual issuer profile reopen");
            let after = invitation_keys(reopened.effects().list_keys(None).await.unwrap());
            assert!(
                after.iter().all(|key| before.contains(key)),
                "cancelled app continuation cannot publish a new invitation across reopen"
            );
        });
    }
    #[test]
    fn original_shortcut_delivery_expiry_keeps_actual_proof_without_issuing_authority() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let ((issuer, invitee, invitation, _, _, _), original) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                    "original-shortcut-delivery-expiry", Some(std::sync::Arc::new(clock.clone())), Some(crate::SharedTransport::new()),
                ).await;
            let selected_time = aura_core::effects::PhysicalTimeEffects::physical_time(&clock)
                .await
                .unwrap();
            let budget = aura_core::TimeoutBudget::from_start_and_timeout(
                &selected_time,
                std::time::Duration::from_millis(100),
            )
            .unwrap();
            let mut continuation =
                original.begin_shortcut_delivery(budget, EnrolledDeviceShortcutPurpose::Contact);
            invitee
                .invitations()
                .unwrap()
                .accept(&invitation.invitation_id)
                .await
                .expect("actual signed enrollment before delivery endpoint");
            continuation
                .observe_disposal()
                .await
                .expect("actual original issuer disposal proof");
            assert!(continuation.disposal.is_some());
            clock.advance(100); // Explicitly expire only the original shortcut operation.
            let expired = continuation
                .acknowledge_positive_authority()
                .await
                .expect_err("stored proof cannot bypass original endpoint");
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&expired);
            let mut exact_deadline = false;
            while let Some(error) = source {
                exact_deadline |= matches!(
                    error.downcast_ref::<aura_core::TimeoutBudgetError>(),
                    Some(aura_core::TimeoutBudgetError::DeadlineExceeded {
                        deadline_at_ms: 5_100,
                        observed_at_ms: 5_100
                    })
                );
                source = error.source();
            }
            assert!(
                exact_deadline,
                "actual original deadline cause is preserved"
            );
            assert!(continuation.authority.is_none());
            assert!(
                continuation.disposal.is_some(),
                "refusal retains exact original proof"
            );
            assert!(
                continuation
                    .observation
                    .as_ref()
                    .unwrap()
                    .require_positive(continuation.disposal.as_ref().unwrap())
                    .is_ok(),
                "time refusal never consumes the actual positive ACK"
            );
            // Returning a test clock behind the endpoint must not resurrect
            // this original operation's latched first timeout.
            clock.set_time(5_000);
            assert!(continuation.acknowledge_positive_authority().await.is_err());
            assert!(continuation.authority.is_none());
            assert_eq!(
                issuer.authority_id(),
                continuation.issued.effects.authority_id()
            );
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod original_finalizer_observation_tests {
    use futures::FutureExt;

    #[test]
    fn original_finalizer_observation_waits_for_actual_original_commit() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let ((issuer, _invitee, _invitation, _start, _acceptance, verified), original) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                    "original-finalizer-observation-pending", None, None,
                ).await;
            let mut observation = Box::pin(original.observe_original_finalizer());
            assert!(
                observation.as_mut().now_or_never().is_none(),
                "no fabricated terminal before verified acceptance"
            );
            issuer
                .runtime()
                .ceremony_runner()
                .record_verified_enrollment_response(verified)
                .await
                .unwrap();
            observation
                .await
                .expect("same original observation sees actual finalizer commit");
        });
    }

    #[test]
    fn original_finalizer_observation_retains_terminal_after_original_task_disposal() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_core::effects::PhysicalTimeEffects;
            let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
            let ((issuer, invitee, invitation, start, _acceptance, _verified), original) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                    "original-finalizer-observation-disposed", Some(std::sync::Arc::new(clock)), None,
                ).await;
            let budget = aura_core::TimeoutBudget::from_start_and_timeout(
                &issuer.runtime().effects().physical_time().await.unwrap(),
                std::time::Duration::from_millis(30_000),
            )
            .unwrap();
            let mut continuation = original.begin_shortcut_delivery(
                budget,
                crate::runtime::services::enrollment_window::EnrolledDeviceShortcutPurpose::Contact,
            );
            invitee
                .invitations()
                .unwrap()
                .accept(&invitation.invitation_id)
                .await
                .expect("actual invitee protocol completes the original producer");
            continuation
                .observe_disposal()
                .await
                .expect("actual original registered owner is disposed");
            assert!(matches!(
                continuation.disposal.as_ref().unwrap().outcome(),
                aura_core::effects::task::RequiredTaskExit::Completed
            ));
            assert_eq!(
                issuer
                    .runtime()
                    .ceremony_runner()
                    .terminal_outcome(&start.ceremony_id)
                    .await
                    .unwrap(),
                Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed)
            );
            continuation
                .issued
                .observe_original_finalizer()
                .await
                .expect("retained terminal observation does not re-admit a disposed task");
        });
    }
}
