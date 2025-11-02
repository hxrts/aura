// Core types for convergent capabilities

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Re-export CapabilityId from aura-types
pub use aura_types::CapabilityId;

// Import authorization types for interoperability
use aura_authorization::{
    Action as AuthzAction, Resource as AuthzResource, Subject as AuthzSubject,
};

/// Subject of a capability (who it's granted to)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Subject {
    /// A device subject
    Device(aura_types::DeviceId),
    /// A guardian subject  
    Guardian(uuid::Uuid),
    /// System subject (for Keyhive delegations)
    System,
    /// Generic string subject (for backward compatibility)
    Generic(String),
}

impl Subject {
    pub fn new(id: &str) -> Self {
        // Try to parse as DeviceId first
        if let Ok(device_id) = id.parse::<aura_types::DeviceId>() {
            return Self::Device(device_id);
        }
        
        // Try to parse as UUID for Guardian
        if let Ok(uuid) = id.parse::<uuid::Uuid>() {
            return Self::Guardian(uuid);
        }
        
        // Check for system
        if id == "system" {
            return Self::System;
        }
        
        // Fall back to generic string
        Self::Generic(id.to_string())
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            Self::Device(device_id) => device_id.to_string().into_bytes(),
            Self::Guardian(uuid) => uuid.to_string().into_bytes(),
            Self::System => b"system".to_vec(),
            Self::Generic(s) => s.as_bytes().to_vec(),
        }
    }

    /// Convert to authorization crate Subject for interoperability
    pub fn to_authz_subject(&self) -> Option<AuthzSubject> {
        match self {
            Self::Device(device_id) => Some(AuthzSubject::Device(*device_id)),
            Self::Guardian(uuid) => Some(AuthzSubject::Guardian(*uuid)),
            Self::System => None, // System subject doesn't have authz equivalent
            Self::Generic(s) => {
                // Try to parse the string
                if let Ok(device_id) = s.parse::<aura_types::DeviceId>() {
                    return Some(AuthzSubject::Device(device_id));
                }
                if let Ok(uuid) = s.parse::<uuid::Uuid>() {
                    return Some(AuthzSubject::Guardian(uuid));
                }
                None
            }
        }
    }
}

impl From<AuthzSubject> for Subject {
    /// Convert from authorization Subject to journal Subject
    fn from(authz_subject: AuthzSubject) -> Self {
        match authz_subject {
            AuthzSubject::Device(device_id) => Subject::Device(device_id),
            AuthzSubject::Guardian(guardian_id) => Subject::Guardian(guardian_id),
            AuthzSubject::ThresholdGroup {
                participants,
                threshold,
            } => {
                // Create a deterministic string representation
                let mut ids: Vec<String> = participants.iter().map(|id| id.to_string()).collect();
                ids.sort();
                Subject::Generic(format!("threshold:{}:{}", threshold, ids.join(",")))
            }
            AuthzSubject::Session { session_id, issuer } => {
                Subject::Generic(format!("session:{}:{}", session_id, issuer))
            }
        }
    }
}

impl std::fmt::Display for Subject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device(device_id) => write!(f, "device:{}", device_id),
            Self::Guardian(uuid) => write!(f, "guardian:{}", uuid),
            Self::System => write!(f, "system"),
            Self::Generic(s) => write!(f, "{}", s),
        }
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

    /// Convert to authorization Action for interoperability
    pub fn to_authz_action(&self) -> AuthzAction {
        match self.operation.as_str() {
            "read" => AuthzAction::Read,
            "write" => AuthzAction::Write,
            "delete" => AuthzAction::Delete,
            "execute" => AuthzAction::Execute,
            "delegate" => AuthzAction::Delegate,
            "revoke" => AuthzAction::Revoke,
            "admin" => AuthzAction::Admin,
            _ => AuthzAction::Custom(self.operation.clone()),
        }
    }

    /// Convert to authorization Resource for interoperability
    pub fn to_authz_resource(&self, account_id: aura_types::AccountId) -> AuthzResource {
        match self.namespace.as_str() {
            "storage" => {
                if let Some(resource_id) = &self.resource {
                    if let Ok(object_uuid) = resource_id.parse::<uuid::Uuid>() {
                        return AuthzResource::StorageObject {
                            object_id: object_uuid,
                            owner: account_id,
                        };
                    }
                }
                AuthzResource::Account(account_id)
            }
            "session" | "protocol" => {
                if let Some(session_str) = &self.resource {
                    if let Ok(session_uuid) = session_str.parse::<uuid::Uuid>() {
                        return AuthzResource::ProtocolSession {
                            session_id: session_uuid,
                            session_type: self
                                .params
                                .get("type")
                                .cloned()
                                .unwrap_or("unknown".to_string()),
                        };
                    }
                }
                AuthzResource::Account(account_id)
            }
            "capability" => {
                if let Some(cap_id) = &self.resource {
                    if let Ok(cap_uuid) = cap_id.parse::<uuid::Uuid>() {
                        // Would need delegator info from context
                        return AuthzResource::CapabilityDelegation {
                            capability_id: cap_uuid,
                            delegator: aura_types::DeviceId::new(), // Placeholder
                        };
                    }
                }
                AuthzResource::Account(account_id)
            }
            _ => AuthzResource::Account(account_id),
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

    /// Convert to bytes for capability ID generation
    pub fn as_bytes(&self) -> Vec<u8> {
        // Use a deterministic serialization format
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.namespace.as_bytes());
        bytes.push(0); // separator
        bytes.extend_from_slice(self.operation.as_bytes());
        bytes.push(0); // separator
        if let Some(ref resource) = self.resource {
            bytes.extend_from_slice(resource.as_bytes());
        }
        bytes.push(0); // separator
        for (k, v) in &self.params {
            bytes.extend_from_slice(k.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(v.as_bytes());
            bytes.push(0);
        }
        bytes
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

    /// Create a scope from a list of permission strings
    /// This is a convenience method for Keyhive integration
    pub fn from_permissions(permissions: &[String]) -> Self {
        // For now, create a simple scope that covers all permissions
        // In a more sophisticated implementation, this could parse
        // structured permission strings
        if permissions.is_empty() {
            return Self::simple("general", "none");
        }

        // Use the first permission as the operation, with "keyhive" namespace
        let operation = permissions.first().unwrap_or(&"unknown".to_string()).clone();
        
        let mut scope = Self::simple("keyhive", &operation);
        
        // Add other permissions as parameters
        for (i, permission) in permissions.iter().enumerate().skip(1) {
            scope.params.insert(format!("perm_{}", i), permission.clone());
        }
        
        scope
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
