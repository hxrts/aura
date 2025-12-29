//! Transcript accumulation and finalization (BFT-DKG).

use super::types::{DealerPackage, DkgConfig, DkgTranscript};
use aura_core::{
    hash,
    util::serialization::to_vec,
    AuraError, ContextId, Hash32, Result,
};
use aura_journal::fact::DkgTranscriptCommit;

#[derive(serde::Serialize)]
struct TranscriptDigest<'a> {
    epoch: u64,
    membership_hash: Hash32,
    cutoff: u64,
    prestate_hash: Hash32,
    operation_hash: Hash32,
    participants: &'a [aura_core::AuthorityId],
    packages: &'a [DealerPackage],
}

pub fn compute_transcript_hash(
    config: &DkgConfig,
    packages: &[DealerPackage],
) -> Result<Hash32> {
    let digest = TranscriptDigest {
        epoch: config.epoch,
        membership_hash: config.membership_hash,
        cutoff: config.cutoff,
        prestate_hash: config.prestate_hash,
        operation_hash: config.operation_hash,
        participants: &config.participants,
        packages,
    };
    let encoded =
        to_vec(&digest).map_err(|e| AuraError::serialization(e.to_string()))?;
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_DKG_TRANSCRIPT");
    hasher.update(&encoded);
    Ok(Hash32(hasher.finalize()))
}

pub fn finalize_transcript(config: &DkgConfig, packages: Vec<DealerPackage>) -> Result<DkgTranscript> {
    let transcript_hash = compute_transcript_hash(config, &packages)?;
    Ok(DkgTranscript {
        epoch: config.epoch,
        membership_hash: config.membership_hash,
        cutoff: config.cutoff,
        prestate_hash: config.prestate_hash,
        operation_hash: config.operation_hash,
        participants: config.participants.clone(),
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
