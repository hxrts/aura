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
        // Note: These conversions cannot create real capabilities since they lack
        // the actual Biscuit token. Return empty capability for all actions.
        match action.as_str() {
            "read" | "write" | "delete" | "execute" | "delegate" | "revoke" | "admin" => {
                Ok(Cap::new())
            }
            _ => Ok(Cap::new()),
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
        let _capability_string = match (category, operation) {
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

            // Custom or unknown - return empty capability since we can't create real tokens
            (_cat, _op) => return Ok(Cap::new()),
        };

        // Note: Cannot create real capabilities since we lack Biscuit token context
        Ok(Cap::new())
    }
}

/// Helper function to convert Cap to authorization action name
pub fn cap_to_action(cap: &Cap) -> String {
    // Note: Cannot inspect capability contents since Cap is now just a token container.
    // Actual authorization decisions must be made through AuthorizationEffects.
    if cap.is_empty() {
        "none".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Helper function to convert Cap to journal operation
///
/// Returns a tuple of (category, operation) suitable for journal-specific operations
pub fn cap_to_journal(cap: &Cap) -> (String, String) {
    // Note: Cannot inspect capability contents since Cap is now just a token container.
    // Actual authorization decisions must be made through AuthorizationEffects.
    if cap.is_empty() {
        ("custom".to_string(), "unknown".to_string())
    } else {
        ("unknown".to_string(), "unknown".to_string())
    }
}

#[cfg(test)]
mod cap_conversion_tests {
    use super::*;
    use crate::Cap;

    #[test]
    fn cap_to_journal_defaults_to_custom_unknown() {
        // Empty Cap implementation returns false for all permissions
        let cap = Cap::new();
        let (category, op) = cap_to_journal(&cap);
        assert_eq!(category, "custom");
        assert_eq!(op, "unknown");
    }
}
