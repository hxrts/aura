//! Identifier newtypes for MPST domains.

use serde::{Deserialize, Serialize};

/// Role identifier used by choreographic annotations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoleId(String);

impl RoleId {
    /// Create a new role identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Get the underlying role name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RoleId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for RoleId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl std::fmt::Display for RoleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Session type identifier for protocol instances.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionTypeId(String);

impl SessionTypeId {
    /// Create a new session type identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Get the underlying session type identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SessionTypeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SessionTypeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl std::fmt::Display for SessionTypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
