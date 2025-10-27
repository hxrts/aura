//! Storage indexer with manifest and chunk indexing
//!
//! Provides efficient storage operations with quota tracking and
//! content addressing via content identifiers (CIDs).

use crate::manifest::ObjectManifest;
use crate::{Result, StorageError, StoreErrorBuilder};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Cid = Vec<u8>;

/// Storage indexer
///
/// Provides indexing of manifests and chunks with quota enforcement
pub struct Indexer {
    manifests: BTreeMap<Vec<u8>, ObjectManifest>,
    chunks: BTreeMap<Vec<u8>, Vec<u8>>,
    quota_limit: u64,
    current_usage: u64,
}

impl Indexer {
    /// Create a new indexer
    pub fn new(quota_limit: u64) -> Self {
        Indexer {
            manifests: BTreeMap::new(),
            chunks: BTreeMap::new(),
            quota_limit,
            current_usage: 0,
        }
    }

    /// Store manifest
    pub fn store_manifest(&mut self, cid: &Cid, manifest: &ObjectManifest) -> Result<()> {
        if self.manifests.contains_key(cid) {
            return Ok(()); // Already stored
        }
        self.manifests.insert(cid.clone(), manifest.clone());
        Ok(())
    }

    /// Load manifest
    pub fn load_manifest(&self, cid: &Cid) -> Result<ObjectManifest> {
        self.manifests
            .get(cid)
            .cloned()
            .ok_or_else(|| StoreErrorBuilder::not_found(hex::encode(cid)))
    }

    /// Store chunk
    pub fn store_chunk(&mut self, chunk_id: &Cid, data: &[u8]) -> Result<()> {
        let size = data.len() as u64;

        // Check quota
        if self.current_usage + size > self.quota_limit {
            return Err(StoreErrorBuilder::quota_exceeded(
                self.current_usage + size,
                self.quota_limit,
            ));
        }

        if !self.chunks.contains_key(chunk_id) {
            self.chunks.insert(chunk_id.clone(), data.to_vec());
            self.current_usage += size;
        }

        Ok(())
    }

    /// Load chunk
    pub fn load_chunk(&self, chunk_id: &Cid) -> Result<Vec<u8>> {
        self.chunks
            .get(chunk_id)
            .cloned()
            .ok_or_else(|| StoreErrorBuilder::not_found(hex::encode(chunk_id)))
    }

    /// Get quota usage
    pub fn get_quota_usage(&self) -> u64 {
        self.current_usage
    }

    /// Get remaining quota
    pub fn get_remaining_quota(&self) -> u64 {
        self.quota_limit.saturating_sub(self.current_usage)
    }
}

/// Put options for storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutOpts {
    pub context_id: Option<[u8; 32]>,
    pub app_metadata: Option<Vec<u8>>,
}

impl Default for PutOpts {
    fn default() -> Self {
        PutOpts {
            context_id: None,
            app_metadata: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indexer_basic() {
        let mut indexer = Indexer::new(1_000_000);

        let chunk_id = vec![1u8; 32];
        let data = vec![42u8; 1000];

        indexer.store_chunk(&chunk_id, &data).unwrap();
        assert_eq!(indexer.get_quota_usage(), 1000);

        let retrieved = indexer.load_chunk(&chunk_id).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn test_quota_exceeded() {
        let mut indexer = Indexer::new(100);

        let chunk_id = vec![1u8; 32];
        let data = vec![42u8; 1000];

        let result = indexer.store_chunk(&chunk_id, &data);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_chunks() {
        let mut indexer = Indexer::new(10_000);

        for i in 0..5 {
            let chunk_id = vec![i as u8; 32];
            let data = vec![i as u8; 1000];
            indexer.store_chunk(&chunk_id, &data).unwrap();
        }

        assert_eq!(indexer.get_quota_usage(), 5000);
    }
}
