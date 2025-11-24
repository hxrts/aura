//! Content addressing and chunk management types
//!
//! This module defines pure types and functions for content addressing,
//! chunking, and manifest creation in Aura's storage system.

use aura_core::{ChunkId, ContentId, ContentSize};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::StorageCapability;
use aura_core::AuraError;

/// Configuration for erasure coding
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErasureConfig {
    /// Data chunks (k)
    pub data_chunks: u8,
    /// Parity chunks (m)
    pub parity_chunks: u8,
    /// Maximum chunk size in bytes
    pub max_chunk_size: u32,
}

impl ErasureConfig {
    /// Create a new erasure config
    pub fn new(data_chunks: u8, parity_chunks: u8, max_chunk_size: u32) -> Self {
        Self {
            data_chunks,
            parity_chunks,
            max_chunk_size,
        }
    }

    /// Total number of chunks (data + parity)
    pub fn total_chunks(&self) -> u8 {
        self.data_chunks + self.parity_chunks
    }

    /// Minimum chunks needed for recovery
    pub fn min_chunks(&self) -> u8 {
        self.data_chunks
    }
}

impl Default for ErasureConfig {
    fn default() -> Self {
        Self::new(3, 2, 1024 * 1024) // 3+2 erasure, 1MB max chunk size
    }
}

/// Layout of content chunks for storage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkLayout {
    /// Ordered list of chunk IDs
    pub chunks: Vec<ChunkId>,
    /// Size of each chunk
    pub chunk_sizes: Vec<u32>,
    /// Total content size
    pub total_size: ContentSize,
    /// Erasure coding configuration
    pub erasure_config: ErasureConfig,
}

impl ChunkLayout {
    /// Create a new chunk layout
    pub fn new(
        chunks: Vec<ChunkId>,
        chunk_sizes: Vec<u32>,
        total_size: ContentSize,
        erasure_config: ErasureConfig,
    ) -> Result<Self, AuraError> {
        if chunks.len() != chunk_sizes.len() {
            return Err(AuraError::invalid("Chunk count mismatch with sizes"));
        }

        // Note: total_size represents the original content size (data only)
        // chunk_sizes includes both data chunks and parity chunks
        // So sum(chunk_sizes) will be >= total_size when parity chunks are included
        let computed_total: u64 = chunk_sizes.iter().map(|&size| size as u64).sum();
        if computed_total < total_size.0 {
            return Err(AuraError::invalid(
                "Total chunk sizes less than content size",
            ));
        }

        Ok(Self {
            chunks,
            chunk_sizes,
            total_size,
            erasure_config,
        })
    }

    /// Number of chunks in this layout
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Get chunk ID by index
    pub fn get_chunk(&self, index: usize) -> Option<&ChunkId> {
        self.chunks.get(index)
    }

    /// Get chunk size by index
    pub fn get_chunk_size(&self, index: usize) -> Option<u32> {
        self.chunk_sizes.get(index).copied()
    }
}

/// Manifest for a single chunk with metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkManifest {
    /// Chunk identifier
    pub chunk_id: ChunkId,
    /// Chunk size in bytes
    pub size: u32,
    /// Required capabilities for access
    pub required_capabilities: Vec<StorageCapability>,
    /// Chunk creation timestamp (Unix timestamp)
    pub created_at: u64,
    /// Optional metadata
    pub metadata: BTreeMap<String, String>,
}

impl ChunkManifest {
    /// Create a new chunk manifest
    pub fn new(
        chunk_id: ChunkId,
        size: u32,
        required_capabilities: Vec<StorageCapability>,
    ) -> Self {
        Self {
            chunk_id,
            size,
            required_capabilities,
            // TODO: Replace with PhysicalTimeEffects from context
            // Using placeholder to avoid violating effect system architecture
            created_at: 0, // Will be replaced with proper time from effect context
            metadata: BTreeMap::new(),
        }
    }

    /// Add metadata to the manifest
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Check if provided capabilities satisfy requirements
    pub fn verify_access(&self, provided_caps: &[StorageCapability]) -> bool {
        self.required_capabilities
            .iter()
            .all(|required| provided_caps.contains(required))
    }
}

/// Manifest for content composed of multiple chunks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentManifest {
    /// Content identifier
    pub content_id: ContentId,
    /// Chunk layout for this content
    pub layout: ChunkLayout,
    /// Per-chunk manifests
    pub chunk_manifests: Vec<ChunkManifest>,
    /// Content-level metadata
    pub metadata: BTreeMap<String, String>,
    /// Content creation timestamp
    pub created_at: u64,
}

