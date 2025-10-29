//! Aura Authorization
//!
//! Layer 3 of the Aura security model: Access control and permission decisions.
//!
//! This crate handles deciding WHAT someone is allowed to do (access control):
//! - "Does DeviceId X have the capability to delegate?"
//! - "Is this capability chain valid and not revoked?"
//! - "Can this subject perform this action on this resource?"
//!
//! Authorization is stateful - it evaluates against capability graphs, policies,
//! and revocation lists to make access control decisions.

#![allow(missing_docs)]

pub mod capability;
pub mod decisions;
pub mod policy;

// Re-export commonly used types
pub use capability::{CapabilityChain, CapabilityScope, CapabilityToken};
pub use decisions::{authorize_event, AccessDecision};
pub use policy::{AuthorityGraph, PolicyEvaluation};

/// Authorization errors
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("Access denied: {0}")]
    AccessDenied(String),

    #[error("Invalid capability: {0}")]
    InvalidCapability(String),

    #[error("Capability expired: {0}")]
    CapabilityExpired(String),

    #[error("Capability revoked: {0}")]
    CapabilityRevoked(String),

    #[error("Invalid delegation chain: {0}")]
    InvalidDelegationChain(String),

    #[error("Policy evaluation failed: {0}")]
    PolicyEvaluationFailed(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(#[from] aura_authentication::AuthenticationError),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub type Result<T> = std::result::Result<T, AuthorizationError>;

/// Subject that is requesting authorization
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Subject {
    /// A device identified by its DeviceId
    Device(aura_types::DeviceId),

    /// A guardian identified by their GuardianId
    Guardian(uuid::Uuid),

    /// A threshold group of devices
    ThresholdGroup {
        participants: Vec<aura_types::DeviceId>,
        threshold: u16,
    },

    /// A session identified by session ticket
    Session {
        session_id: uuid::Uuid,
        issuer: aura_types::DeviceId,
    },
}

/// Resource being accessed
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Resource {
    /// Account-level resource
    Account(aura_types::AccountId),

    /// Device-level resource
    Device(aura_types::DeviceId),

    /// Storage object
    StorageObject {
        object_id: uuid::Uuid,
        owner: aura_types::AccountId,
    },

    /// Protocol session
    ProtocolSession {
        session_id: uuid::Uuid,
        session_type: String,
    },

    /// Capability delegation
    CapabilityDelegation {
        capability_id: uuid::Uuid,
        delegator: aura_types::DeviceId,
    },
}

/// Action being performed on a resource
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    /// Read access
    Read,

    /// Write access
    Write,

    /// Delete access
    Delete,

    /// Execute/invoke access
    Execute,

    /// Delegate capability to another subject
    Delegate,

    /// Revoke a previously granted capability
    Revoke,

    /// Administrative access
    Admin,

    /// Custom action with string identifier
    Custom(String),
}
