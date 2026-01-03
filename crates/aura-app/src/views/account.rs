//! # Account View Types
//!
//! Portable account-related types for backup/restore operations.
//! These types are FFI-safe and can be used across all frontends.

use serde::{Deserialize, Serialize};

/// Current backup format version
pub const BACKUP_VERSION: u32 = 1;

/// Backup format prefix for identification
pub const BACKUP_PREFIX: &str = "aura:backup:v1:";

/// Account configuration data (portable representation)
///
/// This is the portable representation of account configuration
/// that can be serialized for backup/restore operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountConfig {
    /// The authority ID for this account (hex-encoded)
    pub authority_id: String,
    /// The primary context ID for this account (hex-encoded)
    pub context_id: String,
    /// User display name for this device
    #[serde(default)]
    pub display_name: Option<String>,
    /// Account creation timestamp (ms since epoch)
    pub created_at: u64,
}

impl AccountConfig {
    /// Create a new account configuration
    pub fn new(
        authority_id: String,
        context_id: String,
        display_name: Option<String>,
        created_at: u64,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            display_name,
            created_at,
        }
    }
}

/// Complete account backup data structure
///
/// Contains all data needed to restore an account on a new device:
/// - Account configuration (authority_id, context_id, created_at)
/// - Journal facts (all state history)
/// - Backup metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBackup {
    /// Backup format version
    pub version: u32,
    /// Account configuration
    pub account: AccountConfig,
    /// Journal content (JSON string of all facts)
    pub journal: Option<String>,
    /// Backup creation timestamp (ms since epoch)
    pub backup_at: u64,
    /// Device ID that created the backup (informational only)
    pub source_device: Option<String>,
}

impl AccountBackup {
    /// Create a new account backup
    pub fn new(
        account: AccountConfig,
        journal: Option<String>,
        backup_at: u64,
        source_device: Option<String>,
    ) -> Self {
        Self {
            version: BACKUP_VERSION,
            account,
            journal,
            backup_at,
            source_device,
        }
    }

    /// Encode the backup as a portable backup code string
    ///
    /// Format: `aura:backup:v1:<base64>`
    pub fn encode(&self) -> Result<String, String> {
        let json =
            serde_json::to_string(self).map_err(|e| format!("Failed to serialize backup: {e}"))?;

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());

        Ok(format!("{BACKUP_PREFIX}{encoded}"))
    }

    /// Decode a backup code string into an AccountBackup
    pub fn decode(backup_code: &str) -> Result<Self, String> {
        if !backup_code.starts_with(BACKUP_PREFIX) {
            return Err(format!(
                "Invalid backup code format (expected prefix '{BACKUP_PREFIX}')"
            ));
        }

        let encoded = &backup_code[BACKUP_PREFIX.len()..];

        use base64::Engine;
        let json_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("Invalid backup code encoding: {e}"))?;

        let json =
            String::from_utf8(json_bytes).map_err(|e| format!("Invalid backup code UTF-8: {e}"))?;

        let backup: AccountBackup =
            serde_json::from_str(&json).map_err(|e| format!("Invalid backup format: {e}"))?;

        Ok(backup)
    }

    /// Validate the backup structure
    ///
    /// Checks:
    /// - Version compatibility
    /// - Authority ID format
    /// - Context ID format
    pub fn validate(&self) -> Result<(), String> {
        // Check version compatibility
        if self.version > BACKUP_VERSION {
            return Err(format!(
                "Backup version {} is newer than supported version {}",
                self.version, BACKUP_VERSION
            ));
        }

        // Validate authority_id format (should be 32 hex chars = 16 bytes)
        if self.account.authority_id.len() != 32 {
            return Err(format!(
                "Invalid authority_id length: expected 32 hex chars, got {}",
                self.account.authority_id.len()
            ));
        }
        hex::decode(&self.account.authority_id)
            .map_err(|e| format!("Invalid authority_id hex: {e}"))?;

        // Validate context_id format (should be 32 hex chars = 16 bytes)
        if self.account.context_id.len() != 32 {
            return Err(format!(
                "Invalid context_id length: expected 32 hex chars, got {}",
                self.account.context_id.len()
            ));
        }
        hex::decode(&self.account.context_id)
            .map_err(|e| format!("Invalid context_id hex: {e}"))?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn create_test_config() -> AccountConfig {
        AccountConfig::new(
            "0123456789abcdef0123456789abcdef".to_string(),
            "fedcba9876543210fedcba9876543210".to_string(),
            Some("Test User".to_string()),
            1234567890000,
        )
    }

    #[test]
    fn test_account_config_new() {
        let config = create_test_config();
        assert_eq!(config.authority_id, "0123456789abcdef0123456789abcdef");
        assert_eq!(config.display_name, Some("Test User".to_string()));
    }

    #[test]
    fn test_backup_encode_decode_roundtrip() {
        let config = create_test_config();
        let backup = AccountBackup::new(
            config,
            Some("{}".to_string()),
            1234567890000,
            Some("test-device".to_string()),
        );

        let encoded = backup.encode().expect("encode should succeed");
        assert!(encoded.starts_with(BACKUP_PREFIX));

        let decoded = AccountBackup::decode(&encoded).expect("decode should succeed");
        assert_eq!(decoded.version, BACKUP_VERSION);
        assert_eq!(decoded.account.authority_id, backup.account.authority_id);
        assert_eq!(decoded.source_device, Some("test-device".to_string()));
    }

    #[test]
    fn test_backup_decode_invalid_prefix() {
        let result = AccountBackup::decode("invalid:prefix:data");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected prefix"));
    }

    #[test]
    fn test_backup_validate_success() {
        let config = create_test_config();
        let backup = AccountBackup::new(config, None, 0, None);
        assert!(backup.validate().is_ok());
    }

    #[test]
    fn test_backup_validate_version_too_new() {
        let config = create_test_config();
        let mut backup = AccountBackup::new(config, None, 0, None);
        backup.version = BACKUP_VERSION + 1;
        let result = backup.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("newer than supported"));
    }

    #[test]
    fn test_backup_validate_invalid_authority_id() {
        let mut config = create_test_config();
        config.authority_id = "invalid".to_string();
        let backup = AccountBackup::new(config, None, 0, None);
        let result = backup.validate();
        assert!(result.is_err());
    }
}