impl ContentManifest {
    /// Create a new content manifest
    pub fn new(
        content_id: ContentId,
        layout: ChunkLayout,
        chunk_manifests: Vec<ChunkManifest>,
    ) -> Result<Self, AuraError> {
        // Verify chunk manifests match layout
        if layout.chunk_count() != chunk_manifests.len() {
            return Err(AuraError::invalid("Chunk manifest count mismatch"));
        }

        for (i, manifest) in chunk_manifests.iter().enumerate() {
            if let Some(expected_id) = layout.get_chunk(i) {
                if &manifest.chunk_id != expected_id {
                    return Err(AuraError::invalid(format!(
                        "Chunk ID mismatch at index {}",
                        i
                    )));
                }
            }
        }

        Ok(Self {
            content_id,
            layout,
            chunk_manifests,
            metadata: BTreeMap::new(),
            // TODO: Replace with PhysicalTimeEffects from context
            // Using placeholder to avoid violating effect system architecture
            created_at: 0, // Will be replaced with proper time from effect context
        })
    }

    /// Add metadata to the content manifest
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get manifest for a specific chunk
    pub fn get_chunk_manifest(&self, chunk_id: &ChunkId) -> Option<&ChunkManifest> {
        self.chunk_manifests
            .iter()
            .find(|manifest| &manifest.chunk_id == chunk_id)
    }

    /// Get all required capabilities for accessing this content
    pub fn get_required_capabilities(&self) -> Vec<StorageCapability> {
        let mut caps = Vec::new();
        for manifest in &self.chunk_manifests {
            caps.extend_from_slice(&manifest.required_capabilities);
        }
        caps.sort();
        caps.dedup();
        caps
    }

    /// Verify if provided capabilities can access this content
    pub fn verify_content_access(&self, provided_caps: &[StorageCapability]) -> bool {
        self.chunk_manifests
            .iter()
            .all(|manifest| manifest.verify_access(provided_caps))
    }
}

/// Pure function to compute chunk layout from content
pub fn compute_chunk_layout(
    content: &[u8],
    erasure_config: ErasureConfig,
) -> Result<ChunkLayout, AuraError> {
    if content.is_empty() {
        return Err(AuraError::invalid("Empty content"));
    }

    let chunk_size = erasure_config.max_chunk_size as usize;
    let mut chunks = Vec::new();
    let mut chunk_sizes = Vec::new();

    // Split content into data chunks
    for chunk_data in content.chunks(chunk_size) {
        let chunk_id = ChunkId::from_bytes(chunk_data);
        chunks.push(chunk_id);
        chunk_sizes.push(chunk_data.len() as u32);
    }

    // Add parity chunks (for now, just placeholder IDs)
    // In a real implementation, this would compute actual parity data
    for i in 0..erasure_config.parity_chunks {
        let parity_data = format!("parity_{}", i);
        let parity_id = ChunkId::from_bytes(parity_data.as_bytes());
        chunks.push(parity_id);
        chunk_sizes.push(chunk_size.min(content.len()) as u32);
    }

    ChunkLayout::new(
        chunks,
        chunk_sizes,
        ContentSize(content.len() as u64),
        erasure_config,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_erasure_config() {
        let config = ErasureConfig::new(3, 2, 1024);
        assert_eq!(config.total_chunks(), 5);
        assert_eq!(config.min_chunks(), 3);
    }

    #[test]
    fn test_chunk_layout() {
        let chunks = vec![
            ChunkId::from_bytes(b"chunk1"),
            ChunkId::from_bytes(b"chunk2"),
        ];
        let sizes = vec![100, 200];
        let total_size = ContentSize(300);
        let config = ErasureConfig::default();

        let layout = ChunkLayout::new(chunks, sizes, total_size, config).unwrap();
        assert_eq!(layout.chunk_count(), 2);
        assert_eq!(layout.get_chunk_size(0), Some(100));
        assert_eq!(layout.get_chunk_size(1), Some(200));
    }

    #[test]
    fn test_compute_chunk_layout() {
        let content = b"hello world, this is a test of chunking";
        let config = ErasureConfig::new(2, 1, 10);

        let layout = compute_chunk_layout(content, config).unwrap();

        // Should have data chunks + parity chunks
        assert!(layout.chunk_count() > 2); // At least data chunks
        assert_eq!(layout.total_size.0, content.len() as u64);
    }
}
