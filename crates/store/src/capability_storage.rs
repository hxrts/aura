// Capability-driven storage with causal encryption

use aura_groups::{
    encryption::{CausalCiphertext, CausalEncryption},
    ApplicationSecret,
};
use aura_journal::{
    capability::{
        authority_graph::AuthorityGraph,
        identity::IndividualId,
        types::{CapabilityResult, CapabilityScope},
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tokio::{fs, sync::RwLock};
use tracing::{debug, info, warn};

/// Capability-protected storage entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityStorageEntry {
    /// Entry identifier
    pub entry_id: String,
    /// Required capability scope for access
    pub required_scope: CapabilityScope,
    /// Content encrypted with causal encryption
    pub encrypted_content: CausalCiphertext,
    /// Metadata about the entry
    pub metadata: StorageMetadata,
    /// Access control list (additional restrictions)
    pub acl: Option<BTreeSet<IndividualId>>,
}

/// Storage metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetadata {
    /// Creation timestamp
    pub created_at: u64,
    /// Last modified timestamp
    pub modified_at: u64,
    /// Content type/mime type
    pub content_type: String,
    /// Content size in bytes
    pub size: usize,
    /// Creator identity
    pub created_by: IndividualId,
    /// Last modifier identity
    pub modified_by: IndividualId,
    /// Content hash for integrity
    pub content_hash: [u8; 32],
    /// Custom attributes
    pub attributes: BTreeMap<String, String>,
}

/// Capability-driven storage manager
pub struct CapabilityStorage {
    /// Storage root directory
    storage_root: PathBuf,
    /// Individual identity
    individual_id: IndividualId,
    /// Authority graph for capability evaluation
    authority_graph: RwLock<AuthorityGraph>,
    /// Causal encryption manager
    causal_encryption: RwLock<CausalEncryption>,
    /// Storage index (entry_id -> metadata)
    storage_index: RwLock<BTreeMap<String, CapabilityStorageEntry>>,
    /// Access logs for auditing
    access_logs: RwLock<Vec<AccessLogEntry>>,
    /// Injectable effects for deterministic testing
    effects: aura_crypto::Effects,
}

impl CapabilityStorage {
    /// Create new capability storage
    pub async fn new(
        storage_root: PathBuf,
        individual_id: IndividualId,
        effects: aura_crypto::Effects,
    ) -> Result<Self, StorageError> {
        info!(
            "Creating capability storage at {:?} for individual: {}",
            storage_root, individual_id.0
        );

        // Ensure storage directory exists
        fs::create_dir_all(&storage_root)
            .await
            .map_err(|e| StorageError::IoError(e.to_string()))?;

        let storage = Self {
            storage_root,
            individual_id,
            authority_graph: RwLock::new(AuthorityGraph::new()),
            causal_encryption: RwLock::new(CausalEncryption::new()),
            storage_index: RwLock::new(BTreeMap::new()),
            access_logs: RwLock::new(Vec::new()),
            effects,
        };

        // Load existing storage index
        storage.load_storage_index().await?;

        Ok(storage)
    }

    /// Update authority graph
    pub async fn update_authority_graph(&self, authority_graph: AuthorityGraph) {
        let mut graph = self.authority_graph.write().await;
        *graph = authority_graph;
        debug!("Updated authority graph in storage");
    }

    /// Add application secret for causal encryption
    pub async fn add_application_secret(&self, secret: ApplicationSecret) {
        let mut encryption = self.causal_encryption.write().await;
        let epoch = secret.epoch.value();
        encryption.add_application_secret(secret);
        debug!("Added application secret for epoch {}", epoch);
    }

    /// Store data with capability protection
    pub async fn store(
        &self,
        entry_id: String,
        data: Vec<u8>,
        content_type: String,
        required_scope: CapabilityScope,
        acl: Option<BTreeSet<IndividualId>>,
        attributes: BTreeMap<String, String>,
        effects: &aura_crypto::Effects,
    ) -> Result<(), StorageError> {
        info!(
            "Storing entry '{}' ({} bytes) with scope {}:{}",
            entry_id,
            data.len(),
            required_scope.namespace,
            required_scope.operation
        );

        // Check that requester has write capability
        let write_scope = CapabilityScope::simple("storage", "write");
        self.require_capability(&write_scope).await?;

        // Encrypt data using causal encryption
        let context = format!("storage:{}", entry_id);
        let encrypted_content = {
            let encryption = self.causal_encryption.read().await;
            encryption
                .encrypt(&data, &context)
                .map_err(|e| StorageError::EncryptionError(e.to_string()))?
        };

        // Create storage metadata
        let content_hash = *blake3::hash(&data).as_bytes();
        let timestamp = effects.now().unwrap_or(0);

        let metadata = StorageMetadata {
            created_at: timestamp,
            modified_at: timestamp,
            content_type,
            size: data.len(),
            created_by: self.individual_id.clone(),
            modified_by: self.individual_id.clone(),
            content_hash,
            attributes,
        };

        // Create storage entry
        let entry = CapabilityStorageEntry {
            entry_id: entry_id.clone(),
            required_scope: required_scope.clone(),
            encrypted_content,
            metadata,
            acl,
        };

        // Store entry to disk
        self.write_entry_to_disk(&entry).await?;

        // Update storage index
        {
            let mut index = self.storage_index.write().await;
            index.insert(entry_id.clone(), entry);
        }

        // Log access
        self.log_access(AccessType::Write, &entry_id, &required_scope, effects)
            .await;

        debug!("Entry '{}' stored successfully", entry_id);

        Ok(())
    }

