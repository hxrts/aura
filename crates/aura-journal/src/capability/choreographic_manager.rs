//! Choreographic extensions for the unified capability manager
//!
//! This module extends the UnifiedCapabilityManager with choreographic protocol
//! support, enabling capabilities to be used within Rumpsteak choreographies.

use super::{
    unified_manager::{UnifiedCapabilityManager, UnifiedCapabilityToken},
    effects::{JournalCapabilityEffect, JournalCapabilityHandler},
    Permission, CapabilityId,
};
use aura_types::{DeviceId, AccountId};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Choreographic capability manager that bridges unified capabilities with effects
pub struct ChoreographicCapabilityManager {
    /// The underlying unified manager
    unified_manager: Arc<RwLock<UnifiedCapabilityManager>>,
    
    /// Effect handler for choreographic operations
    effect_handler: JournalCapabilityHandler,
    
    /// Account ID for this manager
    account_id: AccountId,
}

impl ChoreographicCapabilityManager {
    /// Create a new choreographic capability manager
    pub fn new(
        unified_manager: UnifiedCapabilityManager,
        account_id: AccountId,
    ) -> Self {
        Self {
            unified_manager: Arc::new(RwLock::new(unified_manager)),
            effect_handler: JournalCapabilityHandler::new(account_id),
            account_id,
        }
    }
    
    /// Handle a capability effect through the unified manager
    pub async fn handle_effect<R, M>(&self, effect: JournalCapabilityEffect<R, M>) -> M
    where
        R: Send + Sync,
        M: Send + Sync,
    {
        match &effect {
            // For grant operations, also update the unified manager
            JournalCapabilityEffect::GrantCapability { device, permission, duration_secs, .. } => {
                let mut manager = self.unified_manager.write().await;
                
                // Create a unified token
                let token = UnifiedCapabilityToken {
                    id: CapabilityId(uuid::Uuid::new_v4()),
                    device_id: *device,
                    permissions: vec![permission.clone()],
                    issued_at: aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0),
                    expires_at: duration_secs.map(|d| {
                        aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0) + d
                    }),
                    delegation_chain: vec![],
                };
                
                // Store in unified manager
                manager.grant_capability_token(token).await;
            }
            
            // For revoke operations, also update the unified manager
            JournalCapabilityEffect::RevokeCapability { capability_id, .. } => {
                let mut manager = self.unified_manager.write().await;
                let _ = manager.revoke_capability(capability_id).await;
            }
            
            _ => {}
        }
        
        // Handle through effect handler
        self.effect_handler.handle_effect(effect).await
    }
    
    /// Get the unified manager for direct access
    pub fn unified_manager(&self) -> Arc<RwLock<UnifiedCapabilityManager>> {
        Arc::clone(&self.unified_manager)
    }
}

/// Extension trait for using UnifiedCapabilityManager in choreographic contexts
#[async_trait]
pub trait UnifiedCapabilityChoreographicExt {
    /// Check if a device has a specific permission
    async fn check_permission_choreographic(
        &self,
        device: DeviceId,
        permission: &Permission,
    ) -> bool;
    
    /// Grant a capability and return the effect
    async fn grant_capability_choreographic(
        &mut self,
        device: DeviceId,
        permission: Permission,
        duration_secs: Option<u64>,
    ) -> Result<CapabilityId, String>;
    
    /// Revoke a capability by ID
    async fn revoke_capability_choreographic(
        &mut self,
        capability_id: &CapabilityId,
    ) -> Result<(), String>;
}

#[async_trait]
impl UnifiedCapabilityChoreographicExt for UnifiedCapabilityManager {
    async fn check_permission_choreographic(
        &self,
        device: DeviceId,
        permission: &Permission,
    ) -> bool {
        // Check through the unified manager's existing methods
        self.query_device_capabilities(&device)
            .await
            .iter()
            .any(|token| {
                token.permissions.contains(permission) &&
                token.expires_at.map_or(true, |exp| {
                    exp > aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0)
                })
            })
    }
    
    async fn grant_capability_choreographic(
        &mut self,
        device: DeviceId,
        permission: Permission,
        duration_secs: Option<u64>,
    ) -> Result<CapabilityId, String> {
        let token = UnifiedCapabilityToken {
            id: CapabilityId(uuid::Uuid::new_v4()),
            device_id: device,
            permissions: vec![permission],
            issued_at: aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0),
            expires_at: duration_secs.map(|d| {
                aura_crypto::current_timestamp_with_effects(&aura_crypto::Effects::production()).unwrap_or(0) + d
            }),
            delegation_chain: vec![],
        };
        
        let id = token.id;
        self.grant_capability_token(token).await;
        Ok(id)
    }
    
    async fn revoke_capability_choreographic(
        &mut self,
        capability_id: &CapabilityId,
    ) -> Result<(), String> {
        self.revoke_capability(capability_id)
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::unified_manager::UnifiedConfig;
    use aura_types::{DeviceIdExt, AccountIdExt};
    
    #[tokio::test]
    async fn test_choreographic_capability_grant() {
        let effects = aura_crypto::Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        
        let config = UnifiedConfig::default();
        let unified_manager = UnifiedCapabilityManager::new(config, effects.clone());
        let mut choreo_manager = ChoreographicCapabilityManager::new(unified_manager, account_id);
        
        // Grant through choreographic interface
        let permission = Permission::Storage {
            operation: super::super::StorageOperation::Read,
            resource: "/test".to_string(),
        };
        
        let unified = choreo_manager.unified_manager();
        let mut manager = unified.write().await;
        let cap_id = manager.grant_capability_choreographic(
            device_id,
            permission.clone(),
            Some(3600)
        ).await.unwrap();
        
        // Verify it exists
        let has_permission = manager.check_permission_choreographic(device_id, &permission).await;
        assert!(has_permission);
        
        // Revoke it
        manager.revoke_capability_choreographic(&cap_id).await.unwrap();
        
        // Verify it's gone
        let has_permission = manager.check_permission_choreographic(device_id, &permission).await;
        assert!(!has_permission);
    }
}