//! Orchestration glue for DKG + consensus integration.

use super::{
    storage::DkgTranscriptStore,
    transcript::{build_transcript_commit, finalize_transcript},
    types::{DealerPackage, DkgConfig, DkgTranscript},
    verifier::verify_dealer_package,
};
use crate::protocol::{run_consensus, ConsensusParams};
use crate::types::CommitFact;
use aura_core::byzantine::ByzantineSafetyAttestation;
use aura_core::effects::{PhysicalTimeEffects, RandomEffects};
use aura_core::{AuraError, ContextId, Prestate, Result};
use aura_journal::fact::DkgTranscriptCommit;
use std::collections::BTreeSet;

fn invalid_dkg(reason: &'static str) -> AuraError {
    AuraError::invalid(reason)
}

/// Aggregate a DKG transcript from validated dealer packages.
pub fn aggregate_dkg_transcript(
    config: &DkgConfig,
    packages: Vec<DealerPackage>,
) -> Result<DkgTranscript> {
    validate_config(config)?;
    validate_packages(config, &packages)?;

    for package in &packages {
        verify_dealer_package(package)?;
    }

    finalize_transcript(config, packages)
}

pub fn run_dkg_ceremony(config: &DkgConfig, packages: Vec<DealerPackage>) -> Result<DkgTranscript> {
    aggregate_dkg_transcript(config, packages)
}

pub async fn persist_transcript<S: DkgTranscriptStore + ?Sized>(
    store: &S,
    context: ContextId,
    transcript: &DkgTranscript,
    byzantine_attestation: Option<ByzantineSafetyAttestation>,
) -> Result<DkgTranscriptCommit> {
    let blob_ref = store.put(transcript).await?;
    Ok(build_transcript_commit(
        context,
        transcript,
        blob_ref,
        byzantine_attestation,
    ))
}

/// Run a consensus-backed DKG transcript finalization.
pub async fn run_consensus_dkg<S: DkgTranscriptStore + ?Sized>(
    prestate: &Prestate,
    context: ContextId,
    config: &DkgConfig,
    packages: Vec<DealerPackage>,
    store: &S,
    params: ConsensusParams,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
    byzantine_attestation: Option<ByzantineSafetyAttestation>,
) -> Result<(DkgTranscriptCommit, CommitFact)> {
    let transcript = run_dkg_ceremony(config, packages)?;
    let commit =
        persist_transcript(store, context, &transcript, byzantine_attestation.clone()).await?;
    let mut consensus_commit = run_consensus(prestate, &commit, params, random, time).await?;
    if let Some(attestation) = byzantine_attestation {
        consensus_commit = consensus_commit.with_byzantine_attestation(attestation);
    }
    Ok((commit, consensus_commit))
}

fn validate_config(config: &DkgConfig) -> Result<()> {
    if config.participants.is_empty() {
        return Err(invalid_dkg("DKG config requires explicit participants"));
    }
    if config.threshold == 0 {
        return Err(invalid_dkg("DKG threshold must be non-zero"));
    }
    if config.threshold as usize > config.participants.len() {
        return Err(invalid_dkg("DKG threshold exceeds participant count"));
    }
    if config.max_signers as usize > config.participants.len() {
        return Err(invalid_dkg("DKG max_signers exceeds participant count"));
    }
    Ok(())
}

fn validate_packages(config: &DkgConfig, packages: &[DealerPackage]) -> Result<()> {
    if packages.len() < config.threshold as usize {
        return Err(invalid_dkg(
            "DKG ceremony requires at least threshold packages",
        ));
    }
    if packages.len() > config.max_signers as usize {
        return Err(invalid_dkg(
            "DKG ceremony exceeds max_signers package count",
        ));
    }

    let mut seen = BTreeSet::new();
    for package in packages {
        if !seen.insert(package.dealer) {
            return Err(invalid_dkg("Duplicate dealer package detected"));
        }

        for participant in &config.participants {
            if !package.encrypted_shares.contains_key(participant) {
                return Err(invalid_dkg("Dealer package missing participant share"));
            }
        }
    }

    Ok(())
}
