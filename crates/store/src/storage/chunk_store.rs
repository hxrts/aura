//! Chunk Storage and Encryption
//!
//! Implements content chunking, encryption using device-derived keys,
//! and local storage with content-addressing via BLAKE3.
//!
//! Reference: docs/040_storage.md Section 3

use crate::manifest::{ChunkingParams, Cid, KeyDerivationSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

// Import shared types from aura-types
pub use aura_types::ChunkId;

// ChunkId methods are now defined in aura-types crate

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedChunk {
    pub chunk_id: ChunkId,
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 24],
    pub size: u64,
}

impl EncryptedChunk {
    pub fn new(chunk_id: ChunkId, ciphertext: Vec<u8>, nonce: [u8; 24]) -> Self {
        let size = ciphertext.len() as u64;
        Self {
            chunk_id,
            ciphertext,
            nonce,
            size,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub chunk_id: ChunkId,
    pub manifest_cid: Cid,
    pub chunk_index: u32,
    pub size: u64,
    pub stored_at: u64,
}

pub struct ChunkStore {
    storage_path: PathBuf,
    chunk_cache: BTreeMap<ChunkId, EncryptedChunk>,
    metadata_index: BTreeMap<ChunkId, ChunkMetadata>,
    manifest_chunks: BTreeMap<Cid, Vec<ChunkId>>,
}

impl ChunkStore {
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            storage_path,
            chunk_cache: BTreeMap::new(),
            metadata_index: BTreeMap::new(),
            manifest_chunks: BTreeMap::new(),
        }
    }

    pub fn chunk_content(
        &self,
        content: &[u8],
        params: &ChunkingParams,
    ) -> Result<Vec<Vec<u8>>, ChunkError> {
        if content.is_empty() {
            return Err(ChunkError::EmptyContent);
        }

        let chunk_size = params.chunk_size as usize;
        let mut chunks = Vec::new();

        for chunk_data in content.chunks(chunk_size) {
            chunks.push(chunk_data.to_vec());
        }

        Ok(chunks)
    }

    pub fn encrypt_chunk(
        &self,
        chunk_data: &[u8],
        key_spec: &KeyDerivationSpec,
    ) -> Result<EncryptedChunk, ChunkError> {
        let encryption_key = self.derive_encryption_key(key_spec)?;

        let nonce = self.generate_nonce(chunk_data);

        let ciphertext =
            self.encrypt_with_xchacha20poly1305(chunk_data, &encryption_key, &nonce)?;

        let chunk_id = ChunkId::from_content(chunk_data);

        Ok(EncryptedChunk::new(chunk_id, ciphertext, nonce))
    }

    pub fn decrypt_chunk(
        &self,
        encrypted_chunk: &EncryptedChunk,
        key_spec: &KeyDerivationSpec,
    ) -> Result<Vec<u8>, ChunkError> {
        let encryption_key = self.derive_encryption_key(key_spec)?;

        let plaintext = self.decrypt_with_xchacha20poly1305(
            &encrypted_chunk.ciphertext,
            &encryption_key,
            &encrypted_chunk.nonce,
        )?;

        let expected_chunk_id = ChunkId::from_content(&plaintext);
        if expected_chunk_id != encrypted_chunk.chunk_id {
            return Err(ChunkError::IntegrityCheckFailed);
        }

        Ok(plaintext)
    }

    pub fn store_chunk(
        &mut self,
        manifest_cid: &Cid,
        chunk_index: u32,
        encrypted_chunk: EncryptedChunk,
        timestamp: u64,
    ) -> Result<ChunkId, ChunkError> {
        let chunk_id = encrypted_chunk.chunk_id.clone();

        let metadata = ChunkMetadata {
            chunk_id: chunk_id.clone(),
            manifest_cid: manifest_cid.clone(),
            chunk_index,
            size: encrypted_chunk.size,
            stored_at: timestamp,
        };

        self.chunk_cache.insert(chunk_id.clone(), encrypted_chunk);
        self.metadata_index.insert(chunk_id.clone(), metadata);

        self.manifest_chunks
            .entry(manifest_cid.clone())
            .or_insert_with(Vec::new)
            .push(chunk_id.clone());

        Ok(chunk_id)
    }

    pub fn retrieve_chunk(&self, chunk_id: &ChunkId) -> Result<&EncryptedChunk, ChunkError> {
        self.chunk_cache
            .get(chunk_id)
            .ok_or(ChunkError::ChunkNotFound)
    }

    pub fn get_manifest_chunks(&self, manifest_cid: &Cid) -> Option<&Vec<ChunkId>> {
        self.manifest_chunks.get(manifest_cid)
    }

    pub fn delete_chunk(&mut self, chunk_id: &ChunkId) -> Result<(), ChunkError> {
        if let Some(metadata) = self.metadata_index.get(chunk_id) {
            let manifest_cid = metadata.manifest_cid.clone();

            if let Some(chunks) = self.manifest_chunks.get_mut(&manifest_cid) {
                chunks.retain(|id| id != chunk_id);
                if chunks.is_empty() {
                    self.manifest_chunks.remove(&manifest_cid);
                }
            }
        }

        self.chunk_cache.remove(chunk_id);
        self.metadata_index.remove(chunk_id);

        Ok(())
    }

