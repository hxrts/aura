use aura_app::IntentError;
use aura_consensus::protocol::ConsensusParams;
use aura_core::byzantine::{ByzantineSafetyAttestation, CapabilitySnapshot};
use aura_core::effects::{AdmissionError, CapabilityKey, RuntimeCapabilityEffects};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::{AuraError, Hash32, Prestate};
use aura_effects::RuntimeCapabilityHandler;
use aura_protocol::admission::{
    required_capability_keys, validate_consensus_profile_capabilities, ConsensusCapabilityProfile,
    PROTOCOL_AURA_CONSENSUS, PROTOCOL_DKG_CEREMONY,
};

use crate::core::default_context_id_for_authority;
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
