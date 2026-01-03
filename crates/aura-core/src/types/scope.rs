//! Resource scopes for authorization
//!
//! This module defines resource scopes in terms of AuthorityId and ContextId,
//! providing a foundation for authority-based authorization across all layers.
//! These types were moved from aura-authorization to eliminate improper domain coupling.

use crate::{AuthorityId, ContextId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

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
        path: StoragePath,
    },
}

/// Validated storage path for authorization scopes.
///
/// Paths are normalized to use forward slashes, with empty segments removed.
/// Wildcards (`*`) are allowed only as the terminal segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StoragePath(String);

/// Errors when parsing or constructing a StoragePath.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StoragePathError {
    /// Path is empty after normalization.
    #[error("storage path is empty")]
    Empty,
    /// Path contains a `.` or `..` segment.
    #[error("storage path contains traversal segment")]
    TraversalSegment,
    /// Path contains control or whitespace characters.
    #[error("storage path contains invalid characters")]
    InvalidCharacters,
    /// Wildcard must be the terminal segment and appear only once.
    #[error("storage path wildcard must be a single terminal segment")]
    WildcardNotTerminal,
    /// Wildcard must occupy an entire segment.
    #[error("storage path wildcard must be the entire segment")]
    InvalidWildcard,
}

impl StoragePath {
    /// Parse and normalize a storage path.
    pub fn parse(input: &str) -> Result<Self, StoragePathError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(StoragePathError::Empty);
        }

        let mut segments = Vec::new();
        for segment in trimmed.split('/') {
            if segment.is_empty() {
                continue;
            }
            if segment == "." || segment == ".." {
                return Err(StoragePathError::TraversalSegment);
            }
            if segment.chars().any(|c| c.is_control() || c.is_whitespace()) {
                return Err(StoragePathError::InvalidCharacters);
            }
            if segment.contains('*') && segment != "*" {
                return Err(StoragePathError::InvalidWildcard);
            }
            segments.push(segment);
        }

        if segments.is_empty() {
            return Err(StoragePathError::Empty);
        }

        let wildcard_positions: Vec<_> = segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| **segment == "*")
            .collect();
        if wildcard_positions.len() > 1 {
            return Err(StoragePathError::WildcardNotTerminal);
        }
        if let Some((index, _)) = wildcard_positions.first() {
            if *index + 1 != segments.len() {
                return Err(StoragePathError::WildcardNotTerminal);
            }
        }

        Ok(Self(segments.join("/")))
    }

    /// Access the normalized storage path string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StoragePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for StoragePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for StoragePath {
    type Error = StoragePathError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        StoragePath::parse(&value)
    }
}

impl TryFrom<&str> for StoragePath {
    type Error = StoragePathError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        StoragePath::parse(value)
    }
}

impl From<StoragePath> for String {
    fn from(path: StoragePath) -> Self {
        path.0
    }
}

/// Canonical authorization operations used across guards and Biscuit policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthorizationOp {
    Read,
    Write,
    Update,
    Append,
    Delete,
    Execute,
    Admin,
    Attest,
    Delegate,
    Revoke,
    List,
    FlowCharge,
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
                    "resource(\"/storage/{authority_id}/{}\"), resource_type(\"storage\")",
                    path.as_str()
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
                format!("/storage/{authority_id}/{}", path.as_str())
            }
        }
    }

    /// Parse a string resource into a ResourceScope.
    pub fn parse(resource: &str) -> Result<Self, ResourceScopeParseError> {
        let trimmed = resource.trim();
        let path = trimmed
            .strip_prefix('/')
            .ok_or(ResourceScopeParseError::MissingLeadingSlash)?;
        let mut parts = path.splitn(3, '/');
        let scope_type = parts
            .next()
            .ok_or(ResourceScopeParseError::MissingSegments)?;
        let id_part = parts
            .next()
            .ok_or(ResourceScopeParseError::MissingSegments)?;
        let remainder = parts
            .next()
            .ok_or(ResourceScopeParseError::MissingSegments)?;

        match scope_type {
            "authority" => {
                let authority_id = AuthorityId::from_str(id_part).map_err(|_| {
                    ResourceScopeParseError::InvalidAuthorityId(id_part.to_string())
                })?;
                if remainder.contains('/') {
                    return Err(ResourceScopeParseError::InvalidOperation(
                        remainder.to_string(),
                    ));
                }
                let operation = AuthorityOp::parse(remainder).ok_or_else(|| {
                    ResourceScopeParseError::InvalidOperation(remainder.to_string())
                })?;
                Ok(ResourceScope::Authority {
                    authority_id,
                    operation,
                })
            }
            "context" => {
                let context_id = ContextId::from_str(id_part)
                    .map_err(|_| ResourceScopeParseError::InvalidContextId(id_part.to_string()))?;
                if remainder.contains('/') {
                    return Err(ResourceScopeParseError::InvalidOperation(
                        remainder.to_string(),
                    ));
                }
                let operation = ContextOp::parse(remainder).ok_or_else(|| {
                    ResourceScopeParseError::InvalidOperation(remainder.to_string())
                })?;
                Ok(ResourceScope::Context {
                    context_id,
                    operation,
                })
            }
            "storage" => {
                let authority_id = AuthorityId::from_str(id_part).map_err(|_| {
                    ResourceScopeParseError::InvalidAuthorityId(id_part.to_string())
                })?;
                let path = StoragePath::parse(remainder)?;
                Ok(ResourceScope::Storage { authority_id, path })
            }
            _ => Err(ResourceScopeParseError::UnknownScopeType(
                scope_type.to_string(),
            )),
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

    /// Parse an authority operation from its string representation.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "update_tree" => Some(AuthorityOp::UpdateTree),
            "add_device" => Some(AuthorityOp::AddDevice),
            "remove_device" => Some(AuthorityOp::RemoveDevice),
            "rotate" => Some(AuthorityOp::Rotate),
            "add_guardian" => Some(AuthorityOp::AddGuardian),
            "remove_guardian" => Some(AuthorityOp::RemoveGuardian),
            "modify_threshold" => Some(AuthorityOp::ModifyThreshold),
            "revoke_device" => Some(AuthorityOp::RevokeDevice),
            _ => None,
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

    /// Parse a context operation from its string representation.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "add_binding" => Some(ContextOp::AddBinding),
            "approve_recovery" => Some(ContextOp::ApproveRecovery),
            "update_params" => Some(ContextOp::UpdateParams),
            "recover_device_key" => Some(ContextOp::RecoverDeviceKey),
            "recover_account_access" => Some(ContextOp::RecoverAccountAccess),
            "update_guardian_set" => Some(ContextOp::UpdateGuardianSet),
            "emergency_freeze" => Some(ContextOp::EmergencyFreeze),
            _ => None,
        }
    }
}

