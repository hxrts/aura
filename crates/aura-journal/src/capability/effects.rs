//! Effect-based capability integration for journal operations
//!
//! This module bridges the journal's capability system with Rumpsteak's
//! algebraic effects, enabling choreographic protocols to use capabilities
//! as first-class effect values.

use super::{
    Permission, StorageOperation, CommunicationOperation, RelayOperation,
    DeviceAuthentication, CapabilityProof, CapabilityId,
};
use aura_types::{DeviceId, AccountId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Capability effect algebra for journal operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalCapabilityEffect<R, M> {
    /// Check if a capability is available
    CheckCapability {
        device: DeviceId,
        permission: Permission,
        continuation: fn(bool) -> M,
    },
    
    /// Request a capability proof
    RequestProof {
        device: DeviceId,
        permission: Permission,
        continuation: fn(Result<CapabilityProof, String>) -> M,
    },
    
    /// Verify a capability proof
    VerifyProof {
        proof: CapabilityProof,
        continuation: fn(Result<(), String>) -> M,
    },
    
    /// Grant a capability
    GrantCapability {
        device: DeviceId,
        permission: Permission,
        duration_secs: Option<u64>,
        continuation: fn(Result<CapabilityId, String>) -> M,
    },
    
    /// Revoke a capability
    RevokeCapability {
        capability_id: CapabilityId,
        continuation: fn(Result<(), String>) -> M,
    },
    
    /// Delegate a capability
    DelegateCapability {
        from: DeviceId,
        to: DeviceId,
        permission: Permission,
        restrictions: Vec<String>,
        continuation: fn(Result<CapabilityId, String>) -> M,
    },
    
    /// Query active capabilities
    QueryCapabilities {
        device: DeviceId,
        continuation: fn(Vec<(CapabilityId, Permission)>) -> M,
    },
    
    _Phantom(std::marker::PhantomData<R>),
}

/// Effect handler for journal capabilities
pub struct JournalCapabilityHandler {
    /// In-memory capability store (production would use persistent storage)
    capabilities: Arc<RwLock<HashMap<CapabilityId, CapabilityRecord>>>,
    
    /// Device authentication cache
    authentications: Arc<RwLock<HashMap<DeviceId, DeviceAuthentication>>>,
    
    /// Account ID for this handler
    account_id: AccountId,
}

/// Internal capability record
#[derive(Debug, Clone)]
struct CapabilityRecord {
    id: CapabilityId,
    device: DeviceId,
    permission: Permission,
    granted_at: u64,
    expires_at: Option<u64>,
    revoked: bool,
    delegated_from: Option<CapabilityId>,
}

impl JournalCapabilityHandler {
    /// Create a new capability handler
    pub fn new(account_id: AccountId) -> Self {
        Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            authentications: Arc::new(RwLock::new(HashMap::new())),
            account_id,
        }
    }
    
    /// Handle a capability effect
    pub async fn handle_effect<R, M>(&self, effect: JournalCapabilityEffect<R, M>) -> M 
    where
        R: Send + Sync,
        M: Send + Sync,
    {
        match effect {
            JournalCapabilityEffect::CheckCapability { device, permission, continuation } => {
                let has_capability = self.check_capability_internal(device, permission).await;
                continuation(has_capability)
            }
            
            JournalCapabilityEffect::RequestProof { device, permission, continuation } => {
                let proof_result = self.request_proof_internal(device, permission).await;
                continuation(proof_result)
            }
            
            JournalCapabilityEffect::VerifyProof { proof, continuation } => {
                let verify_result = self.verify_proof_internal(proof).await;
                continuation(verify_result)
            }
            
            JournalCapabilityEffect::GrantCapability { device, permission, duration_secs, continuation } => {
                let grant_result = self.grant_capability_internal(device, permission, duration_secs).await;
                continuation(grant_result)
            }
            
            JournalCapabilityEffect::RevokeCapability { capability_id, continuation } => {
                let revoke_result = self.revoke_capability_internal(capability_id).await;
                continuation(revoke_result)
            }
            
            JournalCapabilityEffect::DelegateCapability { from, to, permission, restrictions, continuation } => {
                let delegate_result = self.delegate_capability_internal(from, to, permission, restrictions).await;
                continuation(delegate_result)
            }
            
            JournalCapabilityEffect::QueryCapabilities { device, continuation } => {
                let capabilities = self.query_capabilities_internal(device).await;
                continuation(capabilities)
            }
            
            JournalCapabilityEffect::_Phantom(_) => unreachable!(),
        }
    }
    
    // Internal implementation methods
    
    async fn check_capability_internal(&self, device: DeviceId, permission: Permission) -> bool {
        let capabilities = self.capabilities.read().await;
        
        capabilities.values().any(|record| {
            record.device == device 
                && record.permission == permission 
                && !record.revoked
                && record.expires_at.map_or(true, |exp| {
                    exp > aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0)
                })
        })
    }
    
    async fn request_proof_internal(&self, device: DeviceId, permission: Permission) -> Result<CapabilityProof, String> {
        let capabilities = self.capabilities.read().await;
        
        let capability = capabilities.values()
            .find(|record| {
                record.device == device 
                    && record.permission == permission 
                    && !record.revoked
            })
            .ok_or_else(|| "No matching capability found".to_string())?;
            
        // In production, this would create a proper cryptographic proof
        Ok(CapabilityProof {
            capability_id: capability.id,
            device_id: device,
            permission: permission.clone(),
            proof_data: vec![0u8; 64], // Placeholder signature
        })
    }
    
    async fn verify_proof_internal(&self, proof: CapabilityProof) -> Result<(), String> {
        let capabilities = self.capabilities.read().await;
        
        let capability = capabilities.get(&proof.capability_id)
            .ok_or_else(|| "Invalid capability ID".to_string())?;
            
        if capability.device != proof.device_id {
            return Err("Device mismatch".to_string());
        }
        
        if capability.permission != proof.permission {
            return Err("Permission mismatch".to_string());
        }
        
        if capability.revoked {
            return Err("Capability revoked".to_string());
        }
        
        // In production, verify cryptographic proof
        Ok(())
    }
    
    async fn grant_capability_internal(
        &self, 
        device: DeviceId, 
        permission: Permission, 
        duration_secs: Option<u64>
    ) -> Result<CapabilityId, String> {
        let mut capabilities = self.capabilities.write().await;
        
        let id = CapabilityId(uuid::Uuid::new_v4());
        let now = aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0);
        
        let record = CapabilityRecord {
            id,
            device,
            permission,
            granted_at: now,
            expires_at: duration_secs.map(|d| now + d),
            revoked: false,
            delegated_from: None,
        };
        
        capabilities.insert(id, record);
        Ok(id)
    }
    
    async fn revoke_capability_internal(&self, capability_id: CapabilityId) -> Result<(), String> {
        let mut capabilities = self.capabilities.write().await;
        
        let capability = capabilities.get_mut(&capability_id)
            .ok_or_else(|| "Capability not found".to_string())?;
            
        capability.revoked = true;
        Ok(())
    }
    
    async fn delegate_capability_internal(
        &self,
        from: DeviceId,
        to: DeviceId,
        permission: Permission,
        _restrictions: Vec<String>,
    ) -> Result<CapabilityId, String> {
        let capabilities = self.capabilities.read().await;
        
        // Find source capability
        let source = capabilities.values()
            .find(|record| {
                record.device == from 
                    && record.permission == permission 
                    && !record.revoked
            })
            .ok_or_else(|| "Source capability not found".to_string())?;
            
        drop(capabilities);
        
        // Create delegated capability
        let mut capabilities = self.capabilities.write().await;
        let id = CapabilityId(uuid::Uuid::new_v4());
        let now = aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0);
        
        let record = CapabilityRecord {
            id,
            device: to,
            permission,
            granted_at: now,
            expires_at: source.expires_at,
            revoked: false,
            delegated_from: Some(source.id),
        };
        
        capabilities.insert(id, record);
        Ok(id)
    }
    
    async fn query_capabilities_internal(&self, device: DeviceId) -> Vec<(CapabilityId, Permission)> {
        let capabilities = self.capabilities.read().await;
        
        capabilities.values()
            .filter(|record| record.device == device && !record.revoked)
            .map(|record| (record.id, record.permission.clone()))
            .collect()
    }
}

