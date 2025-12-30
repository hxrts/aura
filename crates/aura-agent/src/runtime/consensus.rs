//! Consensus helpers for building params and loading key material.

use aura_consensus::protocol::ConsensusParams;
use aura_core::crypto::tree_signing::{
    public_key_package_from_bytes, share_from_key_package_bytes,
};
use aura_core::effects::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, ThresholdSigningEffects,
};
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::hash::hash;
use aura_core::threshold::ParticipantIdentity;
use aura_core::threshold::ThresholdState;
use aura_core::types::Epoch;
use aura_core::{AuraError, AuthorityId, Hash32};
use serde::Deserialize;
use std::collections::HashMap;

pub(crate) fn participant_identity_to_authority_id(
    identity: &ParticipantIdentity,
) -> Result<AuthorityId, AuraError> {
    match identity {
        ParticipantIdentity::Guardian(id) => Ok(*id),
        ParticipantIdentity::GroupMember { member, .. } => Ok(*member),
        ParticipantIdentity::Device(device_id) => {
            let bytes = device_id.to_bytes().map_err(|_| {
                AuraError::internal("Failed to convert device id to bytes".to_string())
            })?;
            Ok(AuthorityId::new_from_entropy(hash(&bytes)))
        }
    }
}

pub(crate) fn membership_hash_from_participants(participants: &[AuthorityId]) -> Hash32 {
    let mut sorted = participants.to_vec();
    sorted.sort_by_key(|a| a.to_bytes());
    let mut bytes = Vec::with_capacity(sorted.len() * 16);
    for id in sorted {
        bytes.extend_from_slice(&id.to_bytes());
    }
    Hash32(hash(&bytes))
}

pub(crate) async fn load_consensus_key_material(
    effects: &crate::runtime::AuraEffectSystem,
    authority_id: AuthorityId,
    epoch: u64,
    participants: &[ParticipantIdentity],
    public_key_package: Option<Vec<u8>>,
) -> Result<(HashMap<AuthorityId, Share>, PublicKeyPackage), AuraError> {
    if participants.is_empty() {
        return Err(AuraError::invalid(
            "Consensus requires at least one participant".to_string(),
        ));
    }

    let caps = &[SecureStorageCapability::Read];
    let public_key_bytes = match effects
        .secure_retrieve(
            &SecureStorageLocation::with_sub_key(
                "threshold_pubkey",
                format!("{}", authority_id),
                format!("{}", epoch),
            ),
            caps,
        )
        .await
    {
        Ok(bytes) => bytes,
        Err(_) => public_key_package.ok_or_else(|| {
            AuraError::internal("Missing public key package for consensus participants".to_string())
        })?,
    };

    let group_public_key = public_key_package_from_bytes(&public_key_bytes)
        .map_err(|e| AuraError::internal(format!("Failed to parse public key package: {e}")))?;

    let mut key_packages = HashMap::new();
    for participant in participants {
        let authority = participant_identity_to_authority_id(participant)?;
        let location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}/{}", authority_id, epoch),
            participant.storage_key(),
        );
        let bytes = effects
            .secure_retrieve(&location, caps)
            .await
            .map_err(|e| {
                AuraError::internal(format!(
                    "Failed to load key package for {}: {e}",
                    participant.display_name()
                ))
            })?;
        let share = share_from_key_package_bytes(&bytes).map_err(|e| {
            AuraError::internal(format!(
                "Failed to parse key package for {}: {e}",
                participant.display_name()
            ))
        })?;
        key_packages.insert(authority, share);
    }

    Ok((key_packages, group_public_key))
}

pub(crate) async fn build_consensus_params(
    effects: &crate::runtime::AuraEffectSystem,
    authority_id: AuthorityId,
    signing_service: &impl ThresholdSigningEffects,
) -> Result<ConsensusParams, AuraError> {
    let state = match signing_service.threshold_state(&authority_id).await {
        Some(state) => state,
        None => load_threshold_state_from_storage(effects, authority_id).await?,
    };

    let public_key_package = signing_service.public_key_package(&authority_id).await;
    let (key_packages, group_public_key) = load_consensus_key_material(
        effects,
        authority_id,
        state.epoch,
        &state.participants,
        public_key_package,
    )
    .await?;

    let mut witnesses = Vec::with_capacity(state.participants.len());
    for participant in &state.participants {
        witnesses.push(participant_identity_to_authority_id(participant)?);
    }

    Ok(ConsensusParams {
        witnesses,
        threshold: state.threshold,
        key_packages,
        group_public_key,
        epoch: Epoch::new(state.epoch),
    })
}

#[derive(Debug, Deserialize)]
struct ThresholdConfigMetadata {
    threshold_k: u16,
    total_n: u16,
    #[serde(default)]
    participants: Vec<ParticipantIdentity>,
    #[serde(default)]
    agreement_mode: aura_core::threshold::AgreementMode,
}

#[derive(Debug, Deserialize)]
struct ThresholdMetadataFallback {
    threshold: u16,
    total_participants: u16,
    #[serde(default)]
    participants: Vec<ParticipantIdentity>,
    #[serde(default)]
    agreement_mode: aura_core::threshold::AgreementMode,
}

async fn load_threshold_state_from_storage(
    effects: &crate::runtime::AuraEffectSystem,
    authority_id: AuthorityId,
) -> Result<ThresholdState, AuraError> {
    let epoch_location = SecureStorageLocation::new("epoch_state", format!("{}", authority_id));
    let epoch_bytes = effects
        .secure_retrieve(&epoch_location, &[SecureStorageCapability::Read])
        .await
        .map_err(|_| {
            AuraError::invalid("Consensus requires an existing threshold configuration".to_string())
        })?;

    let epoch = if epoch_bytes.len() >= 8 {
        let bytes: [u8; 8] = epoch_bytes[..8].try_into().unwrap_or([0u8; 8]);
        u64::from_le_bytes(bytes)
    } else {
        0
    };

    let config_location = SecureStorageLocation::with_sub_key(
        "threshold_config",
        format!("{}", authority_id),
        format!("{}", epoch),
    );

    let config = match effects
        .secure_retrieve(
            &config_location,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
    {
        Ok(bytes) => {
            let metadata: ThresholdConfigMetadata =
                serde_json::from_slice(&bytes).map_err(|e| {
                    AuraError::internal(format!("Failed to deserialize threshold config: {}", e))
                })?;
            ThresholdState {
                epoch,
                threshold: metadata.threshold_k,
                total_participants: metadata.total_n,
                participants: metadata.participants,
                agreement_mode: metadata.agreement_mode,
            }
        }
        Err(_) => {
            let legacy_location = SecureStorageLocation::with_sub_key(
                "threshold_metadata",
                format!("{}", authority_id),
                format!("{}", epoch),
            );
            let legacy_bytes = effects
                .secure_retrieve(
                    &legacy_location,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await
                .map_err(|_| {
                    AuraError::invalid(
                        "Consensus requires an existing threshold configuration".to_string(),
                    )
                })?;
            let metadata: ThresholdMetadataFallback = serde_json::from_slice(&legacy_bytes)
                .map_err(|e| {
                    AuraError::internal(format!("Failed to deserialize threshold metadata: {}", e))
                })?;
            ThresholdState {
                epoch,
                threshold: metadata.threshold,
                total_participants: metadata.total_participants,
                participants: metadata.participants,
                agreement_mode: metadata.agreement_mode,
            }
        }
    };

    Ok(config)
}
