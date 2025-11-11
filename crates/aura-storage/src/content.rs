//! Content Addressing and Chunk Management
//!
//! This module provides content addressing, chunk storage, and content management
//! for the Aura storage layer with capability-based access control.

use aura_core::{AccountId, AuraResult, ChunkId, ContentId, DeviceId, Hash32};
use aura_wot::{Capability, StoragePermission};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Content store managing addressed content
#[derive(Debug, Clone)]
pub struct ContentStore {
    /// Content metadata by ID
    content_metadata: HashMap<ContentId, ContentMetadata>,
    /// Content chunks by content ID
    content_chunks: HashMap<ContentId, Vec<ChunkId>>,
    /// Chunk store
    chunk_store: ChunkStore,
}

/// Chunk store managing raw chunks
#[derive(Debug, Clone)]
pub struct ChunkStore {
    /// Chunk data by ID
    chunks: HashMap<ChunkId, ChunkData>,
    /// Chunk metadata
    chunk_metadata: HashMap<ChunkId, ChunkMetadata>,
    /// Reference counts for garbage collection
    reference_counts: HashMap<ChunkId, u32>,
}

/// Content metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentMetadata {
    /// Content identifier
    pub content_id: ContentId,
    /// Content owner
    pub owner: AccountId,
    /// Content type/MIME type
    pub content_type: String,
    /// Total content size in bytes
    pub size: u64,
    /// Creation timestamp
    pub created_at: u64,
    /// Last modification timestamp
    pub modified_at: u64,
    /// Access capabilities required
    pub required_capabilities: Vec<StoragePermission>,
    /// Content tags for search
    pub tags: Vec<String>,
    /// Custom metadata
    pub custom_metadata: HashMap<String, String>,
}

/// Chunk metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Chunk identifier (content-addressed hash)
    pub chunk_id: ChunkId,
    /// Chunk size in bytes
    pub size: u32,
    /// Encryption info if encrypted
    pub encryption_info: Option<EncryptionInfo>,
    /// Compression info if compressed
    pub compression_info: Option<CompressionInfo>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last access timestamp
    pub accessed_at: u64,
}

/// Chunk data container
#[derive(Debug, Clone)]
pub struct ChunkData {
    /// Raw chunk bytes
    pub data: Vec<u8>,
    /// Content hash for verification
    pub content_hash: Hash32,
}

/// Encryption information for chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionInfo {
    /// Encryption algorithm used
    pub algorithm: EncryptionAlgorithm,
    /// Key derivation parameters
    pub key_params: Vec<u8>,
    /// Initialization vector/nonce
    pub iv: Vec<u8>,
}

/// Supported encryption algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EncryptionAlgorithm {
    /// ChaCha20-Poly1305 AEAD
    ChaCha20Poly1305,
    /// AES-256-GCM
    Aes256Gcm,
}

/// Compression information for chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionInfo {
    /// Compression algorithm used
    pub algorithm: CompressionAlgorithm,
    /// Original uncompressed size
    pub original_size: u32,
    /// Compression parameters
    pub params: Vec<u8>,
}

/// Supported compression algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// DEFLATE compression
    Deflate,
    /// LZ4 compression
    Lz4,
    /// Zstd compression
    Zstd,
}

/// Content addressing utilities
pub struct ContentAddressing;

/// Content operations request
#[derive(Debug, Clone)]
pub struct ContentRequest {
    /// Requesting device
    pub device_id: DeviceId,
    /// Target content
    pub content_id: ContentId,
    /// Presented capabilities
    pub capabilities: Vec<Capability>,
}

/// Chunk operations request
#[derive(Debug, Clone)]
pub struct ChunkRequest {
    /// Requesting device
    pub device_id: DeviceId,
    /// Target chunk
    pub chunk_id: ChunkId,
    /// Presented capabilities
    pub capabilities: Vec<Capability>,
}

impl ContentStore {
    /// Create new content store
    pub fn new() -> Self {
        Self {
            content_metadata: HashMap::new(),
            content_chunks: HashMap::new(),
            chunk_store: ChunkStore::new(),
        }
    }

