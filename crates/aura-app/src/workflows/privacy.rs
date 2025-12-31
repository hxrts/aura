//! # Privacy Utilities
//!
//! Portable privacy-related functions for normalizing and anonymizing identifiers.
//!
//! ## Design
//!
//! These functions are pure utilities that:
//! - Normalize context names and identifiers for consistent lookup
//! - Anonymize peer IDs and authority IDs for privacy-preserving display
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_app::workflows::privacy::{normalize_context_name, anonymize_peer_id};
//!
//! let normalized = normalize_context_name("  MyContext  ");
//! assert_eq!(normalized, "mycontext");
//!
//! let anon = anonymize_peer_id("peer-12345");
//! assert!(anon.starts_with("anon-"));
//! ```

use aura_core::hash;
use aura_core::identifiers::AuthorityId;

/// Normalize a context name for consistent lookup.
///
/// Trims whitespace and converts to lowercase.
///
/// # Examples
///
/// ```rust
/// use aura_app::ui::workflows::privacy::normalize_context_name;
///
/// assert_eq!(normalize_context_name("  MyContext  "), "mycontext");
/// assert_eq!(normalize_context_name("UPPERCASE"), "uppercase");
/// assert_eq!(normalize_context_name("mixed Case"), "mixed case");
/// ```
#[inline]
pub fn normalize_context_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

/// Normalize a channel name for consistent lookup.
///
/// Trims whitespace and converts to lowercase. Strips leading '#' if present.
///
/// # Examples
///
/// ```rust
/// use aura_app::ui::workflows::privacy::normalize_channel_name;
///
/// assert_eq!(normalize_channel_name("#general"), "general");
/// assert_eq!(normalize_channel_name("  #General  "), "general");
/// assert_eq!(normalize_channel_name("random"), "random");
/// ```
#[inline]
pub fn normalize_channel_name(name: &str) -> String {
    let trimmed = name.trim();
    let stripped = trimmed.strip_prefix('#').unwrap_or(trimmed);
    stripped.to_ascii_lowercase()
}

/// Length of the anonymized ID suffix (hex characters).
const ANONYMIZED_ID_LENGTH: usize = 12;

/// Anonymize a peer ID for privacy-preserving display.
///
/// Creates a hash-based identifier that is:
/// - Deterministic (same input always produces same output)
/// - One-way (cannot recover original from anonymized form)
/// - Readable (includes "anon-" prefix)
///
/// # Examples
///
/// ```rust
/// use aura_app::ui::workflows::privacy::anonymize_peer_id;
///
/// let anon = anonymize_peer_id("peer-12345");
/// assert!(anon.starts_with("anon-"));
/// assert_eq!(anon.len(), 5 + 12); // "anon-" + 12 hex chars
///
/// // Same input produces same output
/// assert_eq!(anonymize_peer_id("peer-12345"), anonymize_peer_id("peer-12345"));
///
/// // Different input produces different output
/// assert_ne!(anonymize_peer_id("peer-12345"), anonymize_peer_id("peer-67890"));
/// ```
pub fn anonymize_peer_id(peer_id: &str) -> String {
    anonymize_string(peer_id)
}

/// Anonymize an authority ID for privacy-preserving display.
///
/// Same behavior as `anonymize_peer_id` but accepts `AuthorityId` directly.
///
/// # Examples
///
/// ```rust,ignore
/// use aura_app::workflows::privacy::anonymize_authority_id;
/// use aura_core::identifiers::AuthorityId;
///
/// let auth_id: AuthorityId = /* ... */;
/// let anon = anonymize_authority_id(&auth_id);
/// assert!(anon.starts_with("anon-"));
/// ```
pub fn anonymize_authority_id(authority_id: &AuthorityId) -> String {
    anonymize_string(&authority_id.to_string())
}

/// Anonymize any string value for privacy-preserving display.
///
/// Core implementation shared by all anonymization functions.
fn anonymize_string(value: &str) -> String {
    let mut hasher = hash::hasher();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(&digest[..ANONYMIZED_ID_LENGTH]);
    format!("anon-{}", &hex[..ANONYMIZED_ID_LENGTH])
}

/// Check if a string looks like an anonymized ID.
///
/// Returns true if the string starts with "anon-" and has the expected format.
///
/// # Examples
///
/// ```rust
/// use aura_app::ui::workflows::privacy::is_anonymized;
///
/// assert!(is_anonymized("anon-1a2b3c4d5e6f"));
/// assert!(!is_anonymized("peer-12345"));
/// assert!(!is_anonymized("anon-short"));
/// ```
pub fn is_anonymized(value: &str) -> bool {
    value.starts_with("anon-") && value.len() == 5 + ANONYMIZED_ID_LENGTH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_context_name_basic() {
        assert_eq!(normalize_context_name("context"), "context");
        assert_eq!(normalize_context_name("Context"), "context");
        assert_eq!(normalize_context_name("CONTEXT"), "context");
    }

    #[test]
    fn test_normalize_context_name_whitespace() {
        assert_eq!(normalize_context_name("  context  "), "context");
        assert_eq!(normalize_context_name("\tcontext\n"), "context");
        assert_eq!(normalize_context_name("  mixed Case  "), "mixed case");
    }

    #[test]
    fn test_normalize_context_name_empty() {
        assert_eq!(normalize_context_name(""), "");
        assert_eq!(normalize_context_name("   "), "");
    }

    #[test]
    fn test_normalize_channel_name_basic() {
        assert_eq!(normalize_channel_name("general"), "general");
        assert_eq!(normalize_channel_name("#general"), "general");
        assert_eq!(normalize_channel_name("#General"), "general");
    }

    #[test]
    fn test_normalize_channel_name_whitespace() {
        assert_eq!(normalize_channel_name("  #general  "), "general");
        assert_eq!(normalize_channel_name("  general  "), "general");
    }

    #[test]
    fn test_normalize_channel_name_no_double_strip() {
        // Should only strip one leading #
        assert_eq!(normalize_channel_name("##general"), "#general");
    }

    #[test]
    fn test_anonymize_peer_id_format() {
        let anon = anonymize_peer_id("peer-12345");
        assert!(anon.starts_with("anon-"));
        assert_eq!(anon.len(), 5 + ANONYMIZED_ID_LENGTH);
    }

    #[test]
    fn test_anonymize_peer_id_deterministic() {
        let anon1 = anonymize_peer_id("peer-12345");
        let anon2 = anonymize_peer_id("peer-12345");
        assert_eq!(anon1, anon2);
    }

    #[test]
    fn test_anonymize_peer_id_different_inputs() {
        let anon1 = anonymize_peer_id("peer-12345");
        let anon2 = anonymize_peer_id("peer-67890");
        assert_ne!(anon1, anon2);
    }

    #[test]
    fn test_anonymize_peer_id_empty() {
        let anon = anonymize_peer_id("");
        assert!(anon.starts_with("anon-"));
        assert_eq!(anon.len(), 5 + ANONYMIZED_ID_LENGTH);
    }

    #[test]
    fn test_is_anonymized_valid() {
        let anon = anonymize_peer_id("test");
        assert!(is_anonymized(&anon));
    }

    #[test]
    fn test_is_anonymized_invalid() {
        assert!(!is_anonymized("peer-12345"));
        assert!(!is_anonymized("anon-short"));
        assert!(!is_anonymized("anon-"));
        assert!(!is_anonymized(""));
    }

    #[test]
    fn test_is_anonymized_wrong_prefix() {
        assert!(!is_anonymized("annon-1a2b3c4d5e6f"));
        assert!(!is_anonymized("Anon-1a2b3c4d5e6f"));
    }
}
