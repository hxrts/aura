//! Permission system for Aura components
//!
//! This module provides the canonical permission types used throughout the Aura system.
//! It serves as the single source of truth for all permission-related types and provides
//! conversion traits for mapping between different permission representations.

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Canonical permission enumeration used throughout Aura
///
/// This is the single source of truth for all permission concepts in the system.
/// All other permission representations (in authorization, journal, and capability systems)
/// convert to this canonical form for consistent handling.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CanonicalPermission {
    /// Permission to read storage data (level 1)
    StorageRead,

    /// Permission to write storage data (level 2, implies read)
    StorageWrite,

    /// Permission to delete storage data (level 3, implies write and read)
    StorageDelete,

    /// Permission to execute protocols (level 2)
    ProtocolExecute,

    /// Administrative permission - highest level (level 255, implies all)
    Admin,

    /// Custom permission with string identifier (level 1)
    Custom(String),
}

impl Display for CanonicalPermission {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonicalPermission::StorageRead => write!(f, "storage:read"),
            CanonicalPermission::StorageWrite => write!(f, "storage:write"),
            CanonicalPermission::StorageDelete => write!(f, "storage:delete"),
            CanonicalPermission::ProtocolExecute => write!(f, "protocol:execute"),
            CanonicalPermission::Admin => write!(f, "admin"),
            CanonicalPermission::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

impl CanonicalPermission {
    /// Check if this permission implies another permission
    pub fn implies(&self, other: &CanonicalPermission) -> bool {
        match (self, other) {
            // Admin implies all permissions
            (CanonicalPermission::Admin, _) => true,

            // Write implies read for storage
            (CanonicalPermission::StorageWrite, CanonicalPermission::StorageRead) => true,

            // Delete implies write and read for storage
            (CanonicalPermission::StorageDelete, CanonicalPermission::StorageWrite) => true,
            (CanonicalPermission::StorageDelete, CanonicalPermission::StorageRead) => true,

            // Exact match
            (a, b) if a == b => true,

            // No other implications
            _ => false,
        }
    }

    /// Get the permission level (higher numbers have more access)
    pub fn level(&self) -> u8 {
        match self {
            CanonicalPermission::StorageRead => 1,
            CanonicalPermission::StorageWrite => 2,
            CanonicalPermission::StorageDelete => 3,
            CanonicalPermission::ProtocolExecute => 2,
            CanonicalPermission::Admin => 255,
            CanonicalPermission::Custom(_) => 1,
        }
    }

    /// Check if this is a storage-related permission
    pub fn is_storage_permission(&self) -> bool {
        matches!(
            self,
            CanonicalPermission::StorageRead
                | CanonicalPermission::StorageWrite
                | CanonicalPermission::StorageDelete
        )
    }

    /// Check if this is an administrative permission
    pub fn is_admin_permission(&self) -> bool {
        matches!(self, CanonicalPermission::Admin)
    }

    /// Get all storage permissions
    pub fn storage_permissions() -> Vec<CanonicalPermission> {
        vec![
            CanonicalPermission::StorageRead,
            CanonicalPermission::StorageWrite,
            CanonicalPermission::StorageDelete,
        ]
    }

    /// Get all protocol permissions
    pub fn protocol_permissions() -> Vec<CanonicalPermission> {
        vec![CanonicalPermission::ProtocolExecute]
    }

    /// Parse permission from string
    pub fn parse(s: &str) -> Result<Self, PermissionError> {
        match s.to_lowercase().as_str() {
            "storage:read" | "read" => Ok(CanonicalPermission::StorageRead),
            "storage:write" | "write" => Ok(CanonicalPermission::StorageWrite),
            "storage:delete" | "delete" => Ok(CanonicalPermission::StorageDelete),
            "protocol:execute" | "execute" => Ok(CanonicalPermission::ProtocolExecute),
            "admin" => Ok(CanonicalPermission::Admin),
            s if s.starts_with("custom:") => {
                let name = s.strip_prefix("custom:").unwrap();
                Ok(CanonicalPermission::Custom(name.to_string()))
            }
            _ => Err(PermissionError::InvalidPermission(s.to_string())),
        }
    }
}

/// Permission set for managing collections of permissions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet {
    permissions: std::collections::HashSet<CanonicalPermission>,
}

