//! Operations for the Automerge effect_api

use crate::types::GuardianMetadata;
use aura_core::{AuthorityId, GuardianId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for an operation
///
/// Wraps a UUID for type safety.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub Uuid);

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationId {
    /// Create a new zero operation ID
    ///
    /// Create a new random operation ID. For actual operation IDs, use
    /// `from_bytes` with bytes from a RandomEffects handler.
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create an operation ID from random bytes (for use with effects)
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    /// Create an operation ID from a UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Protocol outcome for completed protocols
///
/// Indicates the result of a protocol execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProtocolOutcome {
    /// Protocol succeeded with metadata
    Success {
        /// Additional metadata from protocol completion
        metadata: HashMap<String, String>,
    },
    /// Protocol failed
    Failed {
        /// Reason for failure
        reason: String,
    },
    /// Protocol was aborted
    Aborted {
        /// Reason for abort
        reason: String,
    },
}

/// Capability that can be granted
///
/// Represents a permission to perform specific actions on a resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    /// Unique identifier for this capability
    pub id: Uuid,
    /// Resource this capability applies to
    pub resource: String,
    /// Actions allowed by this capability
    pub actions: Vec<String>,
    /// Constraints on capability usage
    pub constraints: HashMap<String, String>,
}

/// Operations that can be applied to the effect_api
///
/// **DEPRECATED**: This enum represents legacy device-centric effect_api operations.
/// In the authority-centric model, these operations are replaced by AttestedOps in the commitment tree.
///
/// **Migration Path**:
/// - Device operations: Use TreeEffects (add_leaf, remove_leaf, etc.)
/// - Guardian operations: Use RelationalContext for guardian bindings
/// - Epoch/resharing: Use TreeEffects::rotate_epoch()
/// - Protocol tracking: Use the choreography system for multi-party coordination
/// - All state changes are now expressed as facts (AttestedOps) rather than imperative operations
#[deprecated(
    since = "0.1.0",
    note = "Use TreeEffects and RelationalContext instead. Operations are now fact-based AttestedOps."
)]
#[allow(clippy::large_enum_variant)] // Deprecated enum - will be removed
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    /// Add a new guardian to the account
    AddGuardian {
        /// Guardian metadata to add
        guardian: GuardianMetadata,
    },
    /// Increment the account epoch for key rotation
    IncrementEpoch,
}

#[allow(deprecated)]
impl Operation {
    /// Generate a deterministic identifier for this operation
    ///
    /// Creates a content-addressed ID by hashing the operation's serialized form.
    /// Identical operations will always produce the same ID.
    pub fn id(&self) -> OperationId {
        let serialized = serde_json::to_string(self).unwrap_or_default();
        let hash = aura_core::hash::hash(serialized.as_bytes());
        // hash returns a 32-byte array, so [..16] is always valid
        #[allow(clippy::expect_used)]
        let uuid_bytes: [u8; 16] = hash[..16].try_into().expect("hash is always 32 bytes");
        OperationId(Uuid::from_bytes(uuid_bytes))
    }

    /// Get the authority that initiated this operation
    ///
    /// Returns None for all current operations (deprecated legacy operations).
    pub fn initiator(&self) -> Option<&AuthorityId> {
        match self {
            Self::AddGuardian { .. } => None,
            Self::IncrementEpoch => None,
        }
    }

    /// Check if this operation can be safely applied multiple times
    ///
    /// Returns true for operations that are idempotent and safe to replay.
    pub fn is_idempotent(&self) -> bool {
        matches!(self, Self::IncrementEpoch)
    }

    /// Check if this operation conflicts with another operation
    ///
    /// Returns true if applying both operations concurrently would create
    /// an inconsistent state. Used for conflict detection in CRDT merging.
    pub fn conflicts_with(&self, _other: &Self) -> bool {
        // No conflicts between remaining operations (legacy enum)
        false
    }
}

/// Validation errors for operations
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Guardian not found in account state
    #[error("Guardian not found: {0}")]
    GuardianNotFound(GuardianId),

    /// Cryptographic signature verification failed
    #[error("Invalid signature")]
    InvalidSignature,

    /// Operation not authorized for this actor
    #[error("Unauthorized operation")]
    Unauthorized,

    /// Referenced protocol session not found
    #[error("Protocol not found: {0}")]
    ProtocolNotFound(Uuid),

    /// General operation validation failure
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

/// Journal operations for effect system processing
///
/// **DEPRECATED**: This enum represents legacy device-centric operations.
/// In the authority-centric model, device and guardian operations are replaced by AttestedOps.
///
/// **Migration Path**:
/// - Device operations: Use TreeEffects to add/remove leaves in the commitment tree
/// - Guardian operations: Use RelationalContext to manage guardian bindings
/// - Epoch operations: Use TreeEffects::rotate_epoch() for key rotation
/// - Queries: Use TreeEffects::get_current_state() to query tree state
///
/// **Note**: The JournalOperation type in aura-protocol/guards/journal_coupler.rs is separate
/// and represents fact-based delta tracking (MergeFacts, RefineCapabilities, etc.), which is
/// aligned with the new architecture.
#[deprecated(
    since = "0.1.0",
    note = "Use TreeEffects for device operations and RelationalContext for guardian operations. See migration path in doc comments."
)]
#[allow(clippy::large_enum_variant)] // Deprecated enum - will be removed
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JournalOperation {
    /// Add a guardian to the account
    AddGuardian {
        /// The guardian metadata to add
        guardian: GuardianMetadata,
    },

    /// Increment the account epoch
    IncrementEpoch,

    /// Get current epoch
    GetEpoch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(deprecated)] // Testing deprecated Operation enum for backward compatibility
    fn test_operation_id_deterministic() {
        let op1 = Operation::IncrementEpoch;
        let op2 = Operation::IncrementEpoch;

        assert_eq!(op1.id(), op2.id());
    }
}
