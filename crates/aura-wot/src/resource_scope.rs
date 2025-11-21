//! Authority-based resource scopes for authorization
//!
//! This module defines resource scopes in terms of AuthorityId and ContextId,
//! replacing the device-centric model with authority-centric authorization.

use aura_core::{AuthorityId, ContextId};
use serde::{Deserialize, Serialize};

/// Resource scope for authority-based authorization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceScope {
    /// Operations on an authority's state
    Authority {
        authority_id: AuthorityId,
        operation: AuthorityOp,
    },
    /// Operations within a relational context
    Context {
        context_id: ContextId,
        operation: ContextOp,
    },
    /// Storage access scoped to an authority
    Storage {
        authority_id: AuthorityId,
        path: String,
    },
    /// Recovery operations (legacy - maps to Context)
    #[deprecated(note = "Use ResourceScope::Context instead")]
    Recovery { recovery_type: String },
    /// Journal operations (legacy - maps to Authority)
    #[deprecated(note = "Use ResourceScope::Authority instead")]
    Journal {
        account_id: String,
        operation: String,
    },
}

/// Operations that can be performed on an authority
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityOp {
    /// Update the commitment tree structure
    UpdateTree,
    /// Add a new device to the authority
    AddDevice,
    /// Remove a device from the authority
    RemoveDevice,
    /// Rotate authority keys
    Rotate,
    /// Add a guardian to the authority
    AddGuardian,
    /// Remove a guardian from the authority
    RemoveGuardian,
    /// Modify the threshold signature requirements
    ModifyThreshold,
    /// Revoke a device from the authority
    RevokeDevice,
}

/// Operations within a relational context
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextOp {
    /// Add a binding to the context
    AddBinding,
    /// Approve a recovery operation
    ApproveRecovery,
    /// Update context parameters
    UpdateParams,
    /// Recover device key within context
    RecoverDeviceKey,
    /// Recover account access within context
    RecoverAccountAccess,
    /// Update guardian set within context
    UpdateGuardianSet,
    /// Emergency freeze within context
    EmergencyFreeze,
}

impl ResourceScope {
    /// Convert to Datalog pattern for Biscuit evaluation
    pub fn to_datalog_pattern(&self) -> String {
        match self {
            ResourceScope::Authority {
                authority_id,
                operation,
            } => {
                format!(
                    "resource(\"/authority/{}/{}\"), resource_type(\"authority\")",
                    authority_id,
                    operation.as_str()
                )
            }
            ResourceScope::Context {
                context_id,
                operation,
            } => {
                format!(
                    "resource(\"/context/{}/{}\"), resource_type(\"context\")",
                    context_id,
                    operation.as_str()
                )
            }
            ResourceScope::Storage { authority_id, path } => {
                format!(
                    "resource(\"/storage/{}/{}\"), resource_type(\"storage\")",
                    authority_id, path
                )
            }
            #[allow(deprecated)]
            ResourceScope::Recovery { recovery_type } => {
                format!(
                    "resource(\"/recovery/{}\"), resource_type(\"recovery\")",
                    recovery_type
                )
            }
            #[allow(deprecated)]
            ResourceScope::Journal {
                account_id,
                operation,
            } => {
                format!(
                    "resource(\"/journal/{}/{}\"), resource_type(\"journal\")",
                    account_id, operation
                )
            }
        }
    }

    /// Get the resource pattern for this scope
    pub fn resource_pattern(&self) -> String {
        match self {
            ResourceScope::Authority {
                authority_id,
                operation,
            } => {
                format!("/authority/{}/{}", authority_id, operation.as_str())
            }
            ResourceScope::Context {
                context_id,
                operation,
            } => {
                format!("/context/{}/{}", context_id, operation.as_str())
            }
            ResourceScope::Storage { authority_id, path } => {
                format!("/storage/{}/{}", authority_id, path)
            }
            #[allow(deprecated)]
            ResourceScope::Recovery { recovery_type } => {
                format!("/recovery/{}", recovery_type)
            }
            #[allow(deprecated)]
            ResourceScope::Journal {
                account_id,
                operation,
            } => {
                format!("/journal/{}/{}", account_id, operation)
            }
        }
    }
}