    pub fn get_chunk_metadata(&self, chunk_id: &ChunkId) -> Option<&ChunkMetadata> {
        self.metadata_index.get(chunk_id)
    }

    pub fn get_storage_stats(&self) -> StorageStats {
        let total_chunks = self.chunk_cache.len();
        let total_size: u64 = self.chunk_cache.values().map(|c| c.size).sum();
        let unique_manifests = self.manifest_chunks.len();

        StorageStats {
            total_chunks,
            total_size,
            unique_manifests,
        }
    }

    fn derive_encryption_key(&self, key_spec: &KeyDerivationSpec) -> Result<[u8; 32], ChunkError> {
        let mut hasher = aura_crypto::blake3_hasher();

        hasher.update(b"encryption-key");
        hasher.update(&key_spec.domain);
        if let Some(ctx) = &key_spec.context {
            hasher.update(ctx);
        }

        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(hash.as_bytes());
        Ok(key)
    }

    fn generate_nonce(&self, data: &[u8]) -> [u8; 24] {
        let mut hasher = aura_crypto::blake3_hasher();
        hasher.update(data);
        hasher.update(b"nonce");
        let hash = hasher.finalize();
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&hash.as_bytes()[..24]);
        nonce
    }

    fn encrypt_with_xchacha20poly1305(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 24],
    ) -> Result<Vec<u8>, ChunkError> {
        use chacha20poly1305::{
            aead::{Aead, KeyInit},
            XChaCha20Poly1305, XNonce,
        };

        let cipher = XChaCha20Poly1305::new(key.into());
        let nonce_obj = XNonce::from_slice(nonce);

        cipher
            .encrypt(nonce_obj, plaintext)
            .map_err(|_| ChunkError::EncryptionFailed)
    }

    fn decrypt_with_xchacha20poly1305(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 24],
    ) -> Result<Vec<u8>, ChunkError> {
        use chacha20poly1305::{
            aead::{Aead, KeyInit},
            XChaCha20Poly1305, XNonce,
        };

        let cipher = XChaCha20Poly1305::new(key.into());
        let nonce_obj = XNonce::from_slice(nonce);

        cipher
            .decrypt(nonce_obj, ciphertext)
            .map_err(|_| ChunkError::DecryptionFailed)
    }
}

#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_chunks: usize,
    pub total_size: u64,
    pub unique_manifests: usize,
}

#[derive(Debug, Clone)]
pub enum ChunkError {
    EmptyContent,
    EncryptionFailed,
    DecryptionFailed,
    ChunkNotFound,
    IntegrityCheckFailed,
    InvalidKeySpec,
    StorageError(String),
}

impl std::fmt::Display for ChunkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyContent => write!(f, "Cannot chunk empty content"),
            Self::EncryptionFailed => write!(f, "Chunk encryption failed"),
            Self::DecryptionFailed => write!(f, "Chunk decryption failed"),
            Self::ChunkNotFound => write!(f, "Chunk not found in storage"),
            Self::IntegrityCheckFailed => write!(f, "Chunk integrity check failed"),
            Self::InvalidKeySpec => write!(f, "Invalid key derivation specification"),
            Self::StorageError(msg) => write!(f, "Storage error: {}", msg),
        }
    }
}

