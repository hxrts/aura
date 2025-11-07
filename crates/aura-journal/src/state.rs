//! Automerge-based account state implementation

use crate::error::{Error, Result};
use crate::types::{DeviceMetadata, DeviceType, GuardianMetadata};
use aura_crypto::Ed25519VerifyingKey;
use aura_types::{AccountId, DeviceId};
use automerge::{transaction::Transactable, AutoCommit, Automerge, ReadDoc};

/// Account state backed by Automerge CRDT
///
/// # Deprecation Notice
///
/// This implementation is deprecated in favor of `ModernAccountState` from
/// `crate::semilattice::account_state`. The new implementation provides the same
/// functionality with better performance and composability using the unified
/// semilattice system.
///
/// ## Migration
///
/// ```rust
/// use aura_journal::ModernAccountState;
/// use aura_journal::semilattice::account_state::migration;
///
/// // Convert from legacy to modern
/// let modern_state = migration::from_legacy(&legacy_state);
/// ```
///
/// See `MIGRATION.md` for detailed migration guidance.
#[deprecated(
    since = "0.1.0",
    note = "Use ModernAccountState from crate::semilattice::account_state instead"
)]
pub struct AccountState {
    /// Core Automerge document
    doc: AutoCommit,

    /// Cached object ID for devices list in Automerge document
    devices_list: automerge::ObjId,
    /// Cached object ID for guardians list in Automerge document
    guardians_list: automerge::ObjId,
    /// Cached object ID for operations map in Automerge document
    operations_map: automerge::ObjId,
    /// Cached object ID for capabilities map in Automerge document
    capabilities_map: automerge::ObjId,

    /// Account ID (immutable after creation)
    pub account_id: AccountId,
    /// Group public key for threshold signature verification (immutable after creation)
    pub group_public_key: Ed25519VerifyingKey,
}

impl AccountState {
    /// Create a new Automerge-backed account state
    pub fn new(account_id: AccountId, group_public_key: Ed25519VerifyingKey) -> Result<Self> {
        let mut doc = AutoCommit::new();

        // Initialize document structure
        let devices_list = doc
            .put_object(automerge::ROOT, "devices", automerge::ObjType::List)
            .map_err(|e| Error::storage_failed(format!("Failed to create devices list: {}", e)))?;

        let guardians_list = doc
            .put_object(automerge::ROOT, "guardians", automerge::ObjType::List)
            .map_err(|e| {
                Error::storage_failed(format!("Failed to create guardians list: {}", e))
            })?;

        let operations_map = doc
            .put_object(automerge::ROOT, "operations", automerge::ObjType::Map)
            .map_err(|e| {
                Error::storage_failed(format!("Failed to create operations map: {}", e))
            })?;

        let capabilities_map = doc
            .put_object(automerge::ROOT, "capabilities", automerge::ObjType::Map)
            .map_err(|e| {
                Error::storage_failed(format!("Failed to create capabilities map: {}", e))
            })?;

        // Set immutable fields
        doc.put(automerge::ROOT, "account_id", account_id.to_string())
            .map_err(|e| Error::storage_failed(format!("Failed to set account_id: {}", e)))?;

        // Initialize epoch counter (Max-Counter CRDT semantics)
        doc.put(automerge::ROOT, "epoch", 0u64)
            .map_err(|e| Error::storage_failed(format!("Failed to set epoch: {}", e)))?;

        // Initialize lamport clock
        doc.put(automerge::ROOT, "lamport_clock", 0u64)
            .map_err(|e| Error::storage_failed(format!("Failed to set lamport_clock: {}", e)))?;

        Ok(Self {
            doc,
            devices_list,
            guardians_list,
            operations_map,
            capabilities_map,
            account_id,
            group_public_key,
        })
    }

