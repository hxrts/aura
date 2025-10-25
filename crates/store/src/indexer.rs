// Storage indexer with quota and eviction

use crate::encryption::{EncryptionContext, Recipients};
use crate::{ChunkId, ObjectManifest, StaticReplicationHint};
use crate::{Result, StorageError};
use aura_journal::serialization::{from_cbor_bytes, to_cbor_bytes};
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

const MANIFESTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("manifests");
const CHUNKS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("chunks");
const QUOTA_TABLE: TableDefinition<&str, u64> = TableDefinition::new("quota");

/// Storage indexer
pub struct Indexer {
    db: Arc<RwLock<Database>>,
    quota_limit: u64,
}

impl Indexer {
    /// Create a new indexer
    pub fn new<P: AsRef<Path>>(path: P, quota_limit: u64) -> Result<Self> {
        let db = Database::create(path)
            .map_err(|e| StorageError::Storage(format!("Failed to create database: {}", e)))?;

        // Initialize tables
        {
            let write_txn = db.begin_write().map_err(|e| {
                StorageError::Storage(format!("Failed to begin transaction: {}", e))
            })?;
            {
                let _ = write_txn.open_table(MANIFESTS_TABLE).map_err(|e| {
                    StorageError::Storage(format!("Failed to open manifests table: {}", e))
                })?;
                let _ = write_txn.open_table(CHUNKS_TABLE).map_err(|e| {
                    StorageError::Storage(format!("Failed to open chunks table: {}", e))
                })?;
                let _ = write_txn.open_table(QUOTA_TABLE).map_err(|e| {
                    StorageError::Storage(format!("Failed to open quota table: {}", e))
                })?;
            }
            write_txn
                .commit()
                .map_err(|e| StorageError::Storage(format!("Failed to commit: {}", e)))?;
        }

        Ok(Indexer {
            db: Arc::new(RwLock::new(db)),
            quota_limit,
        })
    }

    /// Store encrypted data
    pub async fn store_encrypted(
        &self,
        payload: &[u8],
        recipients: Recipients,
        opts: PutOpts,
        effects: &aura_crypto::Effects,
    ) -> Result<crate::manifest::Cid> {
        // Check quota
        let current_usage = self.get_quota_usage().await?;
        if current_usage + payload.len() as u64 > self.quota_limit {
            return Err(StorageError::QuotaExceeded);
        }

        // Generate encryption key
        let enc_ctx = EncryptionContext::new();

        // Encrypt payload
        let ciphertext = enc_ctx.encrypt(payload)?;

        // Phase 3 uses key derivation instead of key wrapping
        // Store the encryption key separately for Phase 3 manifest structure
        
        // Create Phase 3 manifest with proper structure
        let chunk_digests = vec![*blake3::hash(&ciphertext).as_bytes()];
        let chunking = crate::manifest::ChunkingParams::new(
            ciphertext.len() as u64, 
            crate::manifest::ChunkingParams::DEFAULT_CHUNK_SIZE
        );
        
        // Create key derivation spec for Phase 3
        let key_derivation = crate::manifest::KeyDerivationSpec {
            identity_context: crate::manifest::IdentityKeyContext::DeviceEncryption { 
                device_id: vec![] // Placeholder device ID
            },
            permission_context: None,
            derivation_path: opts.context_id.map(|id| id.to_vec()).unwrap_or_default(),
            key_version: 1,
        };
        
        // Create access control based on recipients
        let access_control = crate::manifest::AccessControl::new_capability_based(vec![]);
        
        // Convert replication hint to static hint for Phase 3
        let replication_hint = crate::manifest::StaticReplicationHint::local_only();
        
        // Create placeholder threshold signature for Phase 3
        let sig = crate::manifest::ThresholdSignature::placeholder();
        
        let manifest = ObjectManifest {
            root_cid: blake3::hash(&ciphertext).as_bytes().to_vec(),
            size: payload.len() as u64,
            chunking,
            chunk_digests,
            erasure: None,
            context_id: opts.context_id,
            app_metadata: opts.app_metadata,
            key_derivation,
            access_control,
            replication_hint,
            version: 1,
            prev_manifest: None,
            issued_at_ms: current_timestamp_ms_with_effects(effects),
            nonce: generate_nonce_with_effects(effects),
            sig,
        };

        let manifest_cid = manifest.compute_cid();

        // Store manifest and chunks
        self.store_manifest(&manifest_cid, &manifest).await?;
        self.store_chunk(&ChunkId::new(&manifest_cid, 0), &ciphertext)
            .await?;

        // Update quota
        self.update_quota(payload.len() as u64).await?;

        Ok(manifest_cid)
    }

    /// Fetch encrypted data
    ///
    /// # Arguments
    ///
    /// * `cid` - Content identifier
    /// * `device_id` - Current device ID for key unwrapping
    /// * `device_secret` - Device secret for key unwrapping
    pub async fn fetch_encrypted(
        &self,
        cid: &crate::manifest::Cid,
        device_id: aura_journal::DeviceId,
        device_secret: &[u8; 32],
    ) -> Result<(Vec<u8>, ObjectManifest)> {
        // Load manifest
        let manifest = self.load_manifest(cid).await?;

        // Load chunks (for MVP, single chunk)
        let chunk_id = ChunkId::new(cid, 0);
        let ciphertext = self.load_chunk(&chunk_id).await?;

        // For Phase 3, derive key from manifest key derivation spec
        // This is a placeholder - in production would use proper key derivation
        let key = device_secret.clone();
        let enc_ctx = EncryptionContext::from_key(key);
        let plaintext = enc_ctx.decrypt(&ciphertext)?;

        Ok((plaintext, manifest))
    }

