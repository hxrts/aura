//! Authorization helpers for frontends.
//!
//! This module provides portable authorization checking for commands and operations.
//! It includes:
//! - `CommandAuthorizationLevel`: Classification of commands by sensitivity
//! - `require_*` helpers: Pre-check functions for authorization

use crate::{views::home::HomeRole, StateSnapshot};
use aura_core::AuraError;

// ============================================================================
// Command Authorization Levels
// ============================================================================

/// Authorization level required for a command.
///
/// Commands are classified by sensitivity level, with each level
/// requiring progressively stronger authorization:
/// - **Public**: No authorization required (read-only, status queries)
/// - **Basic**: User token required (normal user operations)
/// - **Sensitive**: Elevated authorization (account modifications)
/// - **Admin**: Moderator/admin capabilities (privileged operations)
///
/// Levels are ordered: Public < Basic < Sensitive < Admin
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandAuthorizationLevel {
    /// No authorization required - read-only/status operations
    Public,
    /// Basic user token required - normal messaging and channels
    Basic,
    /// Elevated authorization - account/device modifications
    Sensitive,
    /// Admin/moderator capabilities - moderation and privileged ops
    Admin,
}

impl CommandAuthorizationLevel {
    /// Check if this level requires any authorization.
    #[inline]
    pub fn requires_auth(&self) -> bool {
        !matches!(self, Self::Public)
    }

    /// Check if this level requires admin privileges.
    #[inline]
    pub fn requires_admin(&self) -> bool {
        matches!(self, Self::Admin)
    }

    /// Check if this level requires elevated (sensitive) authorization.
    #[inline]
    pub fn requires_elevated(&self) -> bool {
        matches!(self, Self::Sensitive | Self::Admin)
    }

    /// Get human-readable description for UI display.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Public => "public access",
            Self::Basic => "user authentication",
            Self::Sensitive => "elevated authorization",
            Self::Admin => "administrator privileges",
        }
    }

    /// Get a short label for logging/display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Basic => "BASIC",
            Self::Sensitive => "SENSITIVE",
            Self::Admin => "ADMIN",
        }
    }
}

impl std::fmt::Display for CommandAuthorizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

// ============================================================================
// Role-Based Authorization Checks
// ============================================================================

/// Require elevated home role in the current home.
pub fn require_admin(snapshot: Option<&StateSnapshot>, operation: &str) -> Result<(), AuraError> {
    let role = snapshot.and_then(|s| s.homes.current_home().map(|h| h.my_role));
    match role {
        Some(HomeRole::Moderator | HomeRole::Member) => Ok(()),
        Some(HomeRole::Participant) => Err(AuraError::permission_denied(format!(
            "{operation} requires administrator privileges",
        ))),
        None => Err(AuraError::permission_denied(format!(
            "{operation} requires a home context",
        ))),
    }
}

/// Check authorization level against a user's role.
///
/// Returns an error message if the authorization check fails.
pub fn check_authorization_level(
    level: CommandAuthorizationLevel,
    role: Option<HomeRole>,
    operation_name: &str,
) -> Result<(), String> {
    match level {
        CommandAuthorizationLevel::Public
        | CommandAuthorizationLevel::Basic
        | CommandAuthorizationLevel::Sensitive => Ok(()),
        CommandAuthorizationLevel::Admin => match role {
            Some(HomeRole::Moderator | HomeRole::Member) => Ok(()),
            Some(HomeRole::Participant) => Err(format!(
                "Permission denied: {operation_name} requires administrator privileges"
            )),
            None => Err(format!(
                "Permission denied: {operation_name} requires a home context"
            )),
        },
    }
}

/// Get the minimum role required for a command authorization level.
pub fn minimum_role_for_level(level: CommandAuthorizationLevel) -> Option<HomeRole> {
    match level {
        CommandAuthorizationLevel::Public | CommandAuthorizationLevel::Basic => None,
        CommandAuthorizationLevel::Sensitive => Some(HomeRole::Participant),
        CommandAuthorizationLevel::Admin => Some(HomeRole::Moderator),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authorization_level_ordering() {
        assert!(CommandAuthorizationLevel::Public < CommandAuthorizationLevel::Basic);
        assert!(CommandAuthorizationLevel::Basic < CommandAuthorizationLevel::Sensitive);
        assert!(CommandAuthorizationLevel::Sensitive < CommandAuthorizationLevel::Admin);
    }

    #[test]
    fn test_requires_auth() {
        assert!(!CommandAuthorizationLevel::Public.requires_auth());
        assert!(CommandAuthorizationLevel::Basic.requires_auth());
        assert!(CommandAuthorizationLevel::Sensitive.requires_auth());
        assert!(CommandAuthorizationLevel::Admin.requires_auth());
    }

    #[test]
    fn test_requires_admin() {
        assert!(!CommandAuthorizationLevel::Public.requires_admin());
        assert!(!CommandAuthorizationLevel::Basic.requires_admin());
        assert!(!CommandAuthorizationLevel::Sensitive.requires_admin());
        assert!(CommandAuthorizationLevel::Admin.requires_admin());
    }

    #[test]
    fn test_requires_elevated() {
        assert!(!CommandAuthorizationLevel::Public.requires_elevated());
        assert!(!CommandAuthorizationLevel::Basic.requires_elevated());
        assert!(CommandAuthorizationLevel::Sensitive.requires_elevated());
        assert!(CommandAuthorizationLevel::Admin.requires_elevated());
    }

    #[test]
    fn test_level_descriptions() {
        assert_eq!(
            CommandAuthorizationLevel::Public.description(),
            "public access"
        );
        assert_eq!(CommandAuthorizationLevel::Admin.label(), "ADMIN");
    }

    #[test]
    fn test_check_authorization_level_public() {
        assert!(check_authorization_level(CommandAuthorizationLevel::Public, None, "Test").is_ok());
    }

    #[test]
    fn test_check_authorization_level_admin() {
        // Admin level fails without admin role
        assert!(check_authorization_level(
            CommandAuthorizationLevel::Admin,
            Some(HomeRole::Participant),
            "Kick"
        )
        .is_err());

        // Admin level succeeds with admin role
        assert!(check_authorization_level(
            CommandAuthorizationLevel::Admin,
            Some(HomeRole::Moderator),
            "Kick"
        )
        .is_ok());

        // Admin level succeeds with member role
        assert!(check_authorization_level(
            CommandAuthorizationLevel::Admin,
            Some(HomeRole::Member),
            "Kick"
        )
        .is_ok());
    }

    #[test]
    fn test_minimum_role_for_level() {
        assert_eq!(
            minimum_role_for_level(CommandAuthorizationLevel::Public),
            None
        );
        assert_eq!(
            minimum_role_for_level(CommandAuthorizationLevel::Basic),
            None
        );
        assert_eq!(
            minimum_role_for_level(CommandAuthorizationLevel::Sensitive),
            Some(HomeRole::Participant)
        );
        assert_eq!(
            minimum_role_for_level(CommandAuthorizationLevel::Admin),
            Some(HomeRole::Moderator)
        );
    }
}
