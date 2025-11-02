//! Operations for the Automerge ledger

use serde::{Deserialize, Serialize};
use aura_types::{DeviceId, GuardianId, ProtocolType, SessionId};
use crate::types::{DeviceMetadata, GuardianMetadata};
use uuid::Uuid;
use std::collections::HashMap;

/// Unique identifier for an operation
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub Uuid);

impl OperationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Protocol outcome for completed protocols
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ProtocolOutcome {
    Success { metadata: HashMap<String, String> },
    Failed { reason: String },
    Aborted { reason: String },
}

/// Capability that can be granted
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    pub id: Uuid,
    pub resource: String,
    pub actions: Vec<String>,
    pub constraints: HashMap<String, String>,
}

/// Operations that can be applied to the ledger
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    // Device management
    AddDevice { 
        device: DeviceMetadata,
        added_by: DeviceId,
    },
    RemoveDevice { 
        device_id: DeviceId,
        removed_by: DeviceId,
    },
    UpdateDeviceMetadata { 
        device_id: DeviceId,
        updates: HashMap<String, serde_json::Value>,
        updated_by: DeviceId,
    },
    
    // Guardian management
    AddGuardian {
        guardian: GuardianMetadata,
        added_by: DeviceId,
    },
    RemoveGuardian {
        guardian_id: GuardianId,
        removed_by: DeviceId,
    },
    UpdateGuardianMetadata {
        guardian_id: GuardianId,
        updates: HashMap<String, serde_json::Value>,
        updated_by: DeviceId,
    },
    
    // Protocol coordination
    StartProtocol {
        protocol_id: Uuid,
        protocol_type: ProtocolType,
        participants: Vec<DeviceId>,
        initiator: DeviceId,
        metadata: HashMap<String, String>,
    },
    UpdateProtocolState {
        protocol_id: Uuid,
        state_updates: HashMap<String, serde_json::Value>,
        updated_by: DeviceId,
    },
    CompleteProtocol {
        protocol_id: Uuid,
        outcome: ProtocolOutcome,
        completed_by: DeviceId,
    },
    
    // Epoch management (auto-converges to max)
    IncrementEpoch {
        device_id: DeviceId,
        reason: String,
    },
    
    // Capabilities
    GrantCapability {
        capability: Capability,
        grantee: DeviceId,
        grantor: DeviceId,
    },
    RevokeCapability {
        capability_id: Uuid,
        revoked_by: DeviceId,
    },
    
    // Session management
    CreateSession {
        session_id: SessionId,
        session_type: String,
        participants: Vec<DeviceId>,
        created_by: DeviceId,
    },
    UpdateSession {
        session_id: SessionId,
        updates: HashMap<String, serde_json::Value>,
        updated_by: DeviceId,
    },
    CloseSession {
        session_id: SessionId,
        closed_by: DeviceId,
    },
}

impl Operation {
    /// Get the operation ID
    pub fn id(&self) -> OperationId {
        // Generate deterministic ID based on operation content
        let serialized = serde_json::to_string(self).unwrap_or_default();
        let hash = blake3::hash(serialized.as_bytes());
        let bytes = hash.as_bytes();
        let uuid_bytes: [u8; 16] = bytes[..16].try_into().unwrap();
        OperationId(Uuid::from_bytes(uuid_bytes))
    }
    
    /// Get the device that initiated this operation
    pub fn initiator(&self) -> Option<&DeviceId> {
        match self {
            Self::AddDevice { added_by, .. } => Some(added_by),
            Self::RemoveDevice { removed_by, .. } => Some(removed_by),
            Self::UpdateDeviceMetadata { updated_by, .. } => Some(updated_by),
            Self::AddGuardian { added_by, .. } => Some(added_by),
            Self::RemoveGuardian { removed_by, .. } => Some(removed_by),
            Self::UpdateGuardianMetadata { updated_by, .. } => Some(updated_by),
            Self::StartProtocol { initiator, .. } => Some(initiator),
            Self::UpdateProtocolState { updated_by, .. } => Some(updated_by),
            Self::CompleteProtocol { completed_by, .. } => Some(completed_by),
            Self::IncrementEpoch { device_id, .. } => Some(device_id),
            Self::GrantCapability { grantor, .. } => Some(grantor),
            Self::RevokeCapability { revoked_by, .. } => Some(revoked_by),
            Self::CreateSession { created_by, .. } => Some(created_by),
            Self::UpdateSession { updated_by, .. } => Some(updated_by),
            Self::CloseSession { closed_by, .. } => Some(closed_by),
        }
    }
    
