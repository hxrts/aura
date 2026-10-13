//! Positive existing-device enrollment work under the original peer custody.
use super::*;
use aura_core::effects::SecureStorageEffects;
use aura_signature::SecurityTranscript;
use aura_sync::protocols::device_epoch_rotation::*;

#[derive(Default)]
pub(in crate::runtime::services) struct OriginalEnrollmentPeerNativeProgress {
    material: Option<ValidatedLocalEnrollmentSigningMaterial>,
    pending_share: Option<zeroize::Zeroizing<Vec<u8>>>,
    stage_started: bool,
    staged: bool,
    acceptance: Option<DeviceEpochAcceptance>,
    tree_installed: bool,
    activation_started: bool,
    committed: bool,
    positive_terminal: Option<OriginalPositiveTerminalSigningMaterial>,
}

impl OriginalEnrollmentPeerNativeProgress {
    pub(super) fn positive_terminal(&self) -> Option<&OriginalPositiveTerminalSigningMaterial> {
        self.positive_terminal.as_ref()
    }
    pub(super) fn retire_positive_for_cleanup(
        &mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        retained: &mut Option<RetiredOriginalSigningMaterial>,
    ) -> Result<(), AuraError> {
        if let Some(actual) = &*retained {
            actual.require_original(approval)?;
            if self.positive_terminal.is_some() {
                return Err(AuraError::permission_denied(
                    "positive cleanup owner reappeared after retirement",
                ));
            }
            return Ok(());
        }
        if let Some(actual) = self.positive_terminal.take() {
            match actual.retire_original(approval, child) {
                Ok(actual) => *retained = Some(actual),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.positive_terminal = Some(actual);
                    return Err(source);
                }
            }
        }
        Ok(())
    }
    pub(super) fn dispose_original_material(&mut self) {
        self.material = None;
        self.pending_share = None;
    }
}

