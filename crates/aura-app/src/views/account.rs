//! # Account View Types
//!
//! Portable account-related types for backup/restore operations.
//! These types are FFI-safe and can be used across all frontends.

use aura_core::identifiers::{AuthorityId, ContextId};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

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
    /// The authority ID for this account.
    pub authority_id: AuthorityId,
    /// The primary context ID for this account.
    pub context_id: ContextId,
    /// Nickname suggestion (what the user wants to be called)
    #[serde(default)]
    pub nickname_suggestion: Option<String>,
    /// Account creation timestamp (ms since epoch)
    pub created_at: u64,
}

impl AccountConfig {
    /// Create a new account configuration
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        nickname_suggestion: Option<String>,
        created_at: u64,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            nickname_suggestion,
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

#[derive(Debug, Clone, Deserialize)]
struct LegacyAccountConfig {
    authority_id: String,
    context_id: String,
    #[serde(default)]
    nickname_suggestion: Option<String>,
    created_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyAccountBackup {
    version: u32,
    account: LegacyAccountConfig,
    journal: Option<String>,
    backup_at: u64,
    source_device: Option<String>,
}

impl TryFrom<LegacyAccountBackup> for AccountBackup {
    type Error = String;

    fn try_from(value: LegacyAccountBackup) -> Result<Self, Self::Error> {
        Ok(Self {
            version: value.version,
            account: AccountConfig {
                authority_id: parse_authority_id_compatible(&value.account.authority_id)?,
                context_id: parse_context_id_compatible(&value.account.context_id)?,
                nickname_suggestion: value.account.nickname_suggestion,
                created_at: value.account.created_at,
            },
            journal: value.journal,
            backup_at: value.backup_at,
            source_device: value.source_device,
        })
    }
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

        let backup = match serde_json::from_str::<AccountBackup>(&json) {
            Ok(backup) => backup,
            Err(parse_error) => {
                let legacy_backup: LegacyAccountBackup = serde_json::from_str(&json)
                    .map_err(|_| format!("Invalid backup format: {parse_error}"))?;
                AccountBackup::try_from(legacy_backup)
                    .map_err(|e| format!("Invalid backup format: {e}"))?
            }
        };

        Ok(backup)
    }

    /// Validate the backup structure
    ///
    /// Checks:
    /// - Version compatibility
    /// - Canonical identifier validity
    pub fn validate(&self) -> Result<(), String> {
        // Check version compatibility
        if self.version > BACKUP_VERSION {
            return Err(format!(
                "Backup version {} is newer than supported version {}",
                self.version, BACKUP_VERSION
            ));
        }

        // Verify string representations can round-trip through canonical parsers.
        parse_authority_id_compatible(&self.account.authority_id.to_string())?;
        parse_context_id_compatible(&self.account.context_id.to_string())?;

        Ok(())
    }
}

fn parse_legacy_uuid(raw: &str) -> Result<uuid::Uuid, String> {
    let bytes = hex::decode(raw).map_err(|e| format!("Invalid legacy hex identifier: {e}"))?;
    let uuid_bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| format!("Invalid legacy hex identifier length: {}", raw.len()))?;
    Ok(uuid::Uuid::from_bytes(uuid_bytes))
}

fn parse_authority_id_compatible(raw: &str) -> Result<AuthorityId, String> {
    AuthorityId::from_str(raw)
        .or_else(|_| parse_legacy_uuid(raw).map(AuthorityId::from_uuid))
        .map_err(|_| format!("Invalid authority_id format: {raw}"))
}

fn parse_context_id_compatible(raw: &str) -> Result<ContextId, String> {
    ContextId::from_str(raw)
        .or_else(|_| parse_legacy_uuid(raw).map(ContextId::from_uuid))
        .map_err(|_| format!("Invalid context_id format: {raw}"))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn create_test_config() -> AccountConfig {
        AccountConfig::new(
            AuthorityId::from_str("01234567-89ab-cdef-0123-456789abcdef").expect("valid UUID"),
            ContextId::from_str("fedcba98-7654-3210-fedc-ba9876543210").expect("valid UUID"),
            Some("Test User".to_string()),
            1234567890000,
        )
    }

    #[test]
    fn test_account_config_new() {
        let config = create_test_config();
        assert_eq!(
            config.authority_id.to_string(),
            "authority-01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(config.nickname_suggestion, Some("Test User".to_string()));
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
        let legacy_json = r#"{
            "version": 1,
            "account": {
                "authority_id": "invalid",
                "context_id": "fedcba9876543210fedcba9876543210",
                "nickname_suggestion": "test",
                "created_at": 0
            },
            "journal": null,
            "backup_at": 0,
            "source_device": null
        }"#;
        let encoded = {
            use base64::Engine;
            let payload = base64::engine::general_purpose::STANDARD.encode(legacy_json);
            format!("{BACKUP_PREFIX}{payload}")
        };
        let result = AccountBackup::decode(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn test_backup_decode_legacy_hex_ids() {
        let legacy_json = r#"{
            "version": 1,
            "account": {
                "authority_id": "0123456789abcdef0123456789abcdef",
                "context_id": "fedcba9876543210fedcba9876543210",
                "nickname_suggestion": "test",
                "created_at": 123
            },
            "journal": null,
            "backup_at": 456,
            "source_device": "legacy-device"
        }"#;

        let encoded = {
            use base64::Engine;
            let payload = base64::engine::general_purpose::STANDARD.encode(legacy_json);
            format!("{BACKUP_PREFIX}{payload}")
        };

        let decoded = AccountBackup::decode(&encoded).expect("legacy backup should decode");
        assert_eq!(
            decoded.account.authority_id.to_string(),
            "authority-01234567-89ab-cdef-0123-456789abcdef"
        );
        assert_eq!(
            decoded.account.context_id.to_string(),
            "context-fedcba98-7654-3210-fedc-ba9876543210"
        );
    }
}
