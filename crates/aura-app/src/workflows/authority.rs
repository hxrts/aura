//! Authority Management Workflow - Portable Business Logic
//!
//! This module contains authority record types and helper functions
//! that are portable across all frontends.

use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

// ============================================================================
// Types
// ============================================================================

/// Authority record for persistence.
///
/// Represents an authority entity with its threshold and associated devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorityRecord {
    /// Unique identifier for this authority
    pub authority_id: AuthorityId,
    /// Threshold for multi-party operations
    pub threshold: u32,
    /// List of device public keys registered to this authority
    pub devices: Vec<String>,
    /// Timestamp when the authority was created (ms since epoch)
    pub created_ms: u64,
}

impl AuthorityRecord {
    /// Create a new authority record.
    #[must_use]
    pub fn new(authority_id: AuthorityId, threshold: u32, created_ms: u64) -> Self {
        Self {
            authority_id,
            threshold,
            devices: Vec::new(),
            created_ms,
        }
    }

    /// Add a device public key to this authority.
    pub fn add_device(&mut self, public_key: impl Into<String>) {
        self.devices.push(public_key.into());
    }

    /// Check if a device is registered to this authority.
    pub fn has_device(&self, public_key: &str) -> bool {
        self.devices.iter().any(|d| d == public_key)
    }

    /// Get the number of devices registered to this authority.
    #[must_use]
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Generate the storage key for this authority record.
    #[must_use]
    pub fn storage_key(&self) -> String {
        format!("authority:{}", self.authority_id)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Generate a storage key prefix for authority records.
#[must_use]
pub const fn authority_key_prefix() -> &'static str {
    "authority:"
}

/// Generate a storage key for a specific authority.
#[must_use]
pub fn authority_storage_key(authority_id: &AuthorityId) -> String {
    format!("authority:{authority_id}")
}

/// Serialize an authority record to bytes.
pub fn serialize_authority(record: &AuthorityRecord) -> Result<Vec<u8>, String> {
    serde_json::to_vec(record).map_err(|e| format!("Failed to serialize authority record: {e}"))
}

/// Deserialize an authority record from bytes.
pub fn deserialize_authority(bytes: &[u8]) -> Result<AuthorityRecord, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("Failed to parse authority record: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority_id(seed: u8) -> AuthorityId {
        let mut entropy = [0u8; 32];
        for (i, byte) in entropy.iter_mut().enumerate() {
            *byte = seed.wrapping_add(i as u8);
        }
        AuthorityId::new_from_entropy(entropy)
    }

    #[test]
    fn test_authority_record_new() {
        let authority_id = test_authority_id(1);
        let record = AuthorityRecord::new(authority_id, 2, 1000);

        assert_eq!(record.authority_id, authority_id);
        assert_eq!(record.threshold, 2);
        assert!(record.devices.is_empty());
        assert_eq!(record.created_ms, 1000);
    }

    #[test]
    fn test_add_device() {
        let authority_id = test_authority_id(1);
        let mut record = AuthorityRecord::new(authority_id, 2, 1000);

        record.add_device("device1_pubkey");
        record.add_device("device2_pubkey");

        assert_eq!(record.device_count(), 2);
        assert!(record.has_device("device1_pubkey"));
        assert!(record.has_device("device2_pubkey"));
        assert!(!record.has_device("device3_pubkey"));
    }

    #[test]
    fn test_storage_key() {
        let authority_id = test_authority_id(1);
        let record = AuthorityRecord::new(authority_id, 2, 1000);

        let key = record.storage_key();
        assert!(key.starts_with("authority:"));
        assert!(key.contains(&authority_id.to_string()));
    }

    #[test]
    fn test_authority_storage_key_fn() {
        let authority_id = test_authority_id(1);
        let key = authority_storage_key(&authority_id);
        assert!(key.starts_with("authority:"));
    }

    #[test]
    fn test_authority_key_prefix() {
        assert_eq!(authority_key_prefix(), "authority:");
    }

    #[test]
    fn test_serialize_deserialize() {
        let authority_id = test_authority_id(1);
        let mut record = AuthorityRecord::new(authority_id, 2, 1000);
        record.add_device("device1");

        let bytes = serialize_authority(&record).unwrap();
        let restored = deserialize_authority(&bytes).unwrap();

        assert_eq!(restored.authority_id, record.authority_id);
        assert_eq!(restored.threshold, record.threshold);
        assert_eq!(restored.devices, record.devices);
        assert_eq!(restored.created_ms, record.created_ms);
    }
}