/// Extension trait to make journal capabilities work with choreographic protocols
#[cfg(feature = "choreographic")]
pub trait JournalChoreographicExt {
    /// Convert a permission check into a choreographic effect
    fn check_permission_effect<R, M>(
        device: DeviceId,
        permission: Permission,
    ) -> JournalCapabilityEffect<R, M>
    where
        M: From<bool>;
        
    /// Convert a capability grant into a choreographic effect
    fn grant_permission_effect<R, M>(
        device: DeviceId,
        permission: Permission,
        duration_secs: Option<u64>,
    ) -> JournalCapabilityEffect<R, M>
    where
        M: From<Result<CapabilityId, String>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::{DeviceIdExt, AccountIdExt};
    
    #[tokio::test]
    async fn test_capability_grant_and_check() {
        let effects = aura_crypto::Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        
        let handler = JournalCapabilityHandler::new(account_id);
        
        // Grant a storage read capability
        let permission = Permission::Storage {
            operation: StorageOperation::Read,
            resource: "/test/resource".to_string(),
        };
        
        let grant_effect = JournalCapabilityEffect::GrantCapability {
            device: device_id,
            permission: permission.clone(),
            duration_secs: Some(3600),
            continuation: |result| result,
        };
        
        let capability_id = handler.handle_effect(grant_effect).await.unwrap();
        
        // Check the capability exists
        let check_effect = JournalCapabilityEffect::CheckCapability {
            device: device_id,
            permission,
            continuation: |has_cap| has_cap,
        };
        
        let has_capability = handler.handle_effect(check_effect).await;
        assert!(has_capability);
    }
    
    #[tokio::test]
    async fn test_capability_revocation() {
        let effects = aura_crypto::Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        
        let handler = JournalCapabilityHandler::new(account_id);
        
        // Grant a capability
        let permission = Permission::Communication {
            operation: CommunicationOperation::Send,
            relationship: "friend".to_string(),
        };
        
        let grant_effect = JournalCapabilityEffect::GrantCapability {
            device: device_id,
            permission: permission.clone(),
            duration_secs: None,
            continuation: |result| result,
        };
        
        let capability_id = handler.handle_effect(grant_effect).await.unwrap();
        
        // Revoke the capability
        let revoke_effect = JournalCapabilityEffect::RevokeCapability {
            capability_id,
            continuation: |result| result,
        };
        
        handler.handle_effect(revoke_effect).await.unwrap();
        
        // Check the capability no longer exists
        let check_effect = JournalCapabilityEffect::CheckCapability {
            device: device_id,
            permission,
            continuation: |has_cap| has_cap,
        };
        
        let has_capability = handler.handle_effect(check_effect).await;
        assert!(!has_capability);
    }
}