    /// Retrieve data with capability checking
    pub async fn retrieve(
        &self,
        entry_id: &str,
        effects: &aura_crypto::Effects,
    ) -> Result<Vec<u8>, StorageError> {
        debug!("Retrieving entry '{}'", entry_id);

        // Get entry from index
        let entry = {
            let index = self.storage_index.read().await;
            index
                .get(entry_id)
                .ok_or_else(|| StorageError::NotFound(entry_id.to_string()))?
                .clone()
        };

        // Check capability requirements
        self.require_capability(&entry.required_scope).await?;

        // Check ACL if present
        if let Some(acl) = &entry.acl {
            if !acl.contains(&self.individual_id) {
                return Err(StorageError::AccessDenied(format!(
                    "Individual {} not in ACL for entry '{}'",
                    self.individual_id.0, entry_id
                )));
            }
        }

        // Decrypt content
        let decrypted_data = {
            let encryption = self.causal_encryption.read().await;
            encryption
                .decrypt(&entry.encrypted_content)
                .map_err(|e| StorageError::DecryptionError(e.to_string()))?
        };

        // Verify content integrity
        let computed_hash = *blake3::hash(&decrypted_data).as_bytes();
        if computed_hash != entry.metadata.content_hash {
            return Err(StorageError::IntegrityError(
                "Content hash mismatch".to_string(),
            ));
        }

        // Log access
        self.log_access(AccessType::Read, entry_id, &entry.required_scope, effects)
            .await;

        debug!(
            "Entry '{}' retrieved successfully ({} bytes)",
            entry_id,
            decrypted_data.len()
        );

        Ok(decrypted_data)
    }

    /// Delete entry with capability checking
    pub async fn delete(
        &self,
        entry_id: &str,
        effects: &aura_crypto::Effects,
    ) -> Result<(), StorageError> {
        info!("Deleting entry '{}'", entry_id);

        // Check delete capability
        let delete_scope = CapabilityScope::simple("storage", "delete");
        self.require_capability(&delete_scope).await?;

        // Get entry from index
        let entry = {
            let index = self.storage_index.read().await;
            index
                .get(entry_id)
                .ok_or_else(|| StorageError::NotFound(entry_id.to_string()))?
                .clone()
        };

        // Also check the entry's required scope
        self.require_capability(&entry.required_scope).await?;

        // Remove from disk
        self.delete_entry_from_disk(entry_id).await?;

        // Remove from index
        {
            let mut index = self.storage_index.write().await;
            index.remove(entry_id);
        }

        // Log access
        self.log_access(AccessType::Delete, entry_id, &delete_scope, effects)
            .await;

        debug!("Entry '{}' deleted successfully", entry_id);

        Ok(())
    }

    /// List entries accessible to current identity
    pub async fn list_entries(&self) -> Result<Vec<String>, StorageError> {
        debug!("Listing accessible entries");

        let list_scope = CapabilityScope::simple("storage", "list");
        self.require_capability(&list_scope).await?;

        let mut accessible_entries = Vec::new();
        let index = self.storage_index.read().await;

        for (entry_id, entry) in index.iter() {
            // Check if we have access to this entry
            if self.check_capability(&entry.required_scope).await {
                // Also check ACL if present
                if let Some(acl) = &entry.acl {
                    if acl.contains(&self.individual_id) {
                        accessible_entries.push(entry_id.clone());
                    }
                } else {
                    accessible_entries.push(entry_id.clone());
                }
            }
        }

        debug!("Found {} accessible entries", accessible_entries.len());

        Ok(accessible_entries)
    }

    /// Get entry metadata
    pub async fn get_metadata(&self, entry_id: &str) -> Result<StorageMetadata, StorageError> {
        debug!("Getting metadata for entry '{}'", entry_id);

        let entry = {
            let index = self.storage_index.read().await;
            index
                .get(entry_id)
                .ok_or_else(|| StorageError::NotFound(entry_id.to_string()))?
                .clone()
        };

        // Check capability requirements for metadata access
        self.require_capability(&entry.required_scope).await?;

        Ok(entry.metadata)
    }

    /// Require specific capability or return error
    async fn require_capability(&self, scope: &CapabilityScope) -> Result<(), StorageError> {
        if !self.check_capability(scope).await {
            return Err(StorageError::InsufficientCapability(format!(
                "Required capability not found: {}:{}",
                scope.namespace, scope.operation
            )));
        }
        Ok(())
    }

