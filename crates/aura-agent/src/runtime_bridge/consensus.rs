use aura_app::IntentError;
use aura_consensus::protocol::ConsensusParams;
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::{AuraError, Hash32, Prestate};

use crate::core::default_context_id_for_authority;
use crate::runtime::consensus::{
    membership_hash_from_participants, participant_identity_to_authority_id,
};

pub(super) fn map_consensus_error(err: AuraError) -> IntentError {
    IntentError::internal_error(format!("{err}"))
}

/// Persist a consensus DKG transcript after successful key generation.
///
/// All parameters are semantically distinct and required for DKG transcript persistence:
/// effects (runtime), prestate (consensus state), params (config), authority (owner),
/// epoch (version), threshold/max_signers (k-of-n), participants (signers), operation_hash (tracking).
#[allow(clippy::too_many_arguments)]
pub(super) async fn persist_consensus_dkg_transcript(
    effects: std::sync::Arc<crate::runtime::AuraEffectSystem>,
    prestate: Prestate,
    params: ConsensusParams,
    authority_id: AuthorityId,
    epoch: u64,
    threshold: u16,
    max_signers: u16,
    participants: &[ParticipantIdentity],
    operation_hash: Hash32,
) -> Result<Option<Hash32>, IntentError> {
    let mut participant_ids = Vec::with_capacity(participants.len());
    for participant in participants {
        participant_ids
            .push(participant_identity_to_authority_id(participant).map_err(map_consensus_error)?);
    }

    let membership_hash = membership_hash_from_participants(&participant_ids);
    let context = default_context_id_for_authority(authority_id);
    let prestate_hash = prestate.compute_hash();

    let config = aura_consensus::dkg::DkgConfig {
        epoch,
        threshold,
        max_signers,
        membership_hash,
        cutoff: epoch,
        prestate_hash,
        operation_hash,
        participants: participant_ids.clone(),
    };

    let mut packages = Vec::with_capacity(participant_ids.len());
    for dealer in participant_ids {
        let package =
            aura_consensus::dkg::dealer::build_dealer_package(&config, dealer).map_err(|e| {
                IntentError::internal_error(format!("Failed to build dealer package: {e}"))
            })?;
        packages.push(package);
    }

    let store = aura_consensus::dkg::StorageTranscriptStore::new_default(effects.clone());
    let (commit, consensus_commit) = aura_consensus::dkg::run_consensus_dkg(
        &prestate,
        context,
        &config,
        packages,
        &store,
        params,
        effects.as_ref(),
        effects.as_ref(),
    )
    .await
    .map_err(|e| IntentError::internal_error(format!("Finalize DKG transcript failed: {e}")))?;

    effects
        .commit_relational_facts(vec![
            aura_journal::fact::RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::DkgTranscriptCommit(commit.clone()),
            ),
            consensus_commit.to_relational_fact(),
        ])
        .await
        .map_err(|e| IntentError::internal_error(format!("Commit DKG fact failed: {e}")))?;

    tracing::info!(
        authority_id = %authority_id,
        epoch,
        "Persisted consensus-backed DKG transcript"
    );

    Ok(commit.blob_ref.or(Some(commit.transcript_hash)))
}
