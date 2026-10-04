//! Confirmed import activation retains its envelope separately from the signed
//! immutable original raw share. IDs never grant decryption or activation.
use super::*;
use crate::handlers::invitation::enrollment_manifest_admission::{
    load_confirmed_enrollment, require_confirmed_import_generation,
    DurableConfirmedEnrollmentCapability,
};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
enum ConfirmedAllocationError {
    #[error("original confirmed wrapping allocation already born without required envelope")]
    AlreadyBorn,
}

/// Holds the actual original reverified local receipt and effect owner.
/// No Clone, Deserialize, raw-id constructor, or ambient activation permission.
pub(crate) struct ConfirmedActivationEnvelopeCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    confirmed: DurableConfirmedEnrollmentCapability,
}

impl ConfirmedActivationEnvelopeCapability<'_> {
    pub(crate) fn confirmed(&self) -> &DurableConfirmedEnrollmentCapability {
        &self.confirmed
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmedActivationEnvelope {
    version: u16,
    manifest_digest: [u8; 32],
    share_digest: [u8; 32],
    original_config: Vec<u8>,
    envelope: Vec<u8>,
}

fn receipt_error(
    source: aura_invitation::enrollment_manifest::EnrollmentManifestError,
) -> AuraError {
    AuraError::PermissionDenied {
        message: "require original confirmed import for activation envelope".into(),
        source: Some(Arc::new(source)),
    }
}

async fn reverify<'runtime>(
    effects: &'runtime AuraEffectSystem,
    confirmed: &DurableConfirmedEnrollmentCapability,
) -> Result<ConfirmedActivationEnvelopeCapability<'runtime>, AuraError> {
    let original = confirmed.confirmation();
    let manifest = original.manifest();
    let local =
        load_confirmed_enrollment(effects, manifest.invitee_authority, &manifest.invitation)
            .await
            .map_err(receipt_error)?;
    require_confirmed_import_generation(effects, &local)
        .await
        .map_err(receipt_error)?;
    let current = local.confirmation();
    if current.manifest_digest() != original.manifest_digest()
        || current.manifest().subject != manifest.subject
        || current.manifest().pending_epoch != manifest.pending_epoch
        || current.manifest().invitee_device != effects.device_id()
        || current.manifest().pending_share_digest != manifest.pending_share_digest
    {
        return Err(AuraError::invalid(
            "confirmed activation envelope belongs to another original generation",
        ));
    }
    Ok(ConfirmedActivationEnvelopeCapability {
        effects,
        confirmed: local,
    })
}

fn envelope_location(owner: &ConfirmedActivationEnvelopeCapability<'_>) -> SecureStorageLocation {
    let proof = owner.confirmed.confirmation();
    let manifest = proof.manifest();
    SecureStorageLocation::with_sub_key(
        "confirmed_enrollment_activation_envelope_v1",
        manifest.subject.to_string(),
        hex::encode(proof.manifest_digest()),
    )
}

fn birth_location(owner: &ConfirmedActivationEnvelopeCapability<'_>) -> SecureStorageLocation {
    let proof = owner.confirmed.confirmation();
    let manifest = proof.manifest();
    SecureStorageLocation::with_sub_key(
        "confirmed_enrollment_activation_birth_v1",
        manifest.subject.to_string(),
        hex::encode(proof.manifest_digest()),
    )
}

