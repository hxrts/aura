//! Capability scopes and permission levels

use crate::{Action, Resource};
use serde::{Deserialize, Serialize};

/// Scope that defines what resources a capability can access
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CapabilityScope {
    /// Global scope - can access any resource (dangerous!)
    Global,

    /// Account-scoped - can access resources within a specific account
    Account { account_id: aura_types::AccountId },

    /// Device-scoped - can access resources of a specific device
    Device { device_id: aura_types::DeviceId },

    /// Storage-scoped - can access specific storage objects
    Storage { object_ids: Vec<uuid::Uuid> },

    /// Protocol-scoped - can access specific protocol sessions
    Protocol { session_types: Vec<String> },

    /// Custom scope with arbitrary constraints
    Custom {
        scope_type: String,
        constraints: serde_json::Value,
    },
}

/// Permission level that determines what actions are allowed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum PermissionLevel {
    /// No permissions
    None = 0,

    /// Read-only access
    Read = 1,

    /// Read and write access
    ReadWrite = 2,

    /// Read, write, and delete access
    ReadWriteDelete = 3,

    /// Full access including administrative operations
    Admin = 4,
}

impl CapabilityScope {
    /// Check if this scope allows access to a specific resource
    pub fn allows_resource(&self, resource: &Resource) -> bool {
        match (self, resource) {
            (CapabilityScope::Global, _) => true,

            (CapabilityScope::Account { account_id }, Resource::Account(res_account_id)) => {
                account_id == res_account_id
            }

            (CapabilityScope::Account { account_id }, Resource::StorageObject { owner, .. }) => {
                account_id == owner
            }

            (CapabilityScope::Device { device_id }, Resource::Device(res_device_id)) => {
                device_id == res_device_id
            }

            (
                CapabilityScope::Storage { object_ids },
                Resource::StorageObject { object_id, .. },
            ) => object_ids.contains(object_id),

            (
                CapabilityScope::Protocol { session_types },
                Resource::ProtocolSession { session_type, .. },
            ) => session_types.contains(session_type),

            _ => false,
        }
    }

    /// Create a more restrictive scope from this one
    pub fn restrict_to(&self, restriction: &CapabilityScope) -> Option<CapabilityScope> {
        match (self, restriction) {
            // Global scope can be restricted to anything
            (CapabilityScope::Global, restriction) => Some(restriction.clone()),

            // Account scope can be restricted to devices or storage within that account
            (CapabilityScope::Account { account_id: _ }, CapabilityScope::Device { .. }) => {
                // Would need to verify device belongs to account
                Some(restriction.clone())
            }

            (CapabilityScope::Account { account_id: _ }, CapabilityScope::Storage { .. }) => {
                // Would need to verify storage belongs to account
                Some(restriction.clone())
            }

            // Same scopes with same parameters
            (scope, restriction) if scope == restriction => Some(scope.clone()),

            // Cannot restrict to broader scope
            _ => None,
        }
    }
}

impl PermissionLevel {
    /// Get the actions allowed by this permission level
    pub fn allowed_actions(&self) -> Vec<Action> {
        match self {
            PermissionLevel::None => vec![],
            PermissionLevel::Read => vec![Action::Read],
            PermissionLevel::ReadWrite => vec![Action::Read, Action::Write],
            PermissionLevel::ReadWriteDelete => vec![Action::Read, Action::Write, Action::Delete],
            PermissionLevel::Admin => vec![
                Action::Read,
                Action::Write,
                Action::Delete,
                Action::Execute,
                Action::Delegate,
                Action::Revoke,
                Action::Admin,
            ],
        }
    }

    /// Check if this permission level allows a specific action
    pub fn allows_action(&self, action: &Action) -> bool {
        self.allowed_actions().contains(action)
    }

    /// Create a permission level from a list of actions
    pub fn from_actions(actions: &[Action]) -> Self {
        if actions.contains(&Action::Admin) {
            return PermissionLevel::Admin;
        }

        if actions.contains(&Action::Delete) || actions.contains(&Action::Revoke) {
            return PermissionLevel::ReadWriteDelete;
        }

        if actions.contains(&Action::Write) || actions.contains(&Action::Execute) {
            return PermissionLevel::ReadWrite;
        }

        if actions.contains(&Action::Read) {
            return PermissionLevel::Read;
        }

        PermissionLevel::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_capability_scope_allows_resource() {
        let effects = Effects::test();
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let account_scope = CapabilityScope::Account { account_id };
        let device_scope = CapabilityScope::Device { device_id };
        let global_scope = CapabilityScope::Global;

        let account_resource = Resource::Account(account_id);
        let device_resource = Resource::Device(device_id);

        // Account scope should allow account resource
        assert!(account_scope.allows_resource(&account_resource));
        assert!(!account_scope.allows_resource(&device_resource));

        // Device scope should allow device resource
        assert!(device_scope.allows_resource(&device_resource));
        assert!(!device_scope.allows_resource(&account_resource));

        // Global scope should allow everything
        assert!(global_scope.allows_resource(&account_resource));
        assert!(global_scope.allows_resource(&device_resource));
    }

    #[test]
    fn test_permission_level_actions() {
        assert_eq!(PermissionLevel::None.allowed_actions().len(), 0);
        assert_eq!(PermissionLevel::Read.allowed_actions().len(), 1);
        assert_eq!(PermissionLevel::ReadWrite.allowed_actions().len(), 2);
        assert_eq!(PermissionLevel::ReadWriteDelete.allowed_actions().len(), 3);
        assert!(PermissionLevel::Admin.allowed_actions().len() >= 7);

        assert!(PermissionLevel::ReadWrite.allows_action(&Action::Read));
        assert!(PermissionLevel::ReadWrite.allows_action(&Action::Write));
        assert!(!PermissionLevel::ReadWrite.allows_action(&Action::Delete));

        assert!(PermissionLevel::Admin.allows_action(&Action::Admin));
    }

    #[test]
    fn test_permission_level_from_actions() {
        let read_actions = vec![Action::Read];
        let write_actions = vec![Action::Read, Action::Write];
        let admin_actions = vec![Action::Read, Action::Write, Action::Admin];

        assert_eq!(
            PermissionLevel::from_actions(&read_actions),
            PermissionLevel::Read
        );
        assert_eq!(
            PermissionLevel::from_actions(&write_actions),
            PermissionLevel::ReadWrite
        );
        assert_eq!(
            PermissionLevel::from_actions(&admin_actions),
            PermissionLevel::Admin
        );
    }

    #[test]
    fn test_scope_restriction() {
        let effects = Effects::test();
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let global_scope = CapabilityScope::Global;
        let account_scope = CapabilityScope::Account { account_id };
        let device_scope = CapabilityScope::Device { device_id };

        // Global can be restricted to account
        assert!(global_scope.restrict_to(&account_scope).is_some());

        // Account can be restricted to device (conceptually)
        assert!(account_scope.restrict_to(&device_scope).is_some());

        // Same scopes should work
        assert!(account_scope.restrict_to(&account_scope).is_some());
    }
}