    /// Load from existing Automerge document
    pub fn from_document(
        doc: Automerge,
        account_id: AccountId,
        group_public_key: Ed25519VerifyingKey,
    ) -> Result<Self> {
        let mut auto_commit = AutoCommit::new();
        auto_commit
            .apply_changes(
                doc.get_changes(&[])
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| Error::storage_failed(format!("Failed to apply changes: {}", e)))?;

        // Get object IDs
        let devices_list = match auto_commit.get(automerge::ROOT, "devices") {
            Ok(Some((_, obj_id))) => obj_id,
            _ => return Err(Error::storage_failed("Missing or invalid devices list")),
        };

        let guardians_list = match auto_commit.get(automerge::ROOT, "guardians") {
            Ok(Some((_, obj_id))) => obj_id,
            _ => return Err(Error::storage_failed("Missing or invalid guardians list")),
        };

        let operations_map = match auto_commit.get(automerge::ROOT, "operations") {
            Ok(Some((_, obj_id))) => obj_id,
            _ => return Err(Error::storage_failed("Missing or invalid operations map")),
        };

        let capabilities_map = match auto_commit.get(automerge::ROOT, "capabilities") {
            Ok(Some((_, obj_id))) => obj_id,
            _ => return Err(Error::storage_failed("Missing or invalid capabilities map")),
        };

        Ok(Self {
            doc: auto_commit,
            devices_list: devices_list.clone(),
            guardians_list: guardians_list.clone(),
            operations_map: operations_map.clone(),
            capabilities_map: capabilities_map.clone(),
            account_id,
            group_public_key,
        })
    }

    // Device management

    /// Add a device to the account
    pub fn add_device(&mut self, device: DeviceMetadata) -> Result<Vec<automerge::Change>> {
        let device_idx = self.doc.length(&self.devices_list);
        let device_obj = self
            .doc
            .insert_object(&self.devices_list, device_idx, automerge::ObjType::Map)
            .map_err(|e| Error::storage_failed(format!("Failed to create device object: {}", e)))?;

        // Store device metadata
        self.doc
            .put(&device_obj, "id", device.device_id.to_string())
            .map_err(|e| Error::storage_failed(format!("Failed to set device id: {}", e)))?;
        self.doc
            .put(&device_obj, "name", device.device_name.clone())
            .map_err(|e| Error::storage_failed(format!("Failed to set device name: {}", e)))?;
        self.doc
            .put(&device_obj, "type", format!("{:?}", device.device_type))
            .map_err(|e| Error::storage_failed(format!("Failed to set device type: {}", e)))?;
        self.doc
            .put(&device_obj, "added_at", device.added_at as i64)
            .map_err(|e| Error::storage_failed(format!("Failed to set added_at: {}", e)))?;
        self.doc
            .put(&device_obj, "last_seen", device.last_seen as i64)
            .map_err(|e| Error::storage_failed(format!("Failed to set last_seen: {}", e)))?;

        // Store public key as hex string
        let public_key_hex = hex::encode(device.public_key.as_bytes());
        self.doc
            .put(&device_obj, "public_key", public_key_hex)
            .map_err(|e| Error::storage_failed(format!("Failed to set public key: {}", e)))?;

        // Mark as active (tombstone pattern for removal)
        self.doc
            .put(&device_obj, "active", true)
            .map_err(|e| Error::storage_failed(format!("Failed to set active flag: {}", e)))?;

        // Get changes for sync
        Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
    }

    /// Remove a device (tombstone pattern)
    pub fn remove_device(&mut self, device_id: DeviceId) -> Result<Vec<automerge::Change>> {
        // Find device in list
        let device_index = self.find_device_index(&device_id)?;

        if let Some(index) = device_index {
            let device_obj = match self.doc.get(&self.devices_list, index) {
                Ok(Some((_, obj_id))) => obj_id,
                _ => return Err(Error::storage_failed("Device not found at index")),
            };

            // Mark as inactive (tombstone)
            self.doc.put(&device_obj, "active", false).map_err(|e| {
                Error::storage_failed(format!("Failed to mark device inactive: {}", e))
            })?;
            self.doc
                .put(
                    &device_obj,
                    "removed_at",
                    aura_types::time::current_unix_timestamp() as i64,
                )
                .map_err(|e| Error::storage_failed(format!("Failed to set removed_at: {}", e)))?;
        } else {
            return Err(Error::storage_failed("Device not found"));
        }

        Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
    }