    /// Store content with capability verification
    pub async fn store_content(
        &mut self,
        content_data: Vec<u8>,
        metadata: ContentMetadata,
        capabilities: Vec<Capability>,
    ) -> AuraResult<ContentId> {
        // Verify storage capabilities
        self.verify_storage_capabilities(&capabilities, &metadata)?;

        // Split content into chunks
        let chunks = self.chunk_content(content_data).await?;
        let chunk_ids: Vec<ChunkId> = chunks
            .iter()
            .map(|c| ChunkId::from(c.content_hash))
            .collect();

        // Store chunks in chunk store
        for chunk in chunks {
            self.chunk_store.store_chunk(chunk).await?;
        }

        // Store content metadata and chunk mapping
        let content_id = metadata.content_id.clone();
        self.content_metadata.insert(content_id.clone(), metadata);
        self.content_chunks.insert(content_id.clone(), chunk_ids);

        Ok(content_id)
    }

    /// Retrieve content with capability verification
    pub async fn retrieve_content(&self, request: &ContentRequest) -> AuraResult<Option<Vec<u8>>> {
        // Get content metadata
        let metadata = self
            .content_metadata
            .get(&request.content_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Content not found"))?;

        // Verify read capabilities
        self.verify_read_capabilities(&request.capabilities, metadata)?;

        // Get chunk IDs for content
        let chunk_ids = self
            .content_chunks
            .get(&request.content_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Content chunks not found"))?;

        // Retrieve and assemble chunks
        let mut content_data = Vec::new();
        for chunk_id in chunk_ids {
            let chunk_request = ChunkRequest {
                device_id: request.device_id,
                chunk_id: chunk_id.clone(),
                capabilities: request.capabilities.clone(),
            };

            if let Some(chunk_data) = self.chunk_store.retrieve_chunk(&chunk_request).await? {
                content_data.extend(chunk_data.data);
            } else {
                return Err(aura_core::AuraError::storage("Missing chunk"));
            }
        }

        Ok(Some(content_data))
    }

    /// Delete content with capability verification
    pub async fn delete_content(&mut self, request: &ContentRequest) -> AuraResult<()> {
        // Get content metadata
        let metadata = self
            .content_metadata
            .get(&request.content_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Content not found"))?;

        // Verify delete capabilities
        self.verify_delete_capabilities(&request.capabilities, metadata)?;

        // Get chunk IDs
        let chunk_ids = self
            .content_chunks
            .remove(&request.content_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Content chunks not found"))?;

        // Decrement chunk reference counts
        for chunk_id in chunk_ids {
            self.chunk_store.decrement_reference(&chunk_id).await?;
        }

        // Remove content metadata
        self.content_metadata.remove(&request.content_id);

        Ok(())
    }

    /// List content accessible to device
    pub async fn list_accessible_content(
        &self,
        device_id: DeviceId,
        capabilities: &[Capability],
        filters: &ContentListFilters,
    ) -> AuraResult<Vec<ContentMetadata>> {
        let mut accessible_content = Vec::new();

        for (content_id, metadata) in &self.content_metadata {
            // Check if device has read access
            if self
                .verify_read_capabilities(capabilities, metadata)
                .is_ok()
            {
                // Apply filters
                if self.apply_content_filters(metadata, filters) {
                    accessible_content.push(metadata.clone());
                }
            }
        }

        // Sort by modification time (newest first)
        accessible_content.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

        // Apply limit if specified
        if let Some(limit) = filters.limit {
            accessible_content.truncate(limit);
        }

        Ok(accessible_content)
    }

    /// Search content by tags and metadata
    pub async fn search_content(
        &self,
        device_id: DeviceId,
        capabilities: &[Capability],
        query: &ContentSearchQuery,
    ) -> AuraResult<Vec<ContentMetadata>> {
        let mut matching_content = Vec::new();

        for (content_id, metadata) in &self.content_metadata {
            // Check access permissions
            if self
                .verify_read_capabilities(capabilities, metadata)
                .is_err()
            {
                continue;
            }

            // Check if content matches query
            if self.matches_search_query(metadata, query) {
                matching_content.push(metadata.clone());
            }
        }

        // Sort by relevance (TODO fix - For now, just by modification time)
        matching_content.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));

        // Apply limit
        matching_content.truncate(query.limit.unwrap_or(100));

        Ok(matching_content)
    }

    /// Split content into chunks for storage
    async fn chunk_content(&self, content_data: Vec<u8>) -> AuraResult<Vec<ChunkData>> {
        const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks
        let mut chunks = Vec::new();

        for (i, chunk_bytes) in content_data.chunks(CHUNK_SIZE).enumerate() {
            let content_hash = ContentAddressing::hash_content(chunk_bytes)?;
            let chunk_id = ChunkId::from(content_hash);

            chunks.push(ChunkData {
                data: chunk_bytes.to_vec(),
                content_hash,
            });
        }

        Ok(chunks)
    }

    /// Verify storage capabilities
    fn verify_storage_capabilities(
        &self,
        capabilities: &[Capability],
        metadata: &ContentMetadata,
    ) -> AuraResult<()> {
        // Check if capabilities allow content storage
        for required_perm in &metadata.required_capabilities {
            let has_permission = capabilities
                .iter()
                .any(|cap| cap.grants_storage_permission(required_perm));

            if !has_permission {
                return Err(aura_core::AuraError::permission_denied(format!(
                    "Missing capability for {:?}",
                    required_perm
                )));
            }
        }

        Ok(())
    }

    /// Verify read capabilities
    fn verify_read_capabilities(
        &self,
        capabilities: &[Capability],
        metadata: &ContentMetadata,
    ) -> AuraResult<()> {
        // Check read permission
        let has_read = capabilities
            .iter()
            .any(|cap| cap.grants_storage_permission(&StoragePermission::ContentRead));

        if !has_read {
            return Err(aura_core::AuraError::permission_denied(
                "No read permission",
            ));
        }

        // Check specific content capabilities if required
        for required_perm in &metadata.required_capabilities {
            let has_permission = capabilities
                .iter()
                .any(|cap| cap.grants_storage_permission(required_perm));

            if !has_permission {
                return Err(aura_core::AuraError::permission_denied(format!(
                    "Missing specific capability: {:?}",
                    required_perm
                )));
            }
        }

        Ok(())
    }

    /// Verify delete capabilities
    fn verify_delete_capabilities(
        &self,
        capabilities: &[Capability],
        metadata: &ContentMetadata,
    ) -> AuraResult<()> {
        // Check delete permission
        let has_delete = capabilities
            .iter()
            .any(|cap| cap.grants_storage_permission(&StoragePermission::ContentDelete));

        if !has_delete {
            return Err(aura_core::AuraError::permission_denied(
                "No delete permission",
            ));
        }

        Ok(())
    }

    /// Apply content list filters
    fn apply_content_filters(
        &self,
        metadata: &ContentMetadata,
        filters: &ContentListFilters,
    ) -> bool {
        // Filter by content type
        if let Some(ref content_types) = filters.content_types {
            if !content_types.contains(&metadata.content_type) {
                return false;
            }
        }

        // Filter by owner
        if let Some(ref owners) = filters.owners {
            if !owners.contains(&metadata.owner) {
                return false;
            }
        }

        // Filter by tags
        if let Some(ref required_tags) = filters.required_tags {
            for tag in required_tags {
                if !metadata.tags.contains(tag) {
                    return false;
                }
            }
        }

        // Filter by size range
        if let Some(min_size) = filters.min_size {
            if metadata.size < min_size {
                return false;
            }
        }
        if let Some(max_size) = filters.max_size {
            if metadata.size > max_size {
                return false;
            }
        }

        // Filter by date range
        if let Some(after) = filters.created_after {
            if metadata.created_at < after {
                return false;
            }
        }
        if let Some(before) = filters.created_before {
            if metadata.created_at > before {
                return false;
            }
        }

        true
    }

    /// Check if content matches search query
    fn matches_search_query(&self, metadata: &ContentMetadata, query: &ContentSearchQuery) -> bool {
        // Search in tags
        for term in &query.terms {
            let term_lower = term.to_lowercase();

            // Check tags
            let matches_tags = metadata
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains(&term_lower));

            // Check custom metadata values
            let matches_metadata = metadata
                .custom_metadata
                .values()
                .any(|value| value.to_lowercase().contains(&term_lower));

            if matches_tags || matches_metadata {
                return true;
            }
        }

        false
    }
}

impl ChunkStore {
    /// Create new chunk store
    pub fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            chunk_metadata: HashMap::new(),
            reference_counts: HashMap::new(),
        }
    }

    /// Store chunk with reference counting
    pub async fn store_chunk(&mut self, chunk_data: ChunkData) -> AuraResult<ChunkId> {
        let chunk_id = ChunkId::from(chunk_data.content_hash);

        // Create metadata
        let metadata = ChunkMetadata {
            chunk_id: chunk_id.clone(),
            size: chunk_data.data.len() as u32,
            encryption_info: None,  // TODO: Add encryption support
            compression_info: None, // TODO: Add compression support
            created_at: self.get_current_timestamp(),
            accessed_at: self.get_current_timestamp(),
        };

        // Store chunk data and metadata
        self.chunks.insert(chunk_id.clone(), chunk_data);
        self.chunk_metadata.insert(chunk_id.clone(), metadata);

        // Increment reference count
        *self.reference_counts.entry(chunk_id.clone()).or_insert(0) += 1;

        Ok(chunk_id)
    }

    /// Retrieve chunk data
    pub async fn retrieve_chunk(&self, request: &ChunkRequest) -> AuraResult<Option<ChunkData>> {
        if let Some(chunk_data) = self.chunks.get(&request.chunk_id) {
            // Verify content hash
            let computed_hash = ContentAddressing::hash_content(&chunk_data.data)?;
            if computed_hash != chunk_data.content_hash {
                return Err(aura_core::AuraError::storage("Chunk hash mismatch"));
            }

            Ok(Some(chunk_data.clone()))
        } else {
            Ok(None)
        }
    }

    /// Decrement reference count for chunk
    pub async fn decrement_reference(&mut self, chunk_id: &ChunkId) -> AuraResult<()> {
        if let Some(count) = self.reference_counts.get_mut(chunk_id) {
            if *count > 0 {
                *count -= 1;

                // If reference count reaches zero, chunk becomes eligible for GC
                if *count == 0 {
                    // Mark for garbage collection but don't delete immediately
                    // Actual deletion happens during GC sweep
                }
            }
        }
        Ok(())
    }

    /// Get chunks eligible for garbage collection
    pub fn get_gc_eligible_chunks(&self) -> Vec<ChunkId> {
        self.reference_counts
            .iter()
            .filter_map(|(chunk_id, &count)| {
                if count == 0 {
                    Some(chunk_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get current timestamp
    fn get_current_timestamp(&self) -> u64 {
        // Would use time effects in real implementation
        1234567890
    }
}

impl ContentAddressing {
    /// Compute content hash for addressing
    pub fn hash_content(content: &[u8]) -> AuraResult<Hash32> {
        // Use Blake3 for content addressing
        Ok(Hash32::from_bytes(content))
    }

    /// Verify content integrity
    pub fn verify_content(content: &[u8], expected_hash: Hash32) -> AuraResult<bool> {
        let computed_hash = Self::hash_content(content)?;
        Ok(computed_hash == expected_hash)
    }
}

/// Content list filters
#[derive(Debug, Clone, Default)]
pub struct ContentListFilters {
    /// Filter by content types
    pub content_types: Option<Vec<String>>,
    /// Filter by owners
    pub owners: Option<Vec<AccountId>>,
    /// Filter by required tags
    pub required_tags: Option<Vec<String>>,
    /// Minimum content size
    pub min_size: Option<u64>,
    /// Maximum content size
    pub max_size: Option<u64>,
    /// Created after timestamp
    pub created_after: Option<u64>,
    /// Created before timestamp
    pub created_before: Option<u64>,
    /// Maximum results to return
    pub limit: Option<usize>,
}

/// Content search query
#[derive(Debug, Clone)]
pub struct ContentSearchQuery {
    /// Search terms
    pub terms: Vec<String>,
    /// Content type filters
    pub content_types: Option<Vec<String>>,
    /// Owner filters
    pub owners: Option<Vec<AccountId>>,
    /// Tag filters
    pub tags: Option<Vec<String>>,
    /// Maximum results
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_metadata_creation() {
        let metadata = ContentMetadata {
            content_id: ContentId::new(Hash32([0u8; 32])),
            owner: AccountId::new(),
            content_type: "text/plain".into(),
            size: 1024,
            created_at: 1234567890,
            modified_at: 1234567890,
            required_capabilities: vec![StoragePermission::ContentRead],
            tags: vec!["document".into(), "text".into()],
            custom_metadata: HashMap::new(),
        };

        assert_eq!(metadata.content_type, "text/plain");
        assert_eq!(metadata.size, 1024);
        assert_eq!(metadata.tags.len(), 2);
    }

    #[test]
    fn test_chunk_metadata_creation() {
        let metadata = ChunkMetadata {
            chunk_id: ChunkId::new(Hash32([0u8; 32])),
            size: 512,
            encryption_info: None,
            compression_info: None,
            created_at: 1234567890,
            accessed_at: 1234567890,
        };

        assert_eq!(metadata.size, 512);
        assert!(metadata.encryption_info.is_none());
    }

    #[test]
    fn test_content_search_query() {
        let query = ContentSearchQuery {
            terms: vec!["test".into(), "document".into()],
            content_types: Some(vec!["text/plain".into()]),
            owners: None,
            tags: Some(vec!["important".into()]),
            limit: Some(10),
        };

        assert_eq!(query.terms.len(), 2);
        assert_eq!(query.limit, Some(10));
    }
}
