use aura_app::IntentError;
use aura_consensus::protocol::runners::{execute_as as consensus_execute_as, AuraConsensusRole};
use aura_consensus::protocol::ConsensusParams;
use aura_core::byzantine::{ByzantineSafetyAttestation, CapabilitySnapshot};
use aura_core::effects::{AdmissionError, CapabilityKey, RuntimeCapabilityEffects};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::{AuraError, Hash32, PhysicalTimeEffects, Prestate, TimeEffects};
use aura_effects::RuntimeCapabilityHandler;
use aura_guards::prelude::{GuardContextProvider, GuardEffects};
use aura_protocol::admission::{
    required_capability_keys, validate_consensus_profile_capabilities, ConsensusCapabilityProfile,
    PROTOCOL_AURA_CONSENSUS, PROTOCOL_DKG_CEREMONY,
};
use aura_protocol::effects::ChoreographicEffects;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::core::default_context_id_for_authority;
use crate::runtime::choreography_adapter::AuraProtocolAdapter;
use crate::runtime::consensus::{
    membership_hash_from_participants, participant_identity_to_authority_id,
};

fn capability_ref(capability: &CapabilityKey) -> String {
    let digest = aura_core::hash::hash(capability.as_str().as_bytes());
    let mut out = String::with_capacity(12);
    for byte in digest.iter().take(6) {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn capability_refs(capabilities: &[CapabilityKey]) -> Vec<String> {
    capabilities.iter().map(capability_ref).collect()
}

fn consensus_runtime_capability_handler() -> RuntimeCapabilityHandler {
    #[cfg(feature = "choreo-backend-telltale-vm")]
    {
        let contracts = telltale_vm::runtime_contracts::RuntimeContracts::full();
        return RuntimeCapabilityHandler::from_runtime_contracts(&contracts);
    }

    #[cfg(not(feature = "choreo-backend-telltale-vm"))]
    {
        RuntimeCapabilityHandler::from_pairs([
            ("byzantine_envelope", true),
            ("termination_bounded", true),
            ("mixed_determinism", true),
            ("reconfiguration", true),
        ])
    }
}

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
    let required_capabilities = required_capability_keys(PROTOCOL_DKG_CEREMONY);
    let required_refs = capability_refs(&required_capabilities);
    let capability_handler = consensus_runtime_capability_handler();
    let capability_snapshot = capability_handler
        .capability_inventory()
        .await
        .map_err(|e| {
            IntentError::internal_error(format!("Capability inventory unavailable: {e}"))
        })?;
    let snapshot_admitted = capability_snapshot
        .iter()
        .filter(|(_, admitted)| *admitted)
        .count();
    tracing::debug!(
        protocol = PROTOCOL_AURA_CONSENSUS,
        epoch,
        capability_inventory_size = capability_snapshot.len(),
        capability_inventory_admitted = snapshot_admitted,
        required_capability_refs = ?required_refs,
        "Byzantine admission capability snapshot captured"
    );

    if let Err(error) = validate_consensus_profile_capabilities(
        ConsensusCapabilityProfile::ThresholdSigning,
        &capability_snapshot,
    ) {
        let missing_ref = match &error {
            AdmissionError::MissingCapability { capability } => capability_ref(capability),
            _ => "unknown".to_string(),
        };
        tracing::error!(
            protocol = PROTOCOL_AURA_CONSENSUS,
            epoch,
            required_capability_refs = ?required_refs,
            missing_capability_ref = %missing_ref,
            "Byzantine admission mismatch: consensus profile requirement failed"
        );
        return Err(IntentError::internal_error(format!(
            "Consensus profile capability validation failed: {error}"
        )));
    }
    tracing::debug!(
        protocol = PROTOCOL_AURA_CONSENSUS,
        epoch,
        required_capability_refs = ?required_refs,
        "Byzantine admission profile validation passed"
    );

    if let Err(error) = capability_handler
        .require_capabilities(&required_capabilities)
        .await
    {
        let missing_ref = match &error {
            AdmissionError::MissingCapability { capability } => capability_ref(capability),
            _ => "unknown".to_string(),
        };
        tracing::error!(
            protocol = PROTOCOL_AURA_CONSENSUS,
            epoch,
            required_capability_refs = ?required_refs,
            missing_capability_ref = %missing_ref,
            "Byzantine admission mismatch: required capability denied before DKG"
        );
        return Err(IntentError::internal_error(format!(
            "Missing Byzantine safety evidence before DKG: {error}"
        )));
    }
    tracing::debug!(
        protocol = PROTOCOL_AURA_CONSENSUS,
        epoch,
        required_capability_refs = ?required_refs,
        "Byzantine admission capability verification passed"
    );
    let byzantine_attestation = ByzantineSafetyAttestation::new(
        PROTOCOL_AURA_CONSENSUS,
        required_capabilities.clone(),
        CapabilitySnapshot::from_inventory("runtime_bridge.consensus", capability_snapshot),
        vec!["evidence://runtime-bridge/consensus".to_string()],
    );
    let (commit, consensus_commit) = aura_consensus::dkg::run_consensus_dkg(
        &prestate,
        context,
        &config,
        packages,
        &store,
        params,
        effects.as_ref(),
        effects.as_ref(),
        Some(byzantine_attestation),
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
    let required_capabilities = required_capability_keys(PROTOCOL_AURA_CONSENSUS);
    let capability_handler = Arc::new(consensus_runtime_capability_handler());

    let mut adapter = AuraProtocolAdapter::new(effects.clone(), authority_id, role, role_map)
        .with_role_family("Witness", witness_roles)
        .with_runtime_capability_admission(capability_handler, required_capabilities);

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
