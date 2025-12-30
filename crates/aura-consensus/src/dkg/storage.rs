//! Transcript storage interface for DKG payloads.

use super::transcript::compute_transcript_hash_from_transcript;
use super::types::DkgTranscript;
use async_lock::Mutex;
use async_trait::async_trait;
use aura_core::{
    effects::StorageEffects,
    util::serialization::{from_slice, to_vec},
    AuraError, Hash32, Result,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Storage interface for DKG transcripts (blob or journal reference).
#[async_trait]
pub trait DkgTranscriptStore: Send + Sync {
    /// Persist a transcript and return an optional blob reference.
    async fn put(&self, transcript: &DkgTranscript) -> Result<Option<Hash32>>;
    /// Load a transcript by reference.
    async fn get(&self, reference: &Hash32) -> Result<DkgTranscript>;
}

/// Default in-memory store (intended for tests).
pub struct MemoryTranscriptStore {
    transcripts: Mutex<HashMap<Hash32, DkgTranscript>>,
}

impl Default for MemoryTranscriptStore {
    fn default() -> Self {
        Self {
            transcripts: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl DkgTranscriptStore for MemoryTranscriptStore {
    async fn put(&self, transcript: &DkgTranscript) -> Result<Option<Hash32>> {
        let reference = transcript.transcript_hash;
        let mut guard = self.transcripts.lock().await;
        guard.insert(reference, transcript.clone());
        Ok(Some(reference))
    }

    async fn get(&self, reference: &Hash32) -> Result<DkgTranscript> {
        let guard = self.transcripts.lock().await;
        let transcript = guard
            .get(reference)
            .cloned()
            .ok_or_else(|| aura_core::AuraError::not_found("transcript not found"))?;
        let computed = compute_transcript_hash_from_transcript(&transcript)?;
        if computed != transcript.transcript_hash {
            return Err(aura_core::AuraError::invalid("transcript hash mismatch"));
        }
        Ok(transcript)
    }
}

/// Storage-backed transcript store using StorageEffects.
pub struct StorageTranscriptStore<S: StorageEffects + ?Sized> {
    storage: Arc<S>,
    prefix: String,
}

impl<S: StorageEffects + ?Sized> StorageTranscriptStore<S> {
    pub fn new_default(storage: Arc<S>) -> Self {
        Self::new(storage, "dkg/transcripts")
    }

    pub fn new(storage: Arc<S>, prefix: impl Into<String>) -> Self {
        Self {
            storage,
            prefix: prefix.into(),
        }
    }

    fn key_for(&self, reference: &Hash32) -> String {
        format!("{}/{}", self.prefix, reference.to_hex())
    }
}

#[async_trait]
impl<S: StorageEffects + ?Sized> DkgTranscriptStore for StorageTranscriptStore<S> {
    async fn put(&self, transcript: &DkgTranscript) -> Result<Option<Hash32>> {
        let reference = transcript.transcript_hash;
        let key = self.key_for(&reference);
        let bytes = to_vec(transcript).map_err(|e| AuraError::serialization(e.to_string()))?;
        self.storage
            .store(&key, bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;
        Ok(Some(reference))
    }

    async fn get(&self, reference: &Hash32) -> Result<DkgTranscript> {
        let key = self.key_for(reference);
        let blob = self
            .storage
            .retrieve(&key)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?
            .ok_or_else(|| AuraError::not_found("transcript not found"))?;
        let transcript = from_slice(&blob).map_err(|e| AuraError::serialization(e.to_string()))?;
        let computed = compute_transcript_hash_from_transcript(&transcript)?;
        if computed != transcript.transcript_hash {
            return Err(AuraError::invalid("transcript hash mismatch"));
        }
        Ok(transcript)
    }
}