impl AuthorizationOp {
    /// Get string representation of the authorization operation.
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthorizationOp::Read => "read",
            AuthorizationOp::Write => "write",
            AuthorizationOp::Update => "update",
            AuthorizationOp::Append => "append",
            AuthorizationOp::Delete => "delete",
            AuthorizationOp::Execute => "execute",
            AuthorizationOp::Admin => "admin",
            AuthorizationOp::Attest => "attest",
            AuthorizationOp::Delegate => "delegate",
            AuthorizationOp::Revoke => "revoke",
            AuthorizationOp::List => "list",
            AuthorizationOp::FlowCharge => "flow_charge",
        }
    }
}

/// Errors that can occur when parsing ResourceScope strings.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResourceScopeParseError {
    /// Resource scope string must start with a leading '/'.
    #[error("resource scope must start with '/'")]
    MissingLeadingSlash,
    /// Resource scope string does not contain the required segments.
    #[error("resource scope is missing required segments")]
    MissingSegments,
    /// Unknown scope prefix.
    #[error("unknown resource scope type '{0}'")]
    UnknownScopeType(String),
    /// Invalid authority identifier segment.
    #[error("invalid authority id '{0}'")]
    InvalidAuthorityId(String),
    /// Invalid context identifier segment.
    #[error("invalid context id '{0}'")]
    InvalidContextId(String),
    /// Invalid operation segment.
    #[error("invalid operation '{0}'")]
    InvalidOperation(String),
    /// Invalid storage path.
    #[error("invalid storage path: {0}")]
    InvalidStoragePath(#[from] StoragePathError),
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

    #[test]
    fn storage_path_normalization_and_validation() {
        let path = StoragePath::parse("/content//personal/user123/").unwrap();
        assert_eq!(path.as_str(), "content/personal/user123");
        assert!(StoragePath::parse("../secrets").is_err());
        assert!(StoragePath::parse("content/*/extra").is_err());
        assert!(StoragePath::parse("content/pa*th").is_err());
    }

    #[test]
    fn resource_scope_parse_round_trip() {
        let authority_id = AuthorityId::new_from_entropy([11u8; 32]);
        let context_id = ContextId::new_from_entropy([12u8; 32]);
        let storage_path = StoragePath::parse("namespace/personal/*").unwrap();

        let authority_scope = ResourceScope::Authority {
            authority_id,
            operation: AuthorityOp::UpdateTree,
        };
        let context_scope = ResourceScope::Context {
            context_id,
            operation: ContextOp::ApproveRecovery,
        };
        let storage_scope = ResourceScope::Storage {
            authority_id,
            path: storage_path,
        };

        let parsed_authority = ResourceScope::parse(&authority_scope.resource_pattern()).unwrap();
        let parsed_context = ResourceScope::parse(&context_scope.resource_pattern()).unwrap();
        let parsed_storage = ResourceScope::parse(&storage_scope.resource_pattern()).unwrap();

        assert_eq!(parsed_authority, authority_scope);
        assert_eq!(parsed_context, context_scope);
        assert_eq!(parsed_storage, storage_scope);
    }

    #[test]
    fn resource_scope_parse_rejects_invalid_inputs() {
        assert!(matches!(
            ResourceScope::parse("authority/invalid"),
            Err(ResourceScopeParseError::MissingLeadingSlash)
        ));
        assert!(matches!(
            ResourceScope::parse("/authority/not-a-uuid/update_tree"),
            Err(ResourceScopeParseError::InvalidAuthorityId(_))
        ));
        assert!(matches!(
            ResourceScope::parse("/context/context-123/unknown"),
            Err(ResourceScopeParseError::InvalidContextId(_))
        ));
        assert!(matches!(
            ResourceScope::parse("/storage/authority-00000000-0000-0000-0000-000000000000/../"),
            Err(ResourceScopeParseError::InvalidStoragePath(_))
        ));
    }
}
