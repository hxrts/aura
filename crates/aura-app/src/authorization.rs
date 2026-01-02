//! Authorization helpers for frontends.
//!
//! This module provides portable authorization checking for commands and operations.
//! It includes:
//! - `CommandAuthorizationLevel`: Classification of commands by sensitivity
//! - `CommandCapability` helpers: Role-based capability checking
//! - `require_*` helpers: Pre-check functions for authorization

use crate::{
    views::home::ResidentRole, workflows::chat_commands::CommandCapability, StateSnapshot,
};
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
/// - **Admin**: Steward/admin capabilities (privileged operations)
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
    /// Admin/steward capabilities - moderation and privileged ops
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

/// Require admin or owner role in the current home.
pub fn require_admin(snapshot: Option<&StateSnapshot>, operation: &str) -> Result<(), AuraError> {
    let role = snapshot.and_then(|s| s.homes.current_home().map(|h| h.my_role));
    match role {
        Some(ResidentRole::Admin | ResidentRole::Owner) => Ok(()),
        Some(ResidentRole::Resident) => Err(AuraError::agent(format!(
            "Permission denied: {operation} requires administrator privileges",
        ))),
        None => Err(AuraError::agent(format!(
            "Permission denied: {operation} requires a home context",
        ))),
    }
}

/// Check if the given role can use a command capability.
///
/// This is a pure function that checks role-to-capability mapping.
/// All users can use basic capabilities; admin capabilities require Admin/Owner role.
pub fn role_has_capability(role: Option<ResidentRole>, capability: &CommandCapability) -> bool {
    // None capability requires no permission
    if matches!(capability, CommandCapability::None) {
        return true;
    }

    // If no role (not in a home), only allow basic communication
    let Some(role) = role else {
        return matches!(
            capability,
            CommandCapability::SendDm | CommandCapability::UpdateContact
        );
    };

    match capability {
        CommandCapability::None => true,
        // Basic capabilities - all roles can use
        CommandCapability::SendDm
        | CommandCapability::SendMessage
        | CommandCapability::UpdateContact
        | CommandCapability::ViewMembers
        | CommandCapability::JoinChannel
        | CommandCapability::LeaveContext => true,
        // Moderation/admin capabilities - require Admin or Owner
        CommandCapability::ModerateKick
        | CommandCapability::ModerateBan
        | CommandCapability::ModerateMute
        | CommandCapability::Invite
        | CommandCapability::ManageChannel
        | CommandCapability::PinContent
        | CommandCapability::GrantSteward => {
            matches!(role, ResidentRole::Admin | ResidentRole::Owner)
        }
    }
}

/// Check authorization level against a user's role.
///
/// Returns an error message if the authorization check fails.
pub fn check_authorization_level(
    level: CommandAuthorizationLevel,
    role: Option<ResidentRole>,
    operation_name: &str,
) -> Result<(), String> {
    match level {
        CommandAuthorizationLevel::Public
        | CommandAuthorizationLevel::Basic
        | CommandAuthorizationLevel::Sensitive => Ok(()),
        CommandAuthorizationLevel::Admin => match role {
            Some(ResidentRole::Admin | ResidentRole::Owner) => Ok(()),
            Some(ResidentRole::Resident) => Err(format!(
                "Permission denied: {operation_name} requires administrator privileges"
            )),
            None => Err(format!(
                "Permission denied: {operation_name} requires a home context"
            )),
        },
    }
}

/// Get the minimum role required for a command authorization level.
pub fn minimum_role_for_level(level: CommandAuthorizationLevel) -> Option<ResidentRole> {
    match level {
        CommandAuthorizationLevel::Public | CommandAuthorizationLevel::Basic => None,
        CommandAuthorizationLevel::Sensitive => Some(ResidentRole::Resident),
        CommandAuthorizationLevel::Admin => Some(ResidentRole::Admin),
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
    fn test_role_has_capability_none() {
        assert!(role_has_capability(None, &CommandCapability::None));
        assert!(role_has_capability(
            Some(ResidentRole::Resident),
            &CommandCapability::None
        ));
    }

    #[test]
    fn test_role_has_capability_basic() {
        // All roles can send messages
        assert!(role_has_capability(
            Some(ResidentRole::Resident),
            &CommandCapability::SendMessage
        ));
        assert!(role_has_capability(
            Some(ResidentRole::Admin),
            &CommandCapability::SendMessage
        ));
        assert!(role_has_capability(
            Some(ResidentRole::Owner),
            &CommandCapability::SendMessage
        ));
    }

    #[test]
    fn test_role_has_capability_no_home() {
        // Without a role (not in home), only DM and contact updates allowed
        assert!(role_has_capability(None, &CommandCapability::SendDm));
        assert!(role_has_capability(None, &CommandCapability::UpdateContact));
        assert!(!role_has_capability(None, &CommandCapability::SendMessage));
        assert!(!role_has_capability(None, &CommandCapability::ModerateKick));
    }

    #[test]
    fn test_role_has_capability_moderation() {
        // Moderation requires Admin or Owner
        assert!(!role_has_capability(
            Some(ResidentRole::Resident),
            &CommandCapability::ModerateKick
        ));
        assert!(role_has_capability(
            Some(ResidentRole::Admin),
            &CommandCapability::ModerateKick
        ));
        assert!(role_has_capability(
            Some(ResidentRole::Owner),
            &CommandCapability::ModerateKick
        ));

        assert!(!role_has_capability(
            Some(ResidentRole::Resident),
            &CommandCapability::GrantSteward
        ));
        assert!(role_has_capability(
            Some(ResidentRole::Admin),
            &CommandCapability::GrantSteward
        ));
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
            Some(ResidentRole::Resident),
            "Kick"
        )
        .is_err());

        // Admin level succeeds with admin role
        assert!(check_authorization_level(
            CommandAuthorizationLevel::Admin,
            Some(ResidentRole::Admin),
            "Kick"
        )
        .is_ok());

        // Admin level succeeds with owner role
        assert!(check_authorization_level(
            CommandAuthorizationLevel::Admin,
            Some(ResidentRole::Owner),
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
            Some(ResidentRole::Resident)
        );
        assert_eq!(
            minimum_role_for_level(CommandAuthorizationLevel::Admin),
            Some(ResidentRole::Admin)
        );
    }
}
