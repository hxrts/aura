//! Resource scopes for authorization
//!
//! This module defines resource scopes in terms of AuthorityId and ContextId,
//! providing a foundation for authority-based authorization across all layers.
//! These types were moved from aura-wot to eliminate improper domain coupling.

use crate::{AuthorityId, ContextId};
use serde::{Deserialize, Serialize};

/// Resource scope for authority-based authorization
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}

/// Operations that can be performed on an authority
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authority_scope_datalog() {
        let scope = ResourceScope::Authority {
            authority_id: AuthorityId::new_from_entropy([40u8; 32]),
            operation: AuthorityOp::UpdateTree,
        };

        let pattern = scope.to_datalog_pattern();
        assert!(pattern.contains("resource_type(\"authority\")"));
        assert!(pattern.contains("update_tree"));
    }

    #[test]
    fn test_context_scope_pattern() {
        let scope = ResourceScope::Context {
            context_id: ContextId::new_from_entropy([41u8; 32]),
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
}
