//! Transcript accumulation and finalization (BFT-DKG).

use super::types::{DealerPackage, DkgTranscript};
use aura_core::{hash, AuraError, Hash32, Result};
use aura_journal::fact::DkgTranscriptCommit;
use aura_core::ContextId;

pub fn compute_transcript_hash(packages: &[DealerPackage]) -> Result<Hash32> {
    let encoded = bincode::serialize(packages)
        .map_err(|e| AuraError::serialization(e.to_string()))?;
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_DKG_TRANSCRIPT");
    hasher.update(&encoded);
    Ok(Hash32(hasher.finalize()))
}

pub fn finalize_transcript(
    epoch: u64,
    membership_hash: Hash32,
    cutoff: u64,
    packages: Vec<DealerPackage>,
) -> Result<DkgTranscript> {
    let transcript_hash = compute_transcript_hash(&packages)?;
    Ok(DkgTranscript {
        epoch,
        membership_hash,
        cutoff,
        packages,
        transcript_hash,
    })
}

pub fn build_transcript_commit(
    context: ContextId,
    transcript: &DkgTranscript,
    blob_ref: Option<Hash32>,
) -> DkgTranscriptCommit {
    DkgTranscriptCommit {
        context,
        epoch: transcript.epoch,
        membership_hash: transcript.membership_hash,
        cutoff: transcript.cutoff,
        package_count: transcript.packages.len() as u32,
        transcript_hash: transcript.transcript_hash,
        blob_ref,
    }
}