impl std::error::Error for ChunkError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::KeyDerivationSpec;

    fn create_test_store() -> ChunkStore {
        ChunkStore::new(PathBuf::from("/tmp/test_store"))
    }

    fn create_test_key_spec() -> KeyDerivationSpec {
        KeyDerivationSpec {
            algorithm: "blake3".to_string(),
            domain: vec![1u8; 32],
            context: None,
        }
    }

    #[test]
    fn test_chunk_id_creation() {
        let manifest_cid = Cid::new("test-manifest");
        let chunk_id = ChunkId::for_manifest_chunk(&manifest_cid, 0);
        assert!(!chunk_id.as_bytes().is_empty());

        let chunk_id2 = ChunkId::for_manifest_chunk(&manifest_cid, 0);
        assert_eq!(chunk_id, chunk_id2);
    }

    #[test]
    fn test_chunk_id_from_content() {
        let content = b"test content";
        let chunk_id1 = ChunkId::from_content(content);
        let chunk_id2 = ChunkId::from_content(content);
        assert_eq!(chunk_id1, chunk_id2);

        let different_content = b"different content";
        let chunk_id3 = ChunkId::from_content(different_content);
        assert_ne!(chunk_id1, chunk_id3);
    }

    #[test]
    fn test_chunk_content() {
        let store = create_test_store();
        let content = vec![0u8; 5 * 1024 * 1024];
        let params = ChunkingParams::default_for_size(content.len() as u64);

        let chunks = store.chunk_content(&content, &params).unwrap();
        assert_eq!(chunks.len(), 5);
        assert_eq!(chunks[0].len(), 1024 * 1024);
    }

    #[test]
    fn test_encrypt_decrypt_chunk() {
        let store = create_test_store();
        let content = b"test chunk content";
        let key_spec = create_test_key_spec();

        let encrypted = store.encrypt_chunk(content, &key_spec).unwrap();
        assert_ne!(encrypted.ciphertext, content);

        let decrypted = store.decrypt_chunk(&encrypted, &key_spec).unwrap();
        assert_eq!(decrypted, content);
    }

    #[test]
    fn test_encrypt_decrypt_large_chunk() {
        let store = create_test_store();
        let content = vec![42u8; 1024 * 1024];
        let key_spec = create_test_key_spec();

        let encrypted = store.encrypt_chunk(&content, &key_spec).unwrap();
        let decrypted = store.decrypt_chunk(&encrypted, &key_spec).unwrap();
        assert_eq!(decrypted, content);
    }

    #[test]
    fn test_store_and_retrieve_chunk() {
        let mut store = create_test_store();
        let manifest_cid = Cid::from(vec![1u8; 32]);
        let content = b"test chunk";
        let key_spec = create_test_key_spec();

        let encrypted = store.encrypt_chunk(content, &key_spec).unwrap();
        let chunk_id = encrypted.chunk_id.clone();

        store
            .store_chunk(&manifest_cid, 0, encrypted, 1000)
            .unwrap();

        let retrieved = store.retrieve_chunk(&chunk_id).unwrap();
        assert_eq!(retrieved.chunk_id, chunk_id);
    }

    #[test]
    fn test_get_manifest_chunks() {
        let mut store = create_test_store();
        let manifest_cid = Cid::from(vec![1u8; 32]);
        let key_spec = create_test_key_spec();

        for i in 0..3 {
            let content = format!("chunk {}", i);
            let encrypted = store.encrypt_chunk(content.as_bytes(), &key_spec).unwrap();
            store
                .store_chunk(&manifest_cid, i, encrypted, 1000)
                .unwrap();
        }

        let chunks = store.get_manifest_chunks(&manifest_cid).unwrap();
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_delete_chunk() {
        let mut store = create_test_store();
        let manifest_cid = Cid::from(vec![1u8; 32]);
        let content = b"test chunk";
        let key_spec = create_test_key_spec();

        let encrypted = store.encrypt_chunk(content, &key_spec).unwrap();
        let chunk_id = encrypted.chunk_id.clone();

        store
            .store_chunk(&manifest_cid, 0, encrypted, 1000)
            .unwrap();
        assert!(store.retrieve_chunk(&chunk_id).is_ok());

        store.delete_chunk(&chunk_id).unwrap();
        assert!(matches!(
            store.retrieve_chunk(&chunk_id),
            Err(ChunkError::ChunkNotFound)
        ));
    }

    #[test]
    fn test_storage_stats() {
        let mut store = create_test_store();
        let manifest_cid = Cid::from(vec![1u8; 32]);
        let key_spec = create_test_key_spec();

        // Create chunks with different content to ensure unique chunk IDs
        for i in 0..5 {
            let content = vec![i as u8; 1024];
            let encrypted = store.encrypt_chunk(&content, &key_spec).unwrap();
            store
                .store_chunk(&manifest_cid, i, encrypted, 1000)
                .unwrap();
        }

        let stats = store.get_storage_stats();
        assert_eq!(stats.total_chunks, 5);
        assert_eq!(stats.unique_manifests, 1);
        assert!(stats.total_size > 0);
    }

    #[test]
    fn test_chunk_metadata() {
        let mut store = create_test_store();
        let manifest_cid = Cid::from(vec![1u8; 32]);
        let content = b"test chunk";
        let key_spec = create_test_key_spec();

        let encrypted = store.encrypt_chunk(content, &key_spec).unwrap();
        let chunk_id = encrypted.chunk_id.clone();

        store
            .store_chunk(&manifest_cid, 0, encrypted, 1000)
            .unwrap();

        let metadata = store.get_chunk_metadata(&chunk_id).unwrap();
        assert_eq!(metadata.chunk_index, 0);
        assert_eq!(metadata.manifest_cid, manifest_cid);
        assert_eq!(metadata.stored_at, 1000);
    }

    #[test]
    fn test_integrity_check() {
        let store = create_test_store();
        let content = b"test content";
        let key_spec = create_test_key_spec();

        let mut encrypted = store.encrypt_chunk(content, &key_spec).unwrap();

        encrypted.ciphertext[0] ^= 1;

        let result = store.decrypt_chunk(&encrypted, &key_spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_content_chunking() {
        let store = create_test_store();
        let content = vec![];
        let params = ChunkingParams::default_for_size(0);

        let result = store.chunk_content(&content, &params);
        assert!(matches!(result, Err(ChunkError::EmptyContent)));
    }
}