impl PermissionSet {
    /// Create a new empty permission set
    pub fn new() -> Self {
        Self {
            permissions: std::collections::HashSet::new(),
        }
    }

    /// Create a permission set with the given permissions
    pub fn with_permissions(permissions: Vec<CanonicalPermission>) -> Self {
        Self {
            permissions: permissions.into_iter().collect(),
        }
    }

    /// Add a permission to the set
    pub fn add(&mut self, permission: CanonicalPermission) {
        self.permissions.insert(permission);
    }

    /// Remove a permission from the set
    pub fn remove(&mut self, permission: &CanonicalPermission) {
        self.permissions.remove(permission);
    }

    /// Check if the set contains a specific permission
    pub fn contains(&self, permission: &CanonicalPermission) -> bool {
        self.permissions.contains(permission)
    }

    /// Check if the set contains any permission that implies the given permission
    pub fn has_permission(&self, required: &CanonicalPermission) -> bool {
        self.permissions.iter().any(|p| p.implies(required))
    }

    /// Check if the set contains all required permissions
    pub fn has_all_permissions(&self, required: &[CanonicalPermission]) -> bool {
        required.iter().all(|req| self.has_permission(req))
    }

    /// Get all permissions in the set
    pub fn permissions(&self) -> Vec<&CanonicalPermission> {
        self.permissions.iter().collect()
    }

    /// Get the number of permissions in the set
    pub fn len(&self) -> usize {
        self.permissions.len()
    }

    /// Check if the permission set is empty
    pub fn is_empty(&self) -> bool {
        self.permissions.is_empty()
    }

    /// Merge another permission set into this one
    pub fn merge(&mut self, other: &PermissionSet) {
        for permission in &other.permissions {
            self.permissions.insert(permission.clone());
        }
    }

    /// Create intersection of two permission sets
    pub fn intersection(&self, other: &PermissionSet) -> PermissionSet {
        let permissions = self
            .permissions
            .intersection(&other.permissions)
            .cloned()
            .collect();
        PermissionSet { permissions }
    }

    /// Create union of two permission sets
    pub fn union(&self, other: &PermissionSet) -> PermissionSet {
        let permissions = self
            .permissions
            .union(&other.permissions)
            .cloned()
            .collect();
        PermissionSet { permissions }
    }
}

impl Default for PermissionSet {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<CanonicalPermission> for PermissionSet {
    fn from_iter<T: IntoIterator<Item = CanonicalPermission>>(iter: T) -> Self {
        Self {
            permissions: iter.into_iter().collect(),
        }
    }
}

impl IntoIterator for PermissionSet {
    type Item = CanonicalPermission;
    type IntoIter = std::collections::hash_set::IntoIter<CanonicalPermission>;

    fn into_iter(self) -> Self::IntoIter {
        self.permissions.into_iter()
    }
}

/// Permission context for authorization decisions
#[derive(Debug, Clone)]
pub struct PermissionContext {
    /// User or entity ID
    pub principal: String,

    /// Resource being accessed
    pub resource: String,

    /// Action being performed
    pub action: String,

    /// Additional context metadata
    pub metadata: std::collections::HashMap<String, String>,

    /// Timestamp of the permission check
    pub timestamp: std::time::Instant,
}

impl PermissionContext {
    /// Create a new permission context
    pub fn new(principal: &str, resource: &str, action: &str) -> Self {
        Self {
            principal: principal.to_string(),
            resource: resource.to_string(),
            action: action.to_string(),
            metadata: std::collections::HashMap::new(),
            #[allow(clippy::disallowed_methods)]
            timestamp: std::time::Instant::now(),
        }
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

/// Permission-related errors
#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    /// Permission string could not be parsed to a valid permission
    #[error("Invalid permission: {0}")]
    InvalidPermission(String),

    /// Principal does not have permission to perform the requested action
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Principal does not have all required permissions for the operation
    #[error("Insufficient permissions")]
    InsufficientPermissions,

    /// The specified permission was not found
    #[error("Permission not found: {0}")]
    NotFound(String),

    /// Conflicting permissions detected in the permission set
    #[error("Permission conflict: {0}")]
    Conflict(String),
}
