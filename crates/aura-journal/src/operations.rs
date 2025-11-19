//! Operations for the Automerge ledger

use crate::types::{DeviceMetadata, GuardianMetadata};
use aura_core::{DeviceId, GuardianId, ProtocolType, SessionId};
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

/// Operations that can be applied to the ledger
///
/// **DEPRECATED**: This enum represents legacy device-centric ledger operations.
/// In the authority-centric model, these operations are replaced by AttestedOps in the ratchet tree.
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    /// Add a new device to the account
    AddDevice {
        /// Device metadata to add
        device: DeviceMetadata,
    },
    /// Remove a device from the account
    RemoveDevice {
        /// Identifier of device to remove
        device_id: DeviceId,
    },
    /// Update metadata for an existing device
    UpdateDeviceMetadata {
        /// Identifier of device to update
        device_id: DeviceId,
        /// Map of metadata fields to update
        updates: HashMap<String, serde_json::Value>,
        /// Device that performed the update
        updated_by: DeviceId,
    },

    /// Add a new guardian to the account
    AddGuardian {
        /// Guardian metadata to add
        guardian: GuardianMetadata,
    },
    /// Remove a guardian from the account
    RemoveGuardian {
        /// Identifier of guardian to remove
        guardian_id: GuardianId,
        /// Device that removed the guardian
        removed_by: DeviceId,
    },
    /// Update metadata for an existing guardian
    UpdateGuardianMetadata {
        /// Identifier of guardian to update
        guardian_id: GuardianId,
        /// Map of metadata fields to update
        updates: HashMap<String, serde_json::Value>,
        /// Device that performed the update
        updated_by: DeviceId,
    },

    /// Start a new protocol execution
    StartProtocol {
        /// Unique identifier for this protocol instance
        protocol_id: Uuid,
        /// Type of protocol being started
        protocol_type: ProtocolType,
        /// Devices participating in the protocol
        participants: Vec<DeviceId>,
        /// Device that initiated the protocol
        initiator: DeviceId,
        /// Additional protocol-specific metadata
        metadata: HashMap<String, String>,
    },
    /// Update state for an ongoing protocol
    UpdateProtocolState {
        /// Identifier of protocol being updated
        protocol_id: Uuid,
        /// Map of state fields to update
        state_updates: HashMap<String, serde_json::Value>,
        /// Device that performed the update
        updated_by: DeviceId,
    },
    /// Mark a protocol as completed with outcome
    CompleteProtocol {
        /// Identifier of protocol being completed
        protocol_id: Uuid,
        /// Result of the protocol execution
        outcome: ProtocolOutcome,
        /// Device that completed the protocol
        completed_by: DeviceId,
    },

    /// Increment the account epoch for key rotation
    IncrementEpoch,

    /// Grant a capability to a device
    GrantCapability {
        /// Capability being granted
        capability: Capability,
        /// Device receiving the capability
        grantee: DeviceId,
        /// Device granting the capability
        grantor: DeviceId,
    },
    /// Revoke a previously granted capability
    RevokeCapability {
        /// Identifier of capability to revoke
        capability_id: Uuid,
        /// Device that revoked the capability
        revoked_by: DeviceId,
    },

    /// Create a new session for protocol coordination
    CreateSession {
        /// Unique identifier for the session
        session_id: SessionId,
        /// Type of session being created
        session_type: String,
        /// Devices participating in the session
        participants: Vec<DeviceId>,
        /// Device that created the session
        created_by: DeviceId,
    },
    /// Update state for an existing session
    UpdateSession {
        /// Identifier of session being updated
        session_id: SessionId,
        /// Map of session fields to update
        updates: HashMap<String, serde_json::Value>,
        /// Device that performed the update
        updated_by: DeviceId,
    },
    /// Close an active session
    CloseSession {
        /// Identifier of session to close
        session_id: SessionId,
        /// Device that closed the session
        closed_by: DeviceId,
    },
}

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

    /// Get the device that initiated this operation
    ///
    /// Returns the device identifier for operations that have an explicit initiator,
    /// or None for operations that don't track an initiating device.
    pub fn initiator(&self) -> Option<&DeviceId> {
        match self {
            Self::AddDevice { .. } => None,
            Self::RemoveDevice { .. } => None,
            Self::UpdateDeviceMetadata { updated_by, .. } => Some(updated_by),
            Self::AddGuardian { .. } => None,
            Self::RemoveGuardian { removed_by, .. } => Some(removed_by),
            Self::UpdateGuardianMetadata { updated_by, .. } => Some(updated_by),
            Self::StartProtocol { initiator, .. } => Some(initiator),
            Self::UpdateProtocolState { updated_by, .. } => Some(updated_by),
            Self::CompleteProtocol { completed_by, .. } => Some(completed_by),
            Self::IncrementEpoch => None,
            Self::GrantCapability { grantor, .. } => Some(grantor),
            Self::RevokeCapability { revoked_by, .. } => Some(revoked_by),
            Self::CreateSession { created_by, .. } => Some(created_by),
            Self::UpdateSession { updated_by, .. } => Some(updated_by),
            Self::CloseSession { closed_by, .. } => Some(closed_by),
        }
    }

    /// Check if this operation can be safely applied multiple times
    ///
    /// Returns true for operations that are idempotent and safe to replay,
    /// such as updates and epoch increments. Returns false for operations
    /// that should only be applied once, such as adding or removing entities.
    pub fn is_idempotent(&self) -> bool {
        matches!(
            self,
            Self::IncrementEpoch { .. }
                | Self::UpdateProtocolState { .. }
                | Self::UpdateDeviceMetadata { .. }
                | Self::UpdateGuardianMetadata { .. }
                | Self::UpdateSession { .. }
        )
    }

    /// Check if this operation conflicts with another operation
    ///
    /// Returns true if applying both operations concurrently would create
    /// an inconsistent state. Used for conflict detection in CRDT merging.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::AddDevice { device }, Self::RemoveDevice { device_id })
            | (Self::RemoveDevice { device_id }, Self::AddDevice { device }) => {
                device.device_id == *device_id
            }

            (
                Self::UpdateDeviceMetadata { device_id: id1, .. },
                Self::RemoveDevice { device_id: id2 },
            )
            | (
                Self::RemoveDevice { device_id: id1 },
                Self::UpdateDeviceMetadata { device_id: id2, .. },
            ) => id1 == id2,

            (
                Self::UpdateProtocolState {
                    protocol_id: id1, ..
                },
                Self::CompleteProtocol {
                    protocol_id: id2, ..
                },
            )
            | (
                Self::CompleteProtocol {
                    protocol_id: id1, ..
                },
                Self::UpdateProtocolState {
                    protocol_id: id2, ..
                },
            ) => id1 == id2,

            _ => false,
        }
    }
}

