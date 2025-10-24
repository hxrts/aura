// Capability events for CRDT integration

use crate::{capability::types::{CapabilityId, CapabilityScope, Subject}, DeviceId};
use serde::{Deserialize, Serialize};

/// Capability delegation event with deterministic IDs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDelegation {
    /// Deterministic capability ID (BLAKE3 hash of parent chain)
    pub capability_id: CapabilityId,
    /// Parent capability ID (None for root authority)
    pub parent_id: Option<CapabilityId>,
    /// Subject being granted the capability
    pub subject_id: Subject,
    /// Scope of the capability
    pub scope: CapabilityScope,
    /// Expiry timestamp (Unix seconds, None for no expiry)
    pub expiry: Option<u64>,
    /// Cryptographic proof (threshold signature)
    pub proof: Vec<u8>,
    /// Timestamp when delegation was created
    pub issued_at: u64,
    /// Device that issued this delegation
    pub issued_by: DeviceId,
}

impl CapabilityDelegation {
    /// Create a new capability delegation
    pub fn new(
        parent_id: Option<CapabilityId>,
        subject_id: Subject,
        scope: CapabilityScope,
        expiry: Option<u64>,
        proof: Vec<u8>,
        issued_by: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Self {
        let capability_id = CapabilityId::from_chain(parent_id.as_ref(), &subject_id, &scope);
        let issued_at = effects.now().unwrap_or(0);
        
        Self {
            capability_id,
            parent_id,
            subject_id,
            scope,
            expiry,
            proof,
            issued_at,
            issued_by,
        }
    }
    
    /// Check if this delegation is expired
    pub fn is_expired(&self, effects: &aura_crypto::Effects) -> bool {
        if let Some(expiry) = self.expiry {
            effects.now().unwrap_or(0) > expiry
        } else {
            false
        }
    }
    
    /// Compute canonical hash for this delegation
    pub fn hash(&self) -> crate::capability::Result<[u8; 32]> {
        let bytes = serde_json::to_vec(self)
            .map_err(|e| crate::capability::CapabilityError::SerializationError(e.to_string()))?;
        Ok(blake3::hash(&bytes).into())
    }
}

/// Capability revocation event with cascade logic
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRevocation {
    /// ID of capability being revoked
    pub capability_id: CapabilityId,
    /// Timestamp when revocation occurred (for ordering)
    pub revoked_at: u64,
    /// Reason for revocation (human readable)
    pub reason: String,
    /// Cryptographic proof (threshold signature)
    pub proof: Vec<u8>,
    /// Device that issued this revocation
    pub issued_by: DeviceId,
}

impl CapabilityRevocation {
    /// Create a new capability revocation
    pub fn new(
        capability_id: CapabilityId,
        reason: String,
        proof: Vec<u8>,
        issued_by: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Self {
        Self {
            capability_id,
            revoked_at: effects.now().unwrap_or(0),
            reason,
            proof,
            issued_by,
        }
    }
    
    /// Compute canonical hash for this revocation
    pub fn hash(&self) -> crate::capability::Result<[u8; 32]> {
        let bytes = serde_json::to_vec(self)
            .map_err(|e| crate::capability::CapabilityError::SerializationError(e.to_string()))?;
        Ok(blake3::hash(&bytes).into())
    }
}


