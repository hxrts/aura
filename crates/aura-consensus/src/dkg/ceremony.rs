//! Orchestration glue for DKG + consensus integration.

use super::{
    storage::DkgTranscriptStore,
    transcript::{build_transcript_commit, finalize_transcript},
    types::{DealerPackage, DkgConfig, DkgTranscript},
    verifier::verify_dealer_package,
};
use aura_core::{AuraError, ContextId, Result};
use aura_journal::fact::DkgTranscriptCommit;

pub fn run_dkg_ceremony(config: &DkgConfig, packages: Vec<DealerPackage>) -> Result<DkgTranscript> {
    if packages.len() < config.threshold as usize {
        return Err(AuraError::invalid(
            "DKG ceremony requires at least threshold packages",
        ));
    }

    for package in &packages {
        verify_dealer_package(package)?;
    }

    finalize_transcript(
        config.epoch,
        config.membership_hash,
        config.cutoff,
        packages,
    )
}

pub fn persist_transcript<S: DkgTranscriptStore + ?Sized>(
    store: &S,
    context: ContextId,
    transcript: &DkgTranscript,
) -> Result<DkgTranscriptCommit> {
    let blob_ref = store.put(transcript)?;
    Ok(build_transcript_commit(context, transcript, blob_ref))
}
