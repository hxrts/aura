//! Consensus helpers for building params and loading key material.

use aura_consensus::protocol::ConsensusParams;
use aura_core::crypto::tree_signing::{public_key_package_from_bytes, share_from_key_package_bytes};
use aura_core::effects::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, ThresholdSigningEffects,
};
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::hash::hash;
use aura_core::threshold::ParticipantIdentity;
use aura_core::types::Epoch;
use aura_core::{AuraError, AuthorityId, Hash32};
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
    sorted.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
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

    let group_public_key = public_key_package_from_bytes(&public_key_bytes).map_err(|e| {
        AuraError::internal(format!("Failed to parse public key package: {e}"))
    })?;

    let mut key_packages = HashMap::new();
    for participant in participants {
        let authority = participant_identity_to_authority_id(participant)?;
        let location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}/{}", authority_id, epoch),
            participant.storage_key(),
        );
        let bytes = effects.secure_retrieve(&location, caps).await.map_err(|e| {
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
    let state = signing_service
        .threshold_state(&authority_id)
        .await
        .ok_or_else(|| {
            AuraError::invalid("Consensus requires an existing threshold configuration".to_string())
        })?;

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
