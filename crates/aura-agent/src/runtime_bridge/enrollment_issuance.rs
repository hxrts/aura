//! Original enrollment preparation remains owned by one runtime task.
use super::*;

impl AgentRuntimeBridge {
    pub(crate) async fn issue_original_device_enrollment(
        &self,
        nickname_suggestion: String,
        setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        preparation: Option<super::enrollment_quorum::OriginalIssuerPreparation>,
    ) -> Result<
        aura_app::runtime_bridge::DeviceEnrollmentStart,
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
            .prepare_authenticated_enrollment_rotation(&setup, &issuance_tracker)
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
        let (pending_epoch, key_packages, _public_key, generation_reservation) = effects
            .prepare_pinned_enrollment_rotation(&setup, &reserved, &ceremony_id, plan)
            .await
            .map_err(|e| IssueError::at(Stage::Rotation, e))?;
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
        let response_policy = generation_reservation
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
                    &generation_reservation,
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
                &generation_reservation,
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
            )
            .await
            .map_err(|e| IssueError::at(Stage::CeremonyRegistration, e))?;

        let original_window = if preparation.is_some() {
            Some(crate::runtime::services::enrollment_window::EnrollmentWindowCapability::held_issuer(
                effects.clone(), &issuance_tracker, &generation_reservation,
            ).await.map_err(|source| IssueError::at(Stage::CeremonyRegistration, source))?)
        } else {
            None
        };

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
        let final_inventory = effects
            .capture_enrollment_final_inventory(&generation_reservation, &baseline_ops)
            .await
            .map_err(|e| IssueError::at(Stage::BaselineExport, e))?;
        let final_state = aura_journal::commitment_tree::reduce(&baseline_ops)
            .map_err(|e| IssueError::at(Stage::BaselineExport, e))?;
        let identity_context = if preparation.is_none() {
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
            version: 3,
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
        let mut quorum_transport = None;
        let (manifest_transfer, issued_manifest) = if let Some(preparation) = preparation {
            let (preview, public_intent, frozen_metadata) = invitation_service
                .preview_owned_enrollment_transport(
                    &reserved,
                    &final_inventory,
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
            let window = original_window.as_ref().ok_or(IssueError::InvalidPolicy)?;
            let approval = preparation
                .require_original_user_approval(
                    effects.clone(),
                    &manifest,
                    &public_intent,
                    &setup,
                    window,
                )
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            let tree_owner =
                crate::runtime::effects::EnrollmentTranscriptTreeOwner::from_original_issuer(
                    &final_inventory,
                );
            let [manifest_signature, transport_signature, request_signature] = self
                .agent
                .threshold_signing()
                .sign_original_approved_enrollment_domains(&approval, &tree_owner, window)
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            quorum_transport = Some((preview, public_intent, frozen_metadata, transport_signature));
            let exported = invitation_service
                .export_quorum_enrollment_manifest(
                    &reserved,
                    &setup,
                    &final_inventory,
                    manifest,
                    manifest_signature,
                )
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            invitation_service
                .retain_quorum_initial_request(
                    &exported.1,
                    &final_inventory,
                    &approval,
                    request_signature,
                )
                .await
                .map_err(|source| IssueError::at(Stage::InvitationExport, source))?;
            exported
        } else {
            invitation_service
                .export_owned_enrollment_manifest(
                    &reserved,
                    &setup,
                    identity_context.as_ref().ok_or(IssueError::InvalidPolicy)?,
                    &final_inventory,
                    manifest,
                )
                .await
                .map_err(|e| IssueError::at(Stage::InvitationExport, e))?
        };
        #[cfg(test)]
        eprintln!("enrollment initiation stage: retain original issued manifest");
        crate::handlers::invitation::enrollment_trust::retain_issued_enrollment_manifest(
            effects.as_ref(),
            &issued_manifest,
        )
        .await
        .map_err(|e| IssueError::at(Stage::SetupVerifierRetention, e))?;
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
        // Release the exact prepared execution lease only after both original
        // domains and required actor acknowledgment have completed.
        drop(original_window);

        let registration = self
            .agent
            .runtime()
            .ceremony_tracker()
            .get(&ceremony_id)
            .await
            .map_err(|error| IssueError::at(Stage::CeremonyRegistration, error))?;
        crate::handlers::invitation::enrollment_trust::persist_pending_enrollment_registration(
            effects.as_ref(),
            &generation_reservation,
            &registration,
            &invitation,
        )
        .await
        .map_err(|error| IssueError::at(Stage::SetupVerifierRetention, error))?;
        let registered_generation = generation_reservation
            .complete_registration()
            .await
            .map_err(|e| IssueError::at(Stage::CeremonyRegistration, e))?;

        let admission = invitation_service
            .start_registered_device_enrollment(&registered_generation)
            .await
            .map_err(|error| IssueError::at(Stage::CeremonyRegistration, error))?;
        // With no other devices, no rotation session will commit the enrollment;
        // finalize it here once the new device's signed acceptance is verified.
        if other_device_ids.is_empty()
            && admission
                == crate::handlers::invitation_service::DeviceEnrollmentInitiatorStart::Started
        {
            self.spawn_sole_device_enrollment_finalizer(ceremony_id.clone());
        }

        // Launch device-scoped rotation sessions for existing devices so they can
        // stage and commit the new epoch through one protocol path.
        if !other_device_ids.is_empty()
            && admission
                == crate::handlers::invitation_service::DeviceEnrollmentInitiatorStart::Started
        {
            for device_id in &other_device_ids {
                let Some(key_package) = key_package_by_device.get(device_id).cloned() else {
                    return Err(IssueError::MissingPackage(*device_id));
                };
                self.spawn_device_epoch_rotation(
                    crate::handlers::device_epoch_rotation::DeviceEpochRotationInitRequest {
                        ceremony_id: ceremony_id.clone(),
                        kind: aura_sync::protocols::DeviceEpochRotationKind::Enrollment,
                        pending_epoch: pending_epoch.value(),
                        participant_device_id: *device_id,
                        key_package,
                        threshold_config: threshold_config.clone(),
                        public_key_package: public_key_package.clone(),
                    },
                );
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
        Ok(aura_app::runtime_bridge::DeviceEnrollmentStart {
            ceremony_id: ceremony_id.clone(),
            enrollment_code,
            pending_epoch,
            device_id: new_device_id,
            manifest_transfer: Some(manifest_transfer),
        })
    }
}
