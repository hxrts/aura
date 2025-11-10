//! Type conversions between different architectural layers
//!
//! This module provides explicit conversion traits between types from different
//! crates and architectural layers, enabling clear cross-layer communication.

use crate::permissions::CanonicalPermission;

/// Conversion trait for permissions across external authorization systems
///
/// Converts between the canonical Permission type in aura-core and
/// the authorization-layer specific variants.
pub trait FromAuthorizationPermission: Sized {
    /// Error type for conversion failures
    type Error: std::fmt::Display;

    /// Convert from external authorization Action to canonical Permission
    fn from_authorization(action: String) -> Result<Self, Self::Error>;
}

/// Conversion trait for permissions from the aura-journal layer
///
/// Converts between the canonical Permission type in aura-core and
/// the journal-layer domain-specific variants (Storage, Communication, Relay).
pub trait FromJournalPermission: Sized {
    /// Error type for conversion failures
    type Error: std::fmt::Display;

    /// Convert from journal-specific permission to canonical Permission
    fn from_journal_permission(
        category: &str,
        operation: &str,
        resource: &str,
    ) -> Result<Self, Self::Error>;
}

/// Conversion implementations for canonical Permission type
impl FromAuthorizationPermission for CanonicalPermission {
    type Error = String;

    fn from_authorization(action: String) -> Result<Self, Self::Error> {
        match action.as_str() {
            "read" => Ok(CanonicalPermission::StorageRead),
            "write" => Ok(CanonicalPermission::StorageWrite),
            "delete" => Ok(CanonicalPermission::StorageDelete),
            "execute" => Ok(CanonicalPermission::ProtocolExecute),
            "delegate" => Ok(CanonicalPermission::Admin),
            "revoke" => Ok(CanonicalPermission::Admin),
            "admin" => Ok(CanonicalPermission::Admin),
            custom => Ok(CanonicalPermission::Custom(custom.to_string())),
        }
    }
}

impl FromJournalPermission for CanonicalPermission {
    type Error = String;

    fn from_journal_permission(
        category: &str,
        operation: &str,
        _resource: &str,
    ) -> Result<Self, Self::Error> {
        match (category, operation) {
            // Storage operations
            ("storage", "read") | ("storage", "retrieve") => Ok(CanonicalPermission::StorageRead),
            ("storage", "write") | ("storage", "store") => Ok(CanonicalPermission::StorageWrite),
            ("storage", "delete") => Ok(CanonicalPermission::StorageDelete),
            ("storage", "replicate") => Ok(CanonicalPermission::StorageRead), // Replication requires read

            // Communication operations (map to ProtocolExecute)
            ("communication", "send") => Ok(CanonicalPermission::ProtocolExecute),
            ("communication", "receive") => Ok(CanonicalPermission::ProtocolExecute),
            ("communication", "subscribe") => Ok(CanonicalPermission::ProtocolExecute),

            // Relay operations
            ("relay", "forward") => Ok(CanonicalPermission::ProtocolExecute),
            ("relay", "store") => Ok(CanonicalPermission::StorageWrite),
            ("relay", "announce") => Ok(CanonicalPermission::Admin),

            // Custom or unknown
            (cat, op) => Ok(CanonicalPermission::Custom(format!("{}:{}", cat, op))),
        }
    }
}

/// Helper function to convert canonical Permission to authorization action name
pub fn permission_to_action(permission: &CanonicalPermission) -> String {
    match permission {
        CanonicalPermission::StorageRead => "read".to_string(),
        CanonicalPermission::StorageWrite => "write".to_string(),
        CanonicalPermission::StorageDelete => "delete".to_string(),
        CanonicalPermission::ProtocolExecute => "execute".to_string(),
        CanonicalPermission::Admin => "admin".to_string(),
        CanonicalPermission::Custom(custom) => custom.clone(),
    }
}

/// Helper function to convert canonical Permission to journal operation
///
/// Returns a tuple of (category, operation) suitable for journal-specific permissions
pub fn permission_to_journal(permission: &CanonicalPermission) -> (String, String) {
    match permission {
        CanonicalPermission::StorageRead => ("storage".to_string(), "read".to_string()),
        CanonicalPermission::StorageWrite => ("storage".to_string(), "write".to_string()),
        CanonicalPermission::StorageDelete => ("storage".to_string(), "delete".to_string()),
        CanonicalPermission::ProtocolExecute => ("protocol".to_string(), "execute".to_string()),
        CanonicalPermission::Admin => ("admin".to_string(), "all".to_string()),
        CanonicalPermission::Custom(custom) => {
            // Try to parse custom format "category:operation"
            if let Some((cat, op)) = custom.split_once(':') {
                (cat.to_string(), op.to_string())
            } else {
                ("custom".to_string(), custom.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authorization_permission_conversion() {
        assert_eq!(
            CanonicalPermission::from_authorization("read".to_string()).unwrap(),
            CanonicalPermission::StorageRead
        );

        assert_eq!(
            CanonicalPermission::from_authorization("write".to_string()).unwrap(),
            CanonicalPermission::StorageWrite
        );

        assert_eq!(
            CanonicalPermission::from_authorization("admin".to_string()).unwrap(),
            CanonicalPermission::Admin
        );
    }

    #[test]
    fn test_journal_permission_conversion() {
        assert_eq!(
            CanonicalPermission::from_journal_permission("storage", "read", "file1").unwrap(),
            CanonicalPermission::StorageRead
        );

        assert_eq!(
            CanonicalPermission::from_journal_permission("communication", "send", "alice").unwrap(),
            CanonicalPermission::ProtocolExecute
        );

        assert_eq!(
            CanonicalPermission::from_journal_permission("relay", "forward", "bob").unwrap(),
            CanonicalPermission::ProtocolExecute
        );
    }

    #[test]
    fn test_permission_to_action() {
        assert_eq!(
            permission_to_action(&CanonicalPermission::StorageRead),
            "read"
        );
        assert_eq!(
            permission_to_action(&CanonicalPermission::StorageWrite),
            "write"
        );
        assert_eq!(permission_to_action(&CanonicalPermission::Admin), "admin");
    }

    #[test]
    fn test_permission_to_journal() {
        assert_eq!(
            permission_to_journal(&CanonicalPermission::StorageRead),
            ("storage".to_string(), "read".to_string())
        );

        assert_eq!(
            permission_to_journal(&CanonicalPermission::ProtocolExecute),
            ("protocol".to_string(), "execute".to_string())
        );

        assert_eq!(
            permission_to_journal(&CanonicalPermission::Admin),
            ("admin".to_string(), "all".to_string())
        );
    }

    #[test]
    fn test_roundtrip_conversion() {
        let original = CanonicalPermission::StorageRead;
        let action = permission_to_action(&original);
        let converted = CanonicalPermission::from_authorization(action).unwrap();
        assert_eq!(original, converted);
    }
}
