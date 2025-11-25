//! Type conversions between different architectural layers
//!
//! This module provides explicit conversion traits between types from different
//! crates and architectural layers, enabling clear cross-layer communication.

use crate::journal::Cap;

// =============================================================================
// CAP-BASED CONVERSION UTILITIES
// =============================================================================

/// Conversion trait for capabilities across external authorization systems
///
/// Converts between authorization action strings and the Cap capability system.
pub trait FromAuthorizationAction: Sized {
    /// Error type for conversion failures
    type Error: std::fmt::Display;

    /// Convert from external authorization action to Cap
    fn from_authorization_action(action: String) -> Result<Self, Self::Error>;
}

/// Conversion trait for capabilities from the aura-journal layer
///
/// Converts between journal-specific operations and the Cap capability system.
pub trait FromJournalOperation: Sized {
    /// Error type for conversion failures
    type Error: std::fmt::Display;

    /// Convert from journal-specific operation to Cap
    fn from_journal_operation(
        category: &str,
        operation: &str,
        resource: &str,
    ) -> Result<Self, Self::Error>;
}

/// Cap-based implementations
impl FromAuthorizationAction for Cap {
    type Error = String;

    fn from_authorization_action(action: String) -> Result<Self, Self::Error> {
        match action.as_str() {
            "read" => Ok(Cap::with_permissions(vec!["storage:read".to_string()])),
            "write" => Ok(Cap::with_permissions(vec!["storage:write".to_string()])),
            "delete" => Ok(Cap::with_permissions(vec!["storage:delete".to_string()])),
            "execute" => Ok(Cap::with_permissions(vec!["protocol:execute".to_string()])),
            "delegate" | "revoke" | "admin" => Ok(Cap::top()),
            custom => Ok(Cap::with_permissions(vec![format!("custom:{}", custom)])),
        }
    }
}

impl FromJournalOperation for Cap {
    type Error = String;

    fn from_journal_operation(
        category: &str,
        operation: &str,
        _resource: &str,
    ) -> Result<Self, Self::Error> {
        let capability_string = match (category, operation) {
            // Storage operations
            ("storage", "read") | ("storage", "retrieve") => "storage:read",
            ("storage", "write") | ("storage", "store") => "storage:write",
            ("storage", "delete") => "storage:delete",
            ("storage", "replicate") => "storage:read", // Replication requires read

            // Communication operations
            ("communication", "send") => "protocol:execute",
            ("communication", "receive") => "protocol:execute",
            ("communication", "subscribe") => "protocol:execute",

            // Relay operations
            ("relay", "forward") => "protocol:execute",
            ("relay", "store") => "storage:write",
            ("relay", "announce") => "admin", // Announcement requires admin

            // Custom or unknown
            (cat, op) => return Ok(Cap::with_permissions(vec![format!("{}:{}", cat, op)])),
        };

        if capability_string == "admin" {
            Ok(Cap::top())
        } else {
            Ok(Cap::with_permissions(vec![capability_string.to_string()]))
        }
    }
}

/// Helper function to convert Cap to authorization action name
pub fn cap_to_action(cap: &Cap) -> String {
    if cap.allows("*") {
        "admin".to_string()
    } else if cap.allows("storage:delete") {
        "delete".to_string()
    } else if cap.allows("storage:write") {
        "write".to_string()
    } else if cap.allows("storage:read") {
        "read".to_string()
    } else if cap.allows("protocol:execute") {
        "execute".to_string()
    } else {
        "custom".to_string()
    }
}

/// Helper function to convert Cap to journal operation
///
/// Returns a tuple of (category, operation) suitable for journal-specific operations
pub fn cap_to_journal(cap: &Cap) -> (String, String) {
    if cap.allows("*") {
        ("admin".to_string(), "all".to_string())
    } else if cap.allows("storage:delete") {
        ("storage".to_string(), "delete".to_string())
    } else if cap.allows("storage:write") {
        ("storage".to_string(), "write".to_string())
    } else if cap.allows("storage:read") {
        ("storage".to_string(), "read".to_string())
    } else if cap.allows("protocol:execute") {
        ("protocol".to_string(), "execute".to_string())
    } else {
        ("custom".to_string(), "unknown".to_string())
    }
}

#[cfg(test)]
mod cap_conversion_tests {
    use super::*;
    use crate::Cap;

    #[test]
    fn cap_to_journal_defaults_to_admin_all() {
        // Current Cap implementation treats empty tokens as permissive; verify mapping is stable.
        let cap = Cap::new();
        let (category, op) = cap_to_journal(&cap);
        assert_eq!(category, "admin");
        assert_eq!(op, "all");
    }
}
