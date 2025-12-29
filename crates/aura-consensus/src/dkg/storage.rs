//! Transcript storage interface for DKG payloads.

use super::types::DkgTranscript;
use aura_core::{AuraError, Hash32, Result};

/// Storage interface for DKG transcripts (blob or journal reference).
pub trait DkgTranscriptStore: Send + Sync {
    /// Persist a transcript and return an optional blob reference.
    fn put(&self, transcript: &DkgTranscript) -> Result<Option<Hash32>>;
    /// Load a transcript by reference.
    fn get(&self, reference: &Hash32) -> Result<DkgTranscript>;
}

/// Default in-memory placeholder store (not for production).
#[derive(Default)]
pub struct MemoryTranscriptStore;

impl DkgTranscriptStore for MemoryTranscriptStore {
    fn put(&self, _transcript: &DkgTranscript) -> Result<Option<Hash32>> {
        Err(AuraError::invalid(
            "MemoryTranscriptStore is a placeholder; provide a real transcript store",
        ))
    }

    fn get(&self, _reference: &Hash32) -> Result<DkgTranscript> {
        Err(AuraError::invalid(
            "MemoryTranscriptStore is a placeholder; provide a real transcript store",
        ))
    }
}