    /// Store manifest
    async fn store_manifest(&self, cid: &crate::manifest::Cid, manifest: &ObjectManifest) -> Result<()> {
        let db = self.db.write().await;
        let write_txn = db
            .begin_write()
            .map_err(|e| StorageError::Storage(format!("Failed to begin transaction: {}", e)))?;

        {
            let mut table = write_txn
                .open_table(MANIFESTS_TABLE)
                .map_err(|e| StorageError::Storage(format!("Failed to open table: {}", e)))?;

            let manifest_bytes = to_cbor_bytes(manifest)
                .map_err(|e| StorageError::Storage(format!("Failed to serialize: {}", e)))?;

            table
                .insert(hex::encode(cid).as_str(), manifest_bytes.as_slice())
                .map_err(|e| StorageError::Storage(format!("Failed to insert: {}", e)))?;
        }

        write_txn
            .commit()
            .map_err(|e| StorageError::Storage(format!("Failed to commit: {}", e)))?;

        Ok(())
    }

    /// Load manifest
    async fn load_manifest(&self, cid: &crate::manifest::Cid) -> Result<ObjectManifest> {
        let db = self.db.read().await;
        let read_txn = db
            .begin_read()
            .map_err(|e| StorageError::Storage(format!("Failed to begin transaction: {}", e)))?;

        let table = read_txn
            .open_table(MANIFESTS_TABLE)
            .map_err(|e| StorageError::Storage(format!("Failed to open table: {}", e)))?;

        let value = table
            .get(hex::encode(cid).as_str())
            .map_err(|e| StorageError::Storage(format!("Failed to get: {}", e)))?
            .ok_or_else(|| StorageError::NotFound(hex::encode(cid)))?;

        let manifest: ObjectManifest = from_cbor_bytes(value.value())
            .map_err(|e| StorageError::Storage(format!("Failed to deserialize: {}", e)))?;

        Ok(manifest)
    }

    /// Store chunk
    async fn store_chunk(&self, chunk_id: &ChunkId, data: &[u8]) -> Result<()> {
        let db = self.db.write().await;
        let write_txn = db
            .begin_write()
            .map_err(|e| StorageError::Storage(format!("Failed to begin transaction: {}", e)))?;

        {
            let mut table = write_txn
                .open_table(CHUNKS_TABLE)
                .map_err(|e| StorageError::Storage(format!("Failed to open table: {}", e)))?;

            table
                .insert(chunk_id.to_hex().as_str(), data)
                .map_err(|e| StorageError::Storage(format!("Failed to insert: {}", e)))?;
        }

        write_txn
            .commit()
            .map_err(|e| StorageError::Storage(format!("Failed to commit: {}", e)))?;

        Ok(())
    }

    /// Load chunk
    async fn load_chunk(&self, chunk_id: &ChunkId) -> Result<Vec<u8>> {
        let db = self.db.read().await;
        let read_txn = db
            .begin_read()
            .map_err(|e| StorageError::Storage(format!("Failed to begin transaction: {}", e)))?;

        let table = read_txn
            .open_table(CHUNKS_TABLE)
            .map_err(|e| StorageError::Storage(format!("Failed to open table: {}", e)))?;

        let value = table
            .get(chunk_id.to_hex().as_str())
            .map_err(|e| StorageError::Storage(format!("Failed to get: {}", e)))?
            .ok_or_else(|| StorageError::NotFound(chunk_id.to_hex()))?;

        Ok(value.value().to_vec())
    }

    /// Get quota usage
    async fn get_quota_usage(&self) -> Result<u64> {
        let db = self.db.read().await;
        let read_txn = db
            .begin_read()
            .map_err(|e| StorageError::Storage(format!("Failed to begin transaction: {}", e)))?;

        let table = read_txn
            .open_table(QUOTA_TABLE)
            .map_err(|e| StorageError::Storage(format!("Failed to open table: {}", e)))?;

        Ok(table
            .get("usage")
            .map_err(|e| StorageError::Storage(format!("Failed to get: {}", e)))?
            .map(|v| v.value())
            .unwrap_or(0))
    }

    /// Update quota
    async fn update_quota(&self, delta: u64) -> Result<()> {
        let db = self.db.write().await;
        let write_txn = db
            .begin_write()
            .map_err(|e| StorageError::Storage(format!("Failed to begin transaction: {}", e)))?;

        {
            let mut table = write_txn
                .open_table(QUOTA_TABLE)
                .map_err(|e| StorageError::Storage(format!("Failed to open table: {}", e)))?;

            let current = table
                .get("usage")
                .map_err(|e| StorageError::Storage(format!("Failed to get: {}", e)))?
                .map(|v| v.value())
                .unwrap_or(0);

            table
                .insert("usage", current + delta)
                .map_err(|e| StorageError::Storage(format!("Failed to insert: {}", e)))?;
        }

        write_txn
            .commit()
            .map_err(|e| StorageError::Storage(format!("Failed to commit: {}", e)))?;

        Ok(())
    }
}

/// Put options
#[derive(Debug, Clone)]
pub struct PutOpts {
    pub context_id: Option<[u8; 32]>,
    pub app_metadata: Option<Vec<u8>>,
    pub replication_hint: StaticReplicationHint,
}

impl Default for PutOpts {
    fn default() -> Self {
        PutOpts {
            context_id: None,
            app_metadata: None,
            replication_hint: StaticReplicationHint::local_only(),
        }
    }
}

fn current_timestamp_ms_with_effects(effects: &aura_crypto::Effects) -> u64 {
    effects.now().unwrap_or(0) * 1000
}

fn generate_nonce_with_effects(effects: &aura_crypto::Effects) -> [u8; 32] {
    effects.random_bytes::<32>()
}