    /// Check if this operation is idempotent
    pub fn is_idempotent(&self) -> bool {
        match self {
            // These operations can be applied multiple times safely
            Self::IncrementEpoch { .. } => true,
            Self::UpdateProtocolState { .. } => true,
            Self::UpdateDeviceMetadata { .. } => true,
            Self::UpdateGuardianMetadata { .. } => true,
            Self::UpdateSession { .. } => true,
            // These should only be applied once
            _ => false,
        }
    }
    
    /// Check if two operations conflict
    pub fn conflicts_with(&self, other: &Self) -> bool {
        match (self, other) {
            // Can't add and remove same device concurrently
            (Self::AddDevice { device, .. }, Self::RemoveDevice { device_id, .. }) |
            (Self::RemoveDevice { device_id, .. }, Self::AddDevice { device, .. }) => {
                device.device_id == *device_id
            }
            
            // Can't modify device while removing it
            (Self::UpdateDeviceMetadata { device_id: id1, .. }, Self::RemoveDevice { device_id: id2, .. }) |
            (Self::RemoveDevice { device_id: id1, .. }, Self::UpdateDeviceMetadata { device_id: id2, .. }) => {
                id1 == id2
            }
            
            // Protocol operations on same protocol might conflict
            (Self::UpdateProtocolState { protocol_id: id1, .. }, Self::CompleteProtocol { protocol_id: id2, .. }) |
            (Self::CompleteProtocol { protocol_id: id1, .. }, Self::UpdateProtocolState { protocol_id: id2, .. }) => {
                id1 == id2
            }
            
            // Most operations don't conflict
            _ => false,
        }
    }
}

/// Validation errors for operations
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Device not found: {0}")]
    DeviceNotFound(DeviceId),
    
    #[error("Guardian not found: {0}")]
    GuardianNotFound(GuardianId),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Unauthorized operation")]
    Unauthorized,
    
    #[error("Protocol not found: {0}")]
    ProtocolNotFound(Uuid),
    
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{DeviceIdExt, GuardianIdExt};
    
    #[test]
    fn test_operation_id_deterministic() {
        let effects = Effects::test(42);
        let device_id = DeviceId::new_with_effects(&effects);
        
        let op1 = Operation::IncrementEpoch {
            device_id,
            reason: "test".to_string(),
        };
        
        let op2 = Operation::IncrementEpoch {
            device_id,
            reason: "test".to_string(),
        };
        
        assert_eq!(op1.id(), op2.id());
    }
    
    #[test]
    fn test_operation_conflicts() {
        let effects = Effects::test(42);
        let device_id = DeviceId::new_with_effects(&effects);
        let device_id2 = DeviceId::new_with_effects(&effects);
        
        let add_op = Operation::AddDevice {
            device: DeviceMetadata {
                device_id,
                device_name: "Test".to_string(),
                device_type: crate::types::DeviceType::Native,
                public_key: aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>()).verifying_key(),
                added_at: 1000,
                last_seen: 1000,
                dkd_commitment_proofs: std::collections::BTreeMap::new(),
                next_nonce: 0,
                used_nonces: std::collections::BTreeSet::new(),
                key_share_epoch: 0,
            },
            added_by: device_id2,
        };
        
        let remove_op = Operation::RemoveDevice {
            device_id,
            removed_by: device_id2,
        };
        
        assert!(add_op.conflicts_with(&remove_op));
        assert!(remove_op.conflicts_with(&add_op));
    }
}