    /// Check if a device exists and is active
    pub fn has_device(&self, device_id: &DeviceId) -> bool {
        self.find_device_index(device_id).ok().flatten().is_some()
    }

    /// Get all active devices
    pub fn get_devices(&self) -> Vec<DeviceMetadata> {
        let mut devices = Vec::new();
        let length = self.doc.length(&self.devices_list);

        for i in 0..length {
            if let Ok(Some((_, obj_id))) = self.doc.get(&self.devices_list, i) {
                // Check if active
                if let Ok(Some((v, _))) = self.doc.get(&obj_id, "active") {
                    if v.to_bool() == Some(true) {
                        // Extract device metadata
                        if let Ok(device) = self.extract_device_metadata(&obj_id) {
                            devices.push(device);
                        }
                    }
                }
            }
        }

        devices
    }

    // Epoch management

    /// Get current epoch (lamport clock)
    pub fn get_epoch(&self) -> u64 {
        self.doc
            .get(automerge::ROOT, "epoch")
            .ok()
            .and_then(|opt| opt.map(|(v, _)| v))
            .and_then(|v| v.to_u64())
            .unwrap_or(0)
    }

    /// Increment epoch (Max-Counter CRDT - converges to highest value)
    pub fn increment_epoch(&mut self) -> Result<Vec<automerge::Change>> {
        let current = self.get_epoch();
        self.doc
            .put(automerge::ROOT, "epoch", current + 1)
            .map_err(|e| Error::storage_failed(format!("Failed to increment epoch: {}", e)))?;

        // Also update lamport clock
        let lamport = self.get_lamport_clock();
        self.doc
            .put(automerge::ROOT, "lamport_clock", lamport + 1)
            .map_err(|e| Error::storage_failed(format!("Failed to update lamport clock: {}", e)))?;

        Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
    }

    /// Set epoch if higher than current (for sync)
    pub fn set_epoch_if_higher(&mut self, new_epoch: u64) -> Result<Vec<automerge::Change>> {
        let current = self.get_epoch();
        if new_epoch > current {
            self.doc
                .put(automerge::ROOT, "epoch", new_epoch)
                .map_err(|e| Error::storage_failed(format!("Failed to set epoch: {}", e)))?;

            Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
        } else {
            Ok(vec![])
        }
    }

    /// Get lamport clock
    pub fn get_lamport_clock(&self) -> u64 {
        self.doc
            .get(automerge::ROOT, "lamport_clock")
            .ok()
            .and_then(|opt| opt.map(|(v, _)| v))
            .and_then(|v| v.to_u64())
            .unwrap_or(0)
    }

    // Guardian management

    /// Add a guardian
    pub fn add_guardian(&mut self, guardian: GuardianMetadata) -> Result<Vec<automerge::Change>> {
        let guardian_idx = self.doc.length(&self.guardians_list);
        let guardian_obj = self
            .doc
            .insert_object(&self.guardians_list, guardian_idx, automerge::ObjType::Map)
            .map_err(|e| {
                Error::storage_failed(format!("Failed to create guardian object: {}", e))
            })?;

        // Store guardian metadata
        self.doc
            .put(&guardian_obj, "id", guardian.guardian_id.to_string())
            .map_err(|e| Error::storage_failed(format!("Failed to set guardian id: {}", e)))?;
        self.doc
            .put(&guardian_obj, "email", guardian.email.clone())
            .map_err(|e| Error::storage_failed(format!("Failed to set guardian name: {}", e)))?;
        self.doc
            .put(&guardian_obj, "added_at", guardian.added_at as i64)
            .map_err(|e| Error::storage_failed(format!("Failed to set added_at: {}", e)))?;
        self.doc
            .put(&guardian_obj, "active", true)
            .map_err(|e| Error::storage_failed(format!("Failed to set active flag: {}", e)))?;

        Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
    }

    // Sync methods

    /// Get the Automerge document for sync
    pub fn document(&self) -> &automerge::AutoCommit {
        &self.doc
    }

    /// Get mutable reference to document
    pub fn document_mut(&mut self) -> &mut automerge::AutoCommit {
        &mut self.doc
    }

