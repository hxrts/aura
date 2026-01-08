use aura_app::IntentError;
use aura_consensus::protocol::runners::{execute_as as consensus_execute_as, AuraConsensusRole};
use aura_consensus::protocol::ConsensusParams;
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::{AuraError, Hash32, PhysicalTimeEffects, Prestate, TimeEffects};
use aura_guards::prelude::{GuardContextProvider, GuardEffects};
use aura_protocol::effects::ChoreographicEffects;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::core::default_context_id_for_authority;
use crate::runtime::choreography_adapter::AuraProtocolAdapter;
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

/// Execute the AuraConsensus choreography as a specific role.
///
/// This function sets up the protocol adapter and executes the choreography-generated
/// runner for the specified role. The adapter handles message routing and role family
/// resolution for broadcast/collect operations to witnesses.
///
/// # Arguments
///
/// * `effects` - Choreographic effects implementation for message passing
/// * `authority_id` - The local authority's ID
/// * `role` - Which role to execute as (Coordinator or Witness)
/// * `witnesses` - List of witness authority IDs for the Witness[N] role family
/// * `role_map` - Mapping from AuraConsensusRole to AuthorityId for all participants
/// * `session_id` - Unique identifier for this consensus session
///
/// # Example
///
/// ```ignore
/// let witnesses = vec![witness1, witness2, witness3];
/// let mut role_map = HashMap::new();
/// role_map.insert(AuraConsensusRole::Coordinator, coordinator_id);
/// for (i, &w) in witnesses.iter().enumerate() {
///     role_map.insert(AuraConsensusRole::Witness(i as u32), w);
/// }
///
/// execute_consensus_as(
///     effects,
///     coordinator_id,
///     AuraConsensusRole::Coordinator,
///     witnesses.clone(),
///     role_map,
///     session_id,
/// ).await?;
/// ```
#[allow(dead_code)]
pub(super) async fn execute_consensus_as<E>(
    effects: Arc<E>,
    authority_id: AuthorityId,
    role: AuraConsensusRole,
    witnesses: Vec<AuthorityId>,
    role_map: HashMap<AuraConsensusRole, AuthorityId>,
    session_id: Uuid,
) -> Result<(), IntentError>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + PhysicalTimeEffects
        + TimeEffects,
{
    // Build witness roles for the role family
    let witness_roles: Vec<AuraConsensusRole> = (0..witnesses.len())
        .map(|i| AuraConsensusRole::Witness(i as u32))
        .collect();

    // Create the protocol adapter with the Witness role family registered
    let mut adapter = AuraProtocolAdapter::new(effects.clone(), authority_id, role, role_map)
        .with_role_family("Witness", witness_roles);

    // Start the choreography session
    adapter
        .start_session(session_id)
        .await
        .map_err(|e| IntentError::internal_error(format!("Failed to start session: {e}")))?;

    // Execute the choreography as the specified role
    let result = consensus_execute_as(role, &mut adapter)
        .await
        .map_err(|e| IntentError::internal_error(format!("Consensus execution failed: {e}")));

    // End the session (best-effort cleanup)
    let _ = adapter.end_session().await;

    result
}
