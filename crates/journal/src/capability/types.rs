// Core types for convergent capabilities

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Deterministic capability identifier (BLAKE3 hash of parent chain)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub [u8; 32]);

impl CapabilityId {
    /// Generate deterministic ID from parent chain
    pub fn from_chain(
        parent_id: Option<&CapabilityId>,
        subject_id: &Subject,
        scope: &CapabilityScope,
    ) -> Self {
        let mut hasher = blake3::Hasher::new();

        if let Some(parent) = parent_id {
            hasher.update(&parent.0);
        }

        hasher.update(subject_id.as_bytes());
        hasher.update(&serde_json::to_vec(scope).unwrap_or_default());

        CapabilityId(hasher.finalize().into())
    }

    pub fn as_hex(&self) -> String {
        hex::encode(self.0)
    }
}

/// Subject of a capability (who it's granted to)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Subject(pub String);

impl Subject {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Capability scope defines what operations are authorized
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityScope {
    /// Operation namespace (e.g., "mls", "storage", "admin")
    pub namespace: String,
    /// Specific operation (e.g., "member", "write", "delegate")
    pub operation: String,
    /// Optional resource constraint (e.g., group ID, file path)
    pub resource: Option<String>,
    /// Additional parameters
    pub params: BTreeMap<String, String>,
}

impl CapabilityScope {
    /// Create a simple scope
    pub fn simple(namespace: &str, operation: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            operation: operation.to_string(),
            resource: None,
            params: BTreeMap::new(),
        }
    }

    /// Create scope with resource constraint
    pub fn with_resource(namespace: &str, operation: &str, resource: &str) -> Self {
        Self {
            namespace: namespace.to_string(),
            operation: operation.to_string(),
            resource: Some(resource.to_string()),
            params: BTreeMap::new(),
        }
    }

    /// Check if this scope subsumes another (is more general)
    pub fn subsumes(&self, other: &CapabilityScope) -> bool {
        // Namespace must match
        if self.namespace != other.namespace {
            return false;
        }

        // Operation must match or be wildcard
        if self.operation != "*" && self.operation != other.operation {
            return false;
        }

        // Resource constraint: None means no constraint (subsumes all)
        if let Some(self_resource) = &self.resource {
            if let Some(other_resource) = &other.resource {
                if self_resource != other_resource {
                    return false;
                }
            } else {
                // Other has no resource constraint but we do
                return false;
            }
        }

        // Check parameters (simplified - exact match required)
        for (key, value) in &other.params {
            if self.params.get(key) != Some(value) {
                return false;
            }
        }

        true
    }
}

/// Capability evaluation result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityResult {
    /// Capability is valid and grants access
    Granted,
    /// Capability is revoked
    Revoked,
    /// Capability is expired
    Expired,
    /// No matching capability found
    NotFound,
}

/// Union type for capability events used in CRDT synchronization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityEvent {
    /// Capability delegation event
    Delegation(crate::capability::events::CapabilityDelegation),
    /// Capability revocation event
    Revocation(crate::capability::events::CapabilityRevocation),
}