impl AuthorityOp {
    /// Get string representation of the operation
    pub fn as_str(&self) -> &str {
        match self {
            AuthorityOp::UpdateTree => "update_tree",
            AuthorityOp::AddDevice => "add_device",
            AuthorityOp::RemoveDevice => "remove_device",
            AuthorityOp::Rotate => "rotate",
            AuthorityOp::AddGuardian => "add_guardian",
            AuthorityOp::RemoveGuardian => "remove_guardian",
            AuthorityOp::ModifyThreshold => "modify_threshold",
            AuthorityOp::RevokeDevice => "revoke_device",
        }
    }
}

impl ContextOp {
    /// Get string representation of the operation
    pub fn as_str(&self) -> &str {
        match self {
            ContextOp::AddBinding => "add_binding",
            ContextOp::ApproveRecovery => "approve_recovery",
            ContextOp::UpdateParams => "update_params",
            ContextOp::RecoverDeviceKey => "recover_device_key",
            ContextOp::RecoverAccountAccess => "recover_account_access",
            ContextOp::UpdateGuardianSet => "update_guardian_set",
            ContextOp::EmergencyFreeze => "emergency_freeze",
        }
    }
}

/// Legacy resource scopes for backward compatibility during migration
/// TODO: Remove once all code is updated to use the new ResourceScope
pub mod legacy {
    use super::*;

    /// Map legacy storage category to authority-based scope
    pub fn storage_scope(authority_id: AuthorityId, category: &str, path: &str) -> ResourceScope {
        ResourceScope::Storage {
            authority_id,
            path: format!("{}/{}", category, path),
        }
    }

    /// Map legacy journal operations to authority operations
    pub fn journal_to_authority_op(op: &str) -> Option<AuthorityOp> {
        match op {
            "write" => Some(AuthorityOp::UpdateTree),
            "add_device" => Some(AuthorityOp::AddDevice),
            "remove_device" => Some(AuthorityOp::RemoveDevice),
            _ => None,
        }
    }

    /// Map legacy admin operations to authority operations
    pub fn admin_to_authority_op(admin_op: &str) -> Option<AuthorityOp> {
        match admin_op {
            "add_guardian" => Some(AuthorityOp::AddGuardian),
            "remove_guardian" => Some(AuthorityOp::RemoveGuardian),
            "modify_threshold" => Some(AuthorityOp::ModifyThreshold),
            "revoke_device" => Some(AuthorityOp::RevokeDevice),
            _ => None,
        }
    }

    /// Map legacy recovery types to context operations
    pub fn recovery_to_context_op(recovery_type: &str) -> Option<ContextOp> {
        match recovery_type {
            "device_key" => Some(ContextOp::RecoverDeviceKey),
            "account_access" => Some(ContextOp::RecoverAccountAccess),
            "guardian_set" => Some(ContextOp::UpdateGuardianSet),
            "emergency_freeze" => Some(ContextOp::EmergencyFreeze),
            _ => None,
        }
    }