impl AuraEffectSystem {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "owner", capability_type = ConfirmedActivationEnvelopeCapability, family = "runtime_helper")]
    async fn encrypt_confirmed_allocation(
        &self,
        owner: &ConfirmedActivationEnvelopeCapability<'_>,
        raw: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let current = reverify(self, owner.confirmed()).await?;
        let scope = EnrollmentSecretScope::confirmed(current.confirmed())?;
        if aura_core::hash::hash(raw) != scope.package_digest {
            return Err(AuraError::invalid(
                "confirmed raw share differs from original allocation scope",
            ));
        }
        let wrap = Zeroizing::new(self.random_bytes_32().await);
        let mut custody = self.crypto.allocation_lifetimes.lock().await;
        let inventory = custody.ready().await?;
        let scope_bytes = scope.encode()?;
        // A provider birth already exists even if envelope publication failed.
        // Such interruption must not allocate another original wrapping secret.
        if inventory
            .references()
            .iter()
            .any(|reference| reference.scope == scope_bytes)
        {
            return Err(AuraError::Storage {
                message: "required confirmed wrapping allocation refused replacement birth".into(),
                source: Some(Arc::new(ConfirmedAllocationError::AlreadyBorn)),
            });
        }
        let allocation = inventory
            .fresh_birth(
                &OwnedSecretBirthCapability {
                    runtime_identity: self.crypto.lifetime_owner_identity(),
                    origin: OriginalSecretBirthOrigin::Confirmed(&current),
                    scope: scope.clone(),
                },
                wrap.as_ref(),
            )
            .await?;
        let nonce = self.random_bytes(12).await;
        let aad = serde_json::to_vec(&(
            "aura:participant-allocation-envelope:v2",
            &scope,
            &allocation,
        ))
        .map_err(|source| AuraError::Serialization {
            message: "encode confirmed allocation envelope AAD".into(),
            source: Some(Arc::new(source)),
        })?;
        let cipher = ChaCha20Poly1305::new((&*wrap).into());
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: raw,
                    aad: &aad,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encrypt actual confirmed owned allocation",
                    Arc::new(source),
                )
            })?;
        let decision = serde_json::to_vec(&(
            2_u16,
            "confirmed-import-committed",
            current.confirmed().confirmation().manifest_digest(),
        ))
        .map_err(|source| AuraError::Serialization {
            message: "encode confirmed allocation positive decision".into(),
            source: Some(Arc::new(source)),
        })?;
        inventory
            .seal_positive(
                &allocation,
                &OwnedSecretPositiveCapability {
                    runtime_identity: self.crypto.lifetime_owner_identity(),
                    origin: OriginalSecretPositiveOrigin::Confirmed(&current),
                    decision: &decision,
                },
            )
            .await?;
        serde_json::to_vec(&AllocationParticipantEnvelope {
            version: 2,
            scope,
            allocation,
            nonce,
            ciphertext,
        })
        .map_err(|source| AuraError::Serialization {
            message: "encode confirmed original allocation envelope".into(),
            source: Some(Arc::new(source)),
        })
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "owner", capability_type = ConfirmedActivationEnvelopeCapability, family = "runtime_helper")]
    async fn decrypt_confirmed_allocation(
        &self,
        owner: &ConfirmedActivationEnvelopeCapability<'_>,
        bytes: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let current = reverify(self, owner.confirmed()).await?;
        if bytes.len() > 131_072 {
            return Err(AuraError::invalid(
                "confirmed allocation envelope exceeds bound",
            ));
        }
        let envelope: AllocationParticipantEnvelope =
            serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
                message: "decode confirmed original allocation envelope".into(),
                source: Some(Arc::new(source)),
            })?;
        let expected = EnrollmentSecretScope::confirmed(current.confirmed())?;
        if envelope.version != 2
            || envelope.scope != expected
            || envelope.nonce.len() != 12
            || envelope.allocation.scope != expected.encode()?
        {
            return Err(AuraError::invalid(
                "confirmed original allocation reader scope differs",
            ));
        }
        let reader = OwnedSecretReadCapability {
            runtime_identity: self.crypto.lifetime_owner_identity(),
            scope: expected,
        };
        let mut custody = self.crypto.allocation_lifetimes.lock().await;
        let inventory = custody.ready().await?;
        let key = Zeroizing::new(
            inventory
                .read_original(&envelope.allocation, &reader)
                .await?,
        );
        let key: &[u8; 32] = key
            .as_slice()
            .try_into()
            .map_err(|_| AuraError::storage("confirmed original wrapping key length"))?;
        let aad = serde_json::to_vec(&(
            "aura:participant-allocation-envelope:v2",
            &envelope.scope,
            &envelope.allocation,
        ))
        .map_err(|source| AuraError::Serialization {
            message: "encode confirmed original allocation read AAD".into(),
            source: Some(Arc::new(source)),
        })?;
        let cipher = ChaCha20Poly1305::new(key.into());
        let clear = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&envelope.nonce),
                    Payload {
                        msg: &envelope.ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "decrypt actual confirmed original allocation",
                        Arc::new(source),
                    )
                })?,
        );
        if aura_core::hash::hash(&clear) != envelope.scope.package_digest {
            return Err(AuraError::invalid(
                "confirmed allocation clear share digest differs",
            ));
        }
        Ok(clear.to_vec())
    }

    #[cfg(all(test, unix))]
    pub(crate) async fn assert_confirmed_managed_allocation_boundaries_for_test(
        &self,
        owner: &ConfirmedActivationEnvelopeCapability<'_>,
    ) -> Result<(), AuraError> {
        // Read the genuine immutable envelope and clear share through the
        // production receipt, birth-anchor, and original-allocation readers.
        let clear = Zeroizing::new(self.decrypt_confirmed_activation_envelope(owner).await?);
        let stored = self
            .secure_retrieve(&envelope_location(owner), &[SecureStorageCapability::Read])
            .await?;
        let retained: ConfirmedActivationEnvelope =
            serde_json::from_slice(&stored).map_err(|source| AuraError::Serialization {
                message: "decode genuine activation for allocation boundary evidence".into(),
                source: Some(Arc::new(source)),
            })?;
        let mut scope_substitution: AllocationParticipantEnvelope =
            serde_json::from_slice(&retained.envelope).map_err(|source| {
                AuraError::Serialization {
                    message: "decode genuine allocation for scope substitution".into(),
                    source: Some(Arc::new(source)),
                }
            })?;
        scope_substitution.scope.original_profile_digest[0] ^= 1;
        let substituted =
            serde_json::to_vec(&scope_substitution).map_err(|source| AuraError::Serialization {
                message: "encode allocation scope substitution".into(),
                source: Some(Arc::new(source)),
            })?;
        let scope_error = self
            .decrypt_confirmed_allocation(owner, &substituted)
            .await
            .expect_err("real receipt cannot authorize a substituted native allocation scope");
        assert!(
            matches!(scope_error, AuraError::Invalid { .. }),
            "{scope_error:?}"
        );

        let mut reference_substitution: AllocationParticipantEnvelope =
            serde_json::from_slice(&retained.envelope).map_err(|source| {
                AuraError::Serialization {
                    message: "decode genuine allocation for reference substitution".into(),
                    source: Some(Arc::new(source)),
                }
            })?;
        reference_substitution.allocation.scope.push(0);
        let substituted = serde_json::to_vec(&reference_substitution).map_err(|source| {
            AuraError::Serialization {
                message: "encode allocation reference substitution".into(),
                source: Some(Arc::new(source)),
            }
        })?;
        let reference_error = self
            .decrypt_confirmed_allocation(owner, &substituted)
            .await
            .expect_err(
                "real receipt cannot authorize a substituted original allocation reference",
            );
        assert!(
            matches!(reference_error, AuraError::Invalid { .. }),
            "{reference_error:?}"
        );

        let before = {
            let mut custody = self.crypto.allocation_lifetimes.lock().await;
            custody.ready().await?.references().len()
        };
        // Enter the actual producer below the envelope/birth-anchor shortcut,
        // as an interrupted publication would. The retained native allocation
        // must independently forbid a second birth for this original scope.
        let birth_error = self
            .encrypt_confirmed_allocation(owner, &clear)
            .await
            .expect_err("already-born original confirmed scope cannot allocate a second wrap");
        assert!(
            matches!(&birth_error, AuraError::Storage { source: Some(source), .. }
            if matches!(source.downcast_ref::<ConfirmedAllocationError>(), Some(ConfirmedAllocationError::AlreadyBorn))),
            "{birth_error:?}"
        );
        let after = {
            let mut custody = self.crypto.allocation_lifetimes.lock().await;
            custody.ready().await?.references().len()
        };
        assert_eq!(
            before, after,
            "rejected second birth does not publish another allocation"
        );
        assert_eq!(
            self.decrypt_confirmed_activation_envelope(owner).await?,
            *clear,
            "negative allocation attempts leave the original usable"
        );
        Ok(())
    }

    #[cfg(all(test, unix))]
    pub(crate) fn confirmed_activation_record_location_for_test(
        &self,
        owner: &ConfirmedActivationEnvelopeCapability<'_>,
    ) -> SecureStorageLocation {
        envelope_location(owner)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "owner", capability_type = ConfirmedActivationEnvelopeCapability, family = "runtime_helper")]
    pub(crate) async fn confirmed_activation_finalized_config(
        &self,
        owner: &ConfirmedActivationEnvelopeCapability<'_>,
    ) -> Result<crate::runtime::effects::ThresholdConfigMetadata, AuraError> {
        let original = self.confirmed_activation_original_config(owner).await?;
        let mut metadata: crate::runtime::effects::ThresholdConfigMetadata =
            serde_json::from_slice(&original).map_err(|source| AuraError::Serialization {
                message: "decode sealed original activation configuration".into(),
                source: Some(Arc::new(source)),
            })?;
        // Only agreement changes after genuine committed receipt verification.
        // Every original policy/material field is retained exactly.
        metadata.agreement_mode = aura_core::threshold::AgreementMode::ConsensusFinalized;
        Ok(metadata)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "confirmed", capability_type = DurableConfirmedEnrollmentCapability, family = "runtime_helper")]
    pub(crate) async fn load_confirmed_activation_envelope<'runtime>(
        &'runtime self,
        confirmed: &DurableConfirmedEnrollmentCapability,
    ) -> Result<ConfirmedActivationEnvelopeCapability<'runtime>, AuraError> {
        let owner = reverify(self, confirmed).await?;
        let _clear = Zeroizing::new(self.decrypt_confirmed_activation_envelope(&owner).await?);
        Ok(owner)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "confirmed", capability_type = DurableConfirmedEnrollmentCapability, family = "runtime_helper")]
    pub(crate) async fn retain_confirmed_activation_envelope<'runtime>(
        &'runtime self,
        confirmed: &DurableConfirmedEnrollmentCapability,
    ) -> Result<ConfirmedActivationEnvelopeCapability<'runtime>, AuraError> {
        let owner = reverify(self, confirmed).await?;
        let target = envelope_location(&owner);
        // Repeated activation acknowledges the exact original envelope. Never
        // reencrypt before learning whether this original record already exists.
        if self.secure_exists(&target).await? {
            let _clear = Zeroizing::new(self.decrypt_confirmed_activation_envelope(&owner).await?);
            return Ok(owner);
        }
        if self.secure_exists(&birth_location(&owner)).await? {
            // Original birth is irreversible: missing envelope is a required
            // storage read failure, never permission to allocate another nonce.
            let _clear = Zeroizing::new(self.decrypt_confirmed_activation_envelope(&owner).await?);
            return Ok(owner);
        }
        let proof = owner.confirmed.confirmation();
        let manifest = proof.manifest();
        let participant = ParticipantIdentity::device(manifest.invitee_device);
        let raw_location = Self::participant_share_location(
            &manifest.subject,
            manifest.pending_epoch,
            &participant,
        );
        let raw = Zeroizing::new(
            self.secure_retrieve(&raw_location, &[SecureStorageCapability::Read])
                .await?,
        );
        if aura_core::hash::hash(&raw) != manifest.pending_share_digest {
            return Err(AuraError::invalid(
                "original signed immutable imported share digest changed",
            ));
        }
        let config_location = SecureStorageLocation::with_sub_key(
            "threshold_config",
            manifest.subject.to_string(),
            manifest.pending_epoch.to_string(),
        );
        let original_config = self
            .secure_retrieve(&config_location, &[SecureStorageCapability::Read])
            .await?;
        if aura_core::hash::hash(&original_config)
            != *manifest.pending_threshold_config_digest.as_bytes()
        {
            return Err(AuraError::invalid(
                "original signed immutable imported config digest changed",
            ));
        }
        let envelope = self.encrypt_confirmed_allocation(&owner, &raw).await?;
        let record = ConfirmedActivationEnvelope {
            version: 1,
            manifest_digest: proof.manifest_digest(),
            share_digest: manifest.pending_share_digest,
            original_config,
            envelope,
        };
        let bytes = serde_json::to_vec(&record).map_err(|source| AuraError::Serialization {
            message: "encode original confirmed activation envelope".into(),
            source: Some(Arc::new(source)),
        })?;
        let birth = aura_core::hash::hash(&bytes);
        let outcome = self
            .secure_store_immutable(
                &birth_location(&owner),
                &birth,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        if matches!(
            outcome,
            aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
        ) {
            let _clear = Zeroizing::new(self.decrypt_confirmed_activation_envelope(&owner).await?);
            return Ok(owner);
        }
        self.secure_store_immutable(
            &target,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
        // A concurrent producer's original envelope is accepted only after its
        // actual native decryption and exact cleartext commitment revalidate.
        let _clear = Zeroizing::new(self.decrypt_confirmed_activation_envelope(&owner).await?);
        Ok(owner)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "owner", capability_type = ConfirmedActivationEnvelopeCapability, family = "runtime_helper")]
    pub(crate) async fn confirmed_activation_original_config(
        &self,
        owner: &ConfirmedActivationEnvelopeCapability<'_>,
    ) -> Result<Vec<u8>, AuraError> {
        let _clear = Zeroizing::new(self.decrypt_confirmed_activation_envelope(owner).await?);
        let current = reverify(self, &owner.confirmed).await?;
        let bytes = self
            .secure_retrieve(
                &envelope_location(&current),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 262_144 {
            return Err(AuraError::invalid(
                "confirmed activation envelope exceeds bound",
            ));
        }
        let record: ConfirmedActivationEnvelope =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
                message: "read sealed original activation config".into(),
                source: Some(Arc::new(source)),
            })?;
        if record.version != 1
            || record.manifest_digest != current.confirmed.confirmation().manifest_digest()
            || aura_core::hash::hash(&record.original_config)
                != *current
                    .confirmed
                    .confirmation()
                    .manifest()
                    .pending_threshold_config_digest
                    .as_bytes()
        {
            return Err(AuraError::invalid(
                "sealed original activation config binding changed",
            ));
        }
        Ok(record.original_config)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "owner", capability_type = ConfirmedActivationEnvelopeCapability, family = "runtime_helper")]
    pub(crate) async fn decrypt_confirmed_activation_envelope(
        &self,
        owner: &ConfirmedActivationEnvelopeCapability<'_>,
    ) -> Result<Vec<u8>, AuraError> {
        if !std::ptr::eq(self, owner.effects) {
            return Err(AuraError::invalid(
                "activation envelope effect owner differs",
            ));
        }
        let current = reverify(self, &owner.confirmed).await?;
        let bytes = self
            .secure_retrieve(
                &envelope_location(&current),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 262_144 {
            return Err(AuraError::invalid(
                "confirmed activation envelope exceeds bound",
            ));
        }
        let record: ConfirmedActivationEnvelope =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
                message: "decode original confirmed activation envelope".into(),
                source: Some(Arc::new(source)),
            })?;
        let birth = self
            .secure_retrieve(&birth_location(&current), &[SecureStorageCapability::Read])
            .await?;
        if birth.as_slice() != aura_core::hash::hash(&bytes).as_slice() {
            return Err(AuraError::storage(
                "confirmed activation original birth digest differs",
            ));
        }
        let proof = current.confirmed.confirmation();
        let manifest = proof.manifest();
        if record.version != 1
            || record.manifest_digest != proof.manifest_digest()
            || record.share_digest != manifest.pending_share_digest
            || aura_core::hash::hash(&record.original_config)
                != *manifest.pending_threshold_config_digest.as_bytes()
        {
            return Err(AuraError::invalid(
                "confirmed activation envelope commitment differs",
            ));
        }
        let clear = Zeroizing::new(
            self.decrypt_confirmed_allocation(&current, &record.envelope)
                .await?,
        );
        if aura_core::hash::hash(&clear) != record.share_digest {
            return Err(AuraError::invalid(
                "confirmed activated share commitment differs",
            ));
        }
        Ok(clear.to_vec())
    }
}