/// Validation errors for operations
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// Device not found in account state
    #[error("Device not found: {0}")]
    DeviceNotFound(DeviceId),

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
/// - Device operations: Use TreeEffects to add/remove leaves in the ratchet tree
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JournalOperation {
    /// Add a device to the account
    AddDevice {
        /// The device metadata to add
        device: DeviceMetadata,
    },

    /// Remove a device from the account
    RemoveDevice {
        /// The ID of the device to remove
        device_id: DeviceId,
    },

    /// Add a guardian to the account
    AddGuardian {
        /// The guardian metadata to add
        guardian: GuardianMetadata,
    },

    /// Increment the account epoch
    IncrementEpoch,

    /// Get all active devices
    GetDevices,

    /// Get current epoch
    GetEpoch,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_operation_id_deterministic() {
        let op1 = Operation::IncrementEpoch;
        let op2 = Operation::IncrementEpoch;

        assert_eq!(op1.id(), op2.id());
    }

    #[test]
    fn test_operation_conflicts() {
        // Use fixed test data instead of effects-based generation
        let device_id = DeviceId(Uuid::from_bytes([13u8; 16]));

        let add_op = Operation::AddDevice {
            device: DeviceMetadata {
                device_id,
                device_name: "Test".to_string(),
                device_type: crate::types::DeviceType::Native,
                public_key: aura_core::Ed25519SigningKey::from_bytes(&[1u8; 32]).verifying_key(),
                added_at: 1000,
                last_seen: 1000,
                dkd_commitment_proofs: std::collections::BTreeMap::new(),
                next_nonce: 0,
                used_nonces: std::collections::BTreeSet::new(),
                key_share_epoch: 0,
            },
        };

        let remove_op = Operation::RemoveDevice { device_id };

        assert!(add_op.conflicts_with(&remove_op));
        assert!(remove_op.conflicts_with(&add_op));
    }
}