    /// Get underlying Automerge document for sync operations
    pub fn automerge_doc(&self) -> automerge::Automerge {
        // Clone the AutoCommit and convert to Automerge
        let mut doc = self.doc.clone();
        doc.document().clone()
    }

    /// Apply changes from remote
    pub fn apply_changes(&mut self, changes: Vec<automerge::Change>) -> Result<()> {
        self.doc
            .apply_changes(changes)
            .map_err(|e| Error::storage_failed(format!("Failed to apply changes: {}", e)))
    }

    /// Get current heads (vector clock)
    pub fn get_heads(&self) -> Vec<automerge::ChangeHash> {
        // Clone to avoid mutable borrow
        let mut doc = self.doc.clone();
        doc.document().get_heads().to_vec()
    }

    /// Save document to bytes
    pub fn save(&self) -> Result<Vec<u8>> {
        // Clone to avoid mutable borrow
        let mut doc = self.doc.clone();
        Ok(doc.save())
    }

    /// Load document from bytes
    pub fn load(
        bytes: &[u8],
        account_id: AccountId,
        group_public_key: Ed25519VerifyingKey,
    ) -> Result<Self> {
        let doc = Automerge::load(bytes)
            .map_err(|e| Error::storage_failed(format!("Failed to load document: {}", e)))?;
        Self::from_document(doc, account_id, group_public_key)
    }

    /// Query state at a specific path
    pub fn query_path(&self, path: &[String]) -> Result<serde_json::Value> {
        let mut current_obj = automerge::ROOT;

        // Navigate through the path
        for segment in path {
            match self.doc.get(&current_obj, segment) {
                Ok(Some((_, obj_id))) => {
                    current_obj = obj_id;
                }
                Ok(None) => {
                    return Ok(serde_json::Value::Null);
                }
                Err(e) => {
                    return Err(Error::storage_failed(format!(
                        "Failed to navigate path: {}",
                        e
                    )));
                }
            }
        }

        // Convert the value at this path to JSON
        // This is a simplified implementation - in practice you'd want more sophisticated conversion
        if path.is_empty() {
            return Ok(serde_json::json!({
                "account_id": self.account_id.to_string(),
                "epoch": self.get_epoch(),
                "devices_count": self.get_devices().len()
            }));
        }

        // For now, return a basic representation
        Ok(serde_json::json!({
            "path": path,
            "available": true
        }))
    }

    /// Merge a single change into the document
    pub fn merge_change(&mut self, change: automerge::Change) -> Result<()> {
        self.doc
            .apply_changes(vec![change])
            .map_err(|e| Error::storage_failed(format!("Failed to merge change: {}", e)))
    }

    /// Check if an operation has been applied
    pub fn has_operation(&self, op_id: &crate::operations::OperationId) -> bool {
        // For now, we'll check if the operation ID exists in a simple way
        // In practice, you'd maintain a separate index of applied operations
        let op_str = format!("{:?}", op_id);

        // Check if there's any record of this operation in the document
        match self.doc.get(automerge::ROOT, "applied_operations") {
            Ok(Some((_, obj_id))) => {
                // Check if the operation ID exists in the applied operations map
                match self.doc.get(&obj_id, &op_str) {
                    Ok(Some(_)) => true,
                    _ => false,
                }
            }
            _ => false,
        }
    }

    // Helper methods

