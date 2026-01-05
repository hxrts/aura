//! Name resolution trait for entities with nickname/suggestion semantics.
//!
//! # Invariants
//!
//! - Resolution order is always: nickname → nickname_suggestion → fallback_id
//! - Empty strings are treated as absent (not "set to empty")
//! - The trait provides a default `effective_name()` implementation that all
//!   implementors share, ensuring consistent behavior across entity types
//!
//! # Category
//!
//! All operations in this module are read-only views (no facts, no effects).

#![forbid(unsafe_code)]

/// Trait for entities with nickname/suggestion resolution.
///
/// Provides consistent name resolution across contacts, devices, peers,
/// and any future entity types with the same pattern.
///
/// # Resolution Order
///
/// 1. Local nickname (user-assigned override) if non-empty
/// 2. Shared nickname_suggestion (what entity wants to be called) if non-empty
/// 3. Fallback identifier (e.g., truncated authority/device ID)
///
/// # Example
///
/// ```
/// use aura_app::views::naming::EffectiveName;
///
/// struct MyEntity {
///     nickname: Option<String>,
///     suggestion: Option<String>,
///     id: String,
/// }
///
/// impl EffectiveName for MyEntity {
///     fn nickname(&self) -> Option<&str> {
///         self.nickname.as_deref().filter(|s| !s.is_empty())
///     }
///
///     fn nickname_suggestion(&self) -> Option<&str> {
///         self.suggestion.as_deref().filter(|s| !s.is_empty())
///     }
///
///     fn fallback_id(&self) -> String {
///         if self.id.len() > 12 {
///             format!("{}...{}", &self.id[..6], &self.id[self.id.len()-4..])
///         } else {
///             self.id.clone()
///         }
///     }
/// }
///
/// let entity = MyEntity {
///     nickname: Some("Alice".to_string()),
///     suggestion: Some("Al".to_string()),
///     id: "abc123def456".to_string(),
/// };
///
/// // Nickname takes precedence
/// assert_eq!(entity.effective_name(), "Alice");
/// ```
pub trait EffectiveName {
    /// The local nickname (user-assigned override), if any.
    ///
    /// Returns `None` if no nickname is set. Implementors should return
    /// `None` for empty strings rather than `Some("")`.
    fn nickname(&self) -> Option<&str>;

    /// The shared nickname suggestion (what the entity wants to be called), if any.
    ///
    /// Returns `None` if no suggestion is set. Implementors should return
    /// `None` for empty strings rather than `Some("")`.
    fn nickname_suggestion(&self) -> Option<&str>;

    /// A stable fallback identifier (e.g., authority ID, device ID).
    ///
    /// This should be deterministic and suitable for display when no
    /// nickname or suggestion is available. Typically a truncated ID.
    fn fallback_id(&self) -> String;

    /// Get the best-effort display name.
    ///
    /// Uses the standard resolution order: nickname → suggestion → fallback.
    /// This default implementation should NOT be overridden to ensure
    /// consistent behavior across all entity types.
    #[must_use]
    fn effective_name(&self) -> String {
        if let Some(nick) = self.nickname() {
            if !nick.is_empty() {
                return nick.to_string();
            }
        }
        if let Some(suggestion) = self.nickname_suggestion() {
            if !suggestion.is_empty() {
                return suggestion.to_string();
            }
        }
        self.fallback_id()
    }

    /// Check if this entity has a user-assigned nickname.
    #[must_use]
    fn has_nickname(&self) -> bool {
        self.nickname().is_some_and(|s| !s.is_empty())
    }

    /// Check if this entity has a shared nickname suggestion.
    #[must_use]
    fn has_nickname_suggestion(&self) -> bool {
        self.nickname_suggestion().is_some_and(|s| !s.is_empty())
    }

    /// Check if effective_name() will return a fallback ID.
    #[must_use]
    fn is_using_fallback(&self) -> bool {
        !self.has_nickname() && !self.has_nickname_suggestion()
    }
}

/// Truncate an ID for display (first 6 chars + "..." + last 4 chars).
///
/// Returns the full ID if it's 16 characters or shorter.
///
/// # Example
///
/// ```
/// use aura_app::views::naming::truncate_id_for_display;
///
/// assert_eq!(truncate_id_for_display("abc"), "abc");
/// assert_eq!(truncate_id_for_display("0123456789abcdef0123"), "012345...0123");
/// ```
#[must_use]
pub fn truncate_id_for_display(id: &str) -> String {
    if id.len() > 16 {
        format!("{}...{}", &id[..6], &id[id.len() - 4..])
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEntity {
        nickname: Option<String>,
        suggestion: Option<String>,
        id: String,
    }

    impl EffectiveName for TestEntity {
        fn nickname(&self) -> Option<&str> {
            self.nickname.as_deref().filter(|s| !s.is_empty())
        }

        fn nickname_suggestion(&self) -> Option<&str> {
            self.suggestion.as_deref().filter(|s| !s.is_empty())
        }

        fn fallback_id(&self) -> String {
            truncate_id_for_display(&self.id)
        }
    }

    #[test]
    fn test_nickname_takes_precedence() {
        let entity = TestEntity {
            nickname: Some("Alice".to_string()),
            suggestion: Some("Al".to_string()),
            id: "abc123def456".to_string(),
        };
        assert_eq!(entity.effective_name(), "Alice");
    }

    #[test]
    fn test_suggestion_when_no_nickname() {
        let entity = TestEntity {
            nickname: None,
            suggestion: Some("Al".to_string()),
            id: "abc123def456".to_string(),
        };
        assert_eq!(entity.effective_name(), "Al");
    }

    #[test]
    fn test_fallback_when_no_names() {
        let entity = TestEntity {
            nickname: None,
            suggestion: None,
            id: "abc123def456".to_string(),
        };
        assert_eq!(entity.effective_name(), "abc123def456");
    }

    #[test]
    fn test_empty_string_treated_as_none() {
        let entity = TestEntity {
            nickname: Some("".to_string()),
            suggestion: Some("".to_string()),
            id: "abc123".to_string(),
        };
        assert_eq!(entity.effective_name(), "abc123");
        assert!(!entity.has_nickname());
        assert!(!entity.has_nickname_suggestion());
        assert!(entity.is_using_fallback());
    }

    #[test]
    fn test_truncate_long_id() {
        let long_id = "0123456789abcdef0123456789";
        assert_eq!(truncate_id_for_display(long_id), "012345...6789");
    }

    #[test]
    fn test_truncate_short_id() {
        let short_id = "abc123";
        assert_eq!(truncate_id_for_display(short_id), "abc123");
    }
}