    /// Convert legacy biscuit ResourceScope to new authority-based ResourceScope
    pub fn convert_legacy_resource_scope(
        legacy: &crate::biscuit_resources::ResourceScope,
        default_authority: AuthorityId,
        default_context: ContextId,
    ) -> ResourceScope {
        match legacy {
            crate::biscuit_resources::ResourceScope::Storage { category, path } => {
                storage_scope(default_authority, category.as_str(), path)
            }
            crate::biscuit_resources::ResourceScope::Journal {
                account_id,
                operation,
            } => {
                if let Some(auth_op) = journal_to_authority_op(operation.as_str()) {
                    ResourceScope::Authority {
                        authority_id: default_authority,
                        operation: auth_op,
                    }
                } else {
                    // Fallback to legacy Journal variant
                    #[allow(deprecated)]
                    ResourceScope::Journal {
                        account_id: account_id.clone(),
                        operation: operation.as_str().to_string(),
                    }
                }
            }
            crate::biscuit_resources::ResourceScope::Admin { operation } => {
                if let Some(auth_op) = admin_to_authority_op(operation.as_str()) {
                    ResourceScope::Authority {
                        authority_id: default_authority,
                        operation: auth_op,
                    }
                } else {
                    // Fallback for unknown operations
                    ResourceScope::Authority {
                        authority_id: default_authority,
                        operation: AuthorityOp::UpdateTree,
                    }
                }
            }
            crate::biscuit_resources::ResourceScope::Recovery { recovery_type } => {
                if let Some(ctx_op) = recovery_to_context_op(recovery_type.as_str()) {
                    ResourceScope::Context {
                        context_id: default_context,
                        operation: ctx_op,
                    }
                } else {
                    // Fallback to legacy Recovery variant
                    #[allow(deprecated)]
                    ResourceScope::Recovery {
                        recovery_type: recovery_type.as_str().to_string(),
                    }
                }
            }
            crate::biscuit_resources::ResourceScope::Relay { channel_id: _ } => {
                // Map relay to context operation (relays operate within contexts)
                ResourceScope::Context {
                    context_id: default_context,
                    operation: ContextOp::AddBinding, // Relay channels involve binding
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authority_scope_datalog() {
        let scope = ResourceScope::Authority {
            authority_id: AuthorityId::new(),
            operation: AuthorityOp::UpdateTree,
        };

        let pattern = scope.to_datalog_pattern();
        assert!(pattern.contains("resource_type(\"authority\")"));
        assert!(pattern.contains("update_tree"));
    }

    #[test]
    fn test_context_scope_pattern() {
        let scope = ResourceScope::Context {
            context_id: ContextId::new(),
            operation: ContextOp::ApproveRecovery,
        };

        let pattern = scope.resource_pattern();
        assert!(pattern.starts_with("/context/"));
        assert!(pattern.ends_with("/approve_recovery"));
    }

    #[test]
    fn test_new_authority_operations() {
        // Test new admin operations
        assert_eq!(AuthorityOp::AddGuardian.as_str(), "add_guardian");
        assert_eq!(AuthorityOp::RemoveGuardian.as_str(), "remove_guardian");
        assert_eq!(AuthorityOp::ModifyThreshold.as_str(), "modify_threshold");
        assert_eq!(AuthorityOp::RevokeDevice.as_str(), "revoke_device");
    }

    #[test]
    fn test_new_context_operations() {
        // Test new recovery operations
        assert_eq!(ContextOp::RecoverDeviceKey.as_str(), "recover_device_key");
        assert_eq!(
            ContextOp::RecoverAccountAccess.as_str(),
            "recover_account_access"
        );
        assert_eq!(ContextOp::UpdateGuardianSet.as_str(), "update_guardian_set");
        assert_eq!(ContextOp::EmergencyFreeze.as_str(), "emergency_freeze");
    }

    #[test]
    fn test_legacy_conversion_helpers() {
        use super::legacy::*;

        // Test admin operation mapping
        assert_eq!(
            admin_to_authority_op("add_guardian"),
            Some(AuthorityOp::AddGuardian)
        );
        assert_eq!(admin_to_authority_op("unknown"), None);

        // Test recovery type mapping
        assert_eq!(
            recovery_to_context_op("device_key"),
            Some(ContextOp::RecoverDeviceKey)
        );
        assert_eq!(recovery_to_context_op("unknown"), None);

        // Test journal operation mapping
        assert_eq!(
            journal_to_authority_op("write"),
            Some(AuthorityOp::UpdateTree)
        );
        assert_eq!(journal_to_authority_op("unknown"), None);
    }
}