impl ThresholdSigningService {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalApprovedNativeAdmission",
        family = "runtime_helper"
    )]
    pub(super) async fn prepare_original_enrollment_peer_acceptance(
        &self,
        native: &mut crate::runtime::effects::OriginalApprovedNativeAdmission,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        consent: &OriginalDeviceProposalConsent,
        proposal: &DeviceEpochProposal,
        accepted_at_ms: u64,
        progress: &mut OriginalEnrollmentPeerNativeProgress,
    ) -> Result<DeviceEpochAcceptance, AuraError> {
        native.acquire_original(approval).await?;
        if progress.material.is_none() {
            progress.material = Some(
                self.admit_original_owned_enrollment_material(approval, native)
                    .await?,
            );
        }
        let material = progress
            .material
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original peer material missing"))?;
        let verifier = enrollment_transcript_wire::original_participant_verifier(
            &material.ordered_devices,
            &material.public_package,
            proposal.initiator_device_id,
        )?;
        consent.require_original_proposal(self.effects.as_ref(), proposal, &verifier)?;
        if !verify_device_epoch_proposal_hashes(proposal) {
            return Err(AuraError::permission_denied(
                "original proposal package hashes refused",
            ));
        }
        let key = aura_core::TrustedPublicKey::active(
            aura_core::TrustedKeyDomain::AuthorityThreshold,
            Some(approval.manifest().final_epoch),
            material.public_package.clone(),
            Hash32::from_bytes(&material.public_package),
        );
        if !verify_device_epoch_authority_signature(
            self.effects.as_ref(),
            proposal.subject_authority,
            DEVICE_EPOCH_PROPOSAL_SIGNING_DOMAIN,
            &DeviceEpochProposalTranscript::new(proposal),
            &proposal.authority_signature,
            &key,
            approval.manifest().final_epoch,
        )
        .await?
        {
            return Err(AuraError::permission_denied(
                "original peer proposal signature refused",
            ));
        }
        if let Some(accepted) = &progress.acceptance {
            return Ok(accepted.clone());
        }
        if progress.pending_share.is_none() {
            let actual = native.original_authenticated(approval)?;
            let state = aura_journal::commitment_tree::reduce(actual.original_baseline(approval)?)
                .map_err(|source| {
                    AuraError::crypto_with_source("reduce original peer baseline", Arc::new(source))
                })?;
            let leaf = state
                .leaves
                .values()
                .find(|leaf| leaf.device_id == material.device)
                .ok_or_else(|| AuraError::permission_denied("original peer leaf absent"))?;
            let share = tree_signing::share_from_key_package_bytes(&material.local_share)?;
            let scalar: [u8; 32] = share
                .value
                .as_slice()
                .try_into()
                .map_err(|_| AuraError::invalid("original peer agreement scalar length"))?;
            let scalar = zeroize::Zeroizing::new(scalar);
            progress.pending_share = Some(zeroize::Zeroizing::new(
                decrypt_device_epoch_key_package(
                    self.effects.as_ref(),
                    proposal,
                    &leaf.public_key,
                    &scalar,
                )
                .await?,
            ));
        }
        if !progress.staged {
            if progress.stage_started {
                return Err(AuraError::invalid(
                    "original peer staging interrupted without completion",
                ));
            }
            progress.stage_started = true;
            self.stage_rotated_device_share(
                &proposal.subject_authority,
                proposal.pending_epoch,
                progress
                    .pending_share
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("original pending share missing"))?,
            )
            .await?;
            for (namespace, bytes) in [
                ("threshold_config", &proposal.threshold_config),
                ("threshold_pubkey", &proposal.public_key_package),
            ] {
                self.effects
                    .secure_store(
                        &SecureStorageLocation::with_sub_key(
                            namespace,
                            proposal.subject_authority.to_string(),
                            proposal.pending_epoch.to_string(),
                        ),
                        bytes,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await?;
            }
            progress.staged = true;
        }
        let payload = DeviceEpochAcceptanceTranscriptPayload {
            ceremony_id: proposal.ceremony_id.clone(),
            acceptor_device_id: material.device,
            proposal_hash: device_epoch_proposal_hash(proposal)?,
            accepted_at_ms,
        };
        let message = DeviceEpochAcceptanceTranscript::from_payload(payload.clone())
            .transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source("encode original peer acceptance", Arc::new(source))
            })?;
        let signature = self
            .effects
            .sign_participant_key_proof(&message, &material.local_share, SigningMode::Threshold)
            .await?;
        let accepted = DeviceEpochAcceptance {
            ceremony_id: payload.ceremony_id,
            acceptor_device_id: payload.acceptor_device_id,
            proposal_hash: payload.proposal_hash,
            accepted_at_ms,
            signing_epoch: Some(approval.manifest().final_epoch),
            signing_mode: Some(SigningMode::Threshold),
            signing_index: Some(material.index),
            signing_package_digest: Some(Hash32::from_bytes(&material.public_package)),
            signature,
        };
        progress.acceptance = Some(accepted.clone());
        Ok(accepted)
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalApprovedNativeAdmission",
        family = "runtime_helper"
    )]
    pub(super) async fn apply_original_enrollment_peer_commit(
        &self,
        native: &mut crate::runtime::effects::OriginalApprovedNativeAdmission,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        consent: &OriginalDeviceProposalConsent,
        proposal: &DeviceEpochProposal,
        commit: &DeviceEpochCommit,
        retirement: &OriginalRetiredConditionalTransportCustody,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        progress: &mut OriginalEnrollmentPeerNativeProgress,
    ) -> Result<(), AuraError> {
        retirement
            .material_retirement()
            .require_original(approval)?;
        retirement.require_original_window(child)?;
        let positive_window = child.retain_original_conditional_identity(approval)?;
        let material = progress
            .material
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original peer acceptance material missing"))?;
        let verifier = enrollment_transcript_wire::original_participant_verifier(
            &material.ordered_devices,
            &material.public_package,
            proposal.initiator_device_id,
        )?;
        consent.require_original_proposal(self.effects.as_ref(), proposal, &verifier)?;
        if !progress.staged || progress.acceptance.is_none() {
            return Err(AuraError::permission_denied(
                "peer commit requires actual staged acceptance",
            ));
        }
        if progress.committed {
            return Ok(());
        }
        let actual = native.original_authenticated(approval)?;
        if !progress.tree_installed {
            actual
                .install_original_peer_commit(approval, proposal, commit)
                .await?;
            progress.tree_installed = true;
        }
        if progress.activation_started {
            return Err(AuraError::invalid(
                "original peer activation interrupted without completion",
            ));
        }
        let pending = self
            .verify_retained_pending_generation(
                &proposal.subject_authority,
                proposal.pending_epoch,
                aura_core::hash::hash(&proposal.public_key_package),
                aura_core::hash::hash(&proposal.threshold_config),
            )
            .await?;
        progress.activation_started = true;
        self.activate_retained_generation(
            &proposal.subject_authority,
            proposal.pending_epoch,
            Some(&pending),
            Some(actual.generation()),
            None,
        )
        .await?;
        progress.committed = true;
        progress.pending_share = None;
        let accepted = progress
            .acceptance
            .as_ref()
            .ok_or_else(|| AuraError::invalid("actual peer acceptance absent"))?
            .clone();
        let actual = progress
            .material
            .take()
            .ok_or_else(|| AuraError::invalid("actual original positive material absent"))?;
        let ValidatedLocalEnrollmentSigningMaterial {
            effects,
            custody,
            device,
            approved_intent_digest,
            index,
            threshold,
            ordered_devices,
            public_package,
            local_share,
            ..
        } = actual;
        progress.positive_terminal = Some(OriginalPositiveTerminalSigningMaterial {
            effects,
            approved_intent_digest,
            device,
            index,
            threshold,
            ordered_devices,
            public_package,
            local_share,
            original_window: positive_window,
            origin: OriginalPositiveTerminalOrigin::Peer(Box::new(
                OriginalPeerPositiveActivationReceipt {
                    proposal: proposal.clone(),
                    commit: commit.clone(),
                    acceptance: accepted,
                },
            )),
        });
        drop(custody);
        Ok(())
    }
}