    /// Check if current identity has specific capability
    async fn check_capability(&self, scope: &CapabilityScope) -> bool {
        let graph = self.authority_graph.read().await;
        let subject = self.individual_id.to_subject();
        let result = graph.evaluate_capability(&subject, scope, &self.effects);
        matches!(result, CapabilityResult::Granted)
    }

    /// Write entry to disk
    async fn write_entry_to_disk(
        &self,
        entry: &CapabilityStorageEntry,
    ) -> Result<(), StorageError> {
        let entry_path = self.storage_root.join(format!("{}.entry", entry.entry_id));
        let entry_data = serde_json::to_vec_pretty(entry)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        fs::write(&entry_path, entry_data)
            .await
            .map_err(|e| StorageError::IoError(e.to_string()))?;

        Ok(())
    }

    /// Delete entry from disk
    async fn delete_entry_from_disk(&self, entry_id: &str) -> Result<(), StorageError> {
        let entry_path = self.storage_root.join(format!("{}.entry", entry_id));

        if entry_path.exists() {
            fs::remove_file(&entry_path)
                .await
                .map_err(|e| StorageError::IoError(e.to_string()))?;
        }

        Ok(())
    }

    /// Load storage index from disk
    async fn load_storage_index(&self) -> Result<(), StorageError> {
        let mut index = BTreeMap::new();

        let mut dir = fs::read_dir(&self.storage_root)
            .await
            .map_err(|e| StorageError::IoError(e.to_string()))?;

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| StorageError::IoError(e.to_string()))?
        {
            if let Some(file_name) = entry.file_name().to_str() {
                if file_name.ends_with(".entry") {
                    let entry_data = fs::read(entry.path())
                        .await
                        .map_err(|e| StorageError::IoError(e.to_string()))?;

                    let storage_entry: CapabilityStorageEntry = serde_json::from_slice(&entry_data)
                        .map_err(|e| StorageError::SerializationError(e.to_string()))?;

                    index.insert(storage_entry.entry_id.clone(), storage_entry);
                }
            }
        }

        let mut storage_index = self.storage_index.write().await;
        *storage_index = index;

        info!("Loaded {} entries from storage index", storage_index.len());

        Ok(())
    }

    /// Log access for auditing
    async fn log_access(
        &self,
        access_type: AccessType,
        entry_id: &str,
        scope: &CapabilityScope,
        effects: &aura_crypto::Effects,
    ) {
        let log_entry = AccessLogEntry {
            timestamp: effects.now().unwrap_or(0),
            access_type,
            entry_id: entry_id.to_string(),
            individual_id: self.individual_id.clone(),
            scope: scope.clone(),
        };

        let mut logs = self.access_logs.write().await;
        logs.push(log_entry);

        // Keep only recent logs (prevent unbounded growth)
        if logs.len() > 10000 {
            logs.drain(0..1000); // Remove oldest 1000 entries
        }
    }

    /// Get access logs (for auditing)
    pub async fn get_access_logs(&self) -> Vec<AccessLogEntry> {
        let audit_scope = CapabilityScope::simple("storage", "audit");
        if !self.check_capability(&audit_scope).await {
            warn!("Access to audit logs denied - insufficient capability");
            return Vec::new();
        }

        self.access_logs.read().await.clone()
    }

    /// Clean up old application secrets and causal keys
    pub async fn cleanup_old_keys(&self, retain_epochs: usize) {
        let mut encryption = self.causal_encryption.write().await;
        encryption.cleanup_old_keys(retain_epochs);
        debug!(
            "Cleaned up old causal encryption keys, retaining {} epochs",
            retain_epochs
        );
    }
}

/// Access log entry for auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessLogEntry {
    /// Unix timestamp when the access occurred
    pub timestamp: u64,
    /// Type of access operation performed
    pub access_type: AccessType,
    /// Identifier of the storage entry accessed
    pub entry_id: String,
    /// Individual who performed the access
    pub individual_id: IndividualId,
    /// Capability scope used for the access
    pub scope: CapabilityScope,
}

/// Type of storage access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessType {
    /// Reading data from storage
    Read,
    /// Writing data to storage
    Write,
    /// Deleting data from storage
    Delete,
    /// Listing storage entries
    List,
    /// Accessing entry metadata
    Metadata,
}

/// Storage layer errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Storage entry was not found
    #[error("Entry not found: {0}")]
    NotFound(String),

    /// Access to storage entry was denied
    #[error("Access denied: {0}")]
    AccessDenied(String),

    /// Insufficient capability to perform operation
    #[error("Insufficient capability: {0}")]
    InsufficientCapability(String),

    /// Error occurred during encryption
    #[error("Encryption error: {0}")]
    EncryptionError(String),

    /// Error occurred during decryption
    #[error("Decryption error: {0}")]
    DecryptionError(String),

    /// Data integrity check failed
    #[error("Integrity error: {0}")]
    IntegrityError(String),

    /// Input/output operation failed
    #[error("IO error: {0}")]
    IoError(String),

    /// Error occurred during serialization/deserialization
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Operation is not valid in current context
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}
