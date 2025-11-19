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
}

/// Operations that can be performed on an authority
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorityOp {
    /// Update the ratchet tree structure
    UpdateTree,
    /// Add a new device to the authority
    AddDevice,
    /// Remove a device from the authority
    RemoveDevice,
    /// Rotate authority keys
    Rotate,
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
}
