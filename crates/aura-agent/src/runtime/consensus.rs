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
use aura_core::{AuraError, AuthorityId, ContextId, Hash32};
use serde::Deserialize;
use std::collections::HashMap;

pub(crate) fn participant_identity_to_authority_id(
    identity: &ParticipantIdentity,
) -> Result<AuthorityId, AuraError> {
    match identity {
        ParticipantIdentity::Guardian(id) => Ok(*id),
        ParticipantIdentity::GroupMember { member, .. } => Ok(*member),
        ParticipantIdentity::Device(device_id) => Err(AuraError::invalid(format!(
            "Consensus participants must carry explicit authorities; device participant {device_id} is not valid here"
        ))),
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
                    participant.debug_label()
                ))
            })?;
        let share = share_from_key_package_bytes(&bytes).map_err(|e| {
            AuraError::internal(format!(
                "Failed to parse key package for {}: {e}",
                participant.debug_label()
            ))
        })?;
        key_packages.insert(authority, share);
    }

    Ok((key_packages, group_public_key))
}

pub(crate) async fn build_consensus_params(
    context_id: ContextId,
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
        context_id,
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

    let metadata: ThresholdConfigMetadata = effects
        .secure_retrieve(
            &config_location,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .map_err(|_| {
            AuraError::invalid("Consensus requires an existing threshold configuration".to_string())
        })
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|e| {
                AuraError::internal(format!("Failed to deserialize threshold config: {}", e))
            })
        })?;

    Ok(ThresholdState {
        epoch,
        threshold: metadata.threshold_k,
        total_participants: metadata.total_n,
        participants: metadata.participants,
        agreement_mode: metadata.agreement_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime::AuraEffectSystem;
    use aura_core::effects::ThresholdSigningEffects;

    #[tokio::test]
    async fn load_threshold_state_uses_threshold_config_metadata_written_by_effects() {
        let config = AgentConfig::default();
        let effects = AuraEffectSystem::simulation_for_test(&config)
            .expect("simulation effect system should build");
        let authority = AuthorityId::new_from_entropy([21u8; 32]);
        let participants = vec![ParticipantIdentity::Device(effects.device_id())];

        let (new_epoch, _, _) = effects
            .rotate_keys(&authority, 1, 1, &participants)
            .await
            .expect("effect-layer rotate_keys should still write legacy metadata");
        effects
            .commit_key_rotation(&authority, new_epoch)
            .await
            .expect("effect-layer commit should update epoch state");

        let state = load_threshold_state_from_storage(&effects, authority)
            .await
            .expect("consensus should read the shared threshold_config record");

        assert_eq!(state.epoch, new_epoch);
        assert_eq!(state.threshold, 1);
        assert_eq!(state.total_participants, 1);
        assert_eq!(
            state.agreement_mode,
            aura_core::threshold::AgreementMode::CoordinatorSoftSafe
        );
    }
}