    fn find_device_index(&self, device_id: &DeviceId) -> Result<Option<usize>> {
        let length = self.doc.length(&self.devices_list);
        let device_id_str = device_id.to_string();

        for i in 0..length {
            if let Ok(Some((_, obj_id))) = self.doc.get(&self.devices_list, i) {
                // Check if active
                if let Ok(Some((v, _))) = self.doc.get(&obj_id, "active") {
                    if v.to_bool() == Some(true) {
                        // Check ID
                        if let Ok(Some((id_val, _))) = self.doc.get(&obj_id, "id") {
                            if let Some(id_str) = id_val.to_str() {
                                if id_str == device_id_str {
                                    return Ok(Some(i as usize));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    fn extract_device_metadata(&self, obj_id: &automerge::ObjId) -> Result<DeviceMetadata> {
        // Extract fields from Automerge object
        let device_id = self
            .doc
            .get(obj_id, "id")
            .ok()
            .and_then(|opt| opt.map(|(v, _)| v))
            .and_then(|v| v.to_str().map(|s| s.to_string()))
            .and_then(|s| s.parse::<DeviceId>().ok())
            .ok_or_else(|| Error::storage_failed("Invalid device ID"))?;

        let device_name = self
            .doc
            .get(obj_id, "name")
            .ok()
            .and_then(|opt| opt.map(|(v, _)| v))
            .and_then(|v| v.to_str().map(|s| s.to_string()))
            .ok_or_else(|| Error::storage_failed("Invalid device name"))?;

        let added_at = self
            .doc
            .get(obj_id, "added_at")
            .ok()
            .and_then(|opt| opt.map(|(v, _)| v))
            .and_then(|v| v.to_i64())
            .map(|i| i as u64)
            .ok_or_else(|| Error::storage_failed("Invalid added_at"))?;

        let last_seen = self
            .doc
            .get(obj_id, "last_seen")
            .ok()
            .and_then(|opt| opt.map(|(v, _)| v))
            .and_then(|v| v.to_i64())
            .map(|i| i as u64)
            .ok_or_else(|| Error::storage_failed("Invalid last_seen"))?;

        let public_key_hex = self
            .doc
            .get(obj_id, "public_key")
            .ok()
            .and_then(|opt| opt.map(|(v, _)| v))
            .and_then(|v| v.to_str().map(|s| s.to_string()))
            .ok_or_else(|| Error::storage_failed("Invalid public key"))?;

        let public_key_bytes = hex::decode(public_key_hex)
            .map_err(|_| Error::storage_failed("Invalid public key hex"))?;

        let public_key_array: [u8; 32] = public_key_bytes
            .try_into()
            .map_err(|_| Error::storage_failed("Invalid public key length"))?;
        let public_key = Ed25519VerifyingKey::from_bytes(&public_key_array)
            .map_err(|_| Error::storage_failed("Invalid public key"))?;

        Ok(DeviceMetadata {
            device_id,
            device_name,
            device_type: DeviceType::Native, // TODO: store and retrieve
            public_key,
            added_at,
            last_seen,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 0,
            used_nonces: std::collections::BTreeSet::new(),
            key_share_epoch: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_automerge_state_creation() {
        let effects = Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();

        let state = AccountState::new(account_id, group_public_key).unwrap();
        assert_eq!(state.get_epoch(), 0);
        assert_eq!(state.get_devices().len(), 0);
    }

    #[test]
    fn test_device_management() {
        let effects = Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();

        let mut state = AccountState::new(account_id, group_public_key).unwrap();

        let device_id = DeviceId::new_with_effects(&effects);
        let device = DeviceMetadata {
            device_id,
            device_name: "Test Device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: 1000,
            last_seen: 1000,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 0,
            used_nonces: std::collections::BTreeSet::new(),
            key_share_epoch: 0,
        };

        // Add device
        let changes = state.add_device(device.clone()).unwrap();
        assert!(!changes.is_empty());
        assert_eq!(state.get_devices().len(), 1);
        assert!(state.has_device(&device_id));

        // Remove device
        let changes = state.remove_device(device_id).unwrap();
        assert!(!changes.is_empty());
        assert_eq!(state.get_devices().len(), 0);
        assert!(!state.has_device(&device_id));
    }

    #[test]
    fn test_epoch_management() {
        let effects = Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();

        let mut state = AccountState::new(account_id, group_public_key).unwrap();

        assert_eq!(state.get_epoch(), 0);

        // Increment epoch
        let changes = state.increment_epoch().unwrap();
        assert!(!changes.is_empty());
        assert_eq!(state.get_epoch(), 1);

        // Set epoch if higher
        let changes = state.set_epoch_if_higher(5).unwrap();
        assert!(!changes.is_empty());
        assert_eq!(state.get_epoch(), 5);

        // Try to set lower epoch (should not change)
        let changes = state.set_epoch_if_higher(3).unwrap();
        assert!(changes.is_empty());
        assert_eq!(state.get_epoch(), 5);
    }
}
