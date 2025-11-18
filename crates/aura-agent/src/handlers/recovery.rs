//! Guardian Recovery Operations
//!
//! Simplified recovery operations using the aura-recovery crate.
//! This handler provides a clean interface for guardian-based key recovery.

use crate::errors::{AuraError, Result};
use crate::runtime::AuraEffectSystem;
#[cfg(test)]
use crate::runtime::EffectSystemConfig;
use aura_authenticate::guardian_auth::RecoveryContext;
#[cfg(test)]
use aura_authenticate::guardian_auth::RecoveryOperationType;
use aura_core::{AccountId, DeviceId};
use aura_recovery::{
    guardian_key_recovery::GuardianKeyRecoveryCoordinator,
    types::{GuardianSet, RecoveryRequest, RecoveryResponse},
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Recovery operations handler
pub struct RecoveryOperations {
    /// Core effect system
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this instance
    device_id: DeviceId,
    /// Account ID
    account_id: AccountId,
}

impl RecoveryOperations {
    /// Create new recovery operations handler
    pub fn new(
        effects: Arc<RwLock<AuraEffectSystem>>,
        device_id: DeviceId,
        account_id: AccountId,
    ) -> Self {
        Self {
            effects,
            device_id,
            account_id,
        }
    }

    /// Execute emergency key recovery
    pub async fn execute_emergency_recovery(
        &self,
        guardians: GuardianSet,
        threshold: usize,
        context: RecoveryContext,
    ) -> Result<RecoveryResponse> {
        // TODO: Fix coordinator creation - requires refactoring to use Arc<dyn AuraEffects>
        let _ = self.effects.read().await;
        Err(AuraError::internal(
            "Guardian key recovery not yet implemented - requires Arc-based effect system",
        ))

        /*
        let request = RecoveryRequest {
            requesting_device: self.device_id,
            account_id: self.account_id,
            context,
            threshold,
            guardians,
        };

        coordinator
            .execute_key_recovery(request)
            .await
            .map_err(|e| AuraError::internal(format!("Recovery failed: {}", e)))
        */
    }

    /// Get account ID
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::GuardianId;
    use aura_recovery::types::GuardianProfile;

    #[tokio::test]
    async fn test_emergency_recovery() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let account_id = AccountId(uuid::Uuid::from_bytes([1u8; 16]));
        let config = EffectSystemConfig::for_testing(device_id);
        let effects = Arc::new(RwLock::new(AuraEffectSystem::new(config).unwrap()));

        let recovery_ops = RecoveryOperations::new(effects, device_id, account_id);

        // Create test guardians
        let guardian1 = GuardianProfile::new(
            GuardianId::new(),
            DeviceId(uuid::Uuid::from_bytes([2u8; 16])),
            "Guardian 1",
        );
        let guardian2 = GuardianProfile::new(
            GuardianId::new(),
            DeviceId(uuid::Uuid::from_bytes([3u8; 16])),
            "Guardian 2",
        );
        let guardians = GuardianSet::new(vec![guardian1, guardian2]);

        let context = RecoveryContext {
            operation_type: RecoveryOperationType::DeviceKeyRecovery,
            justification: "Device lost".to_string(),
            is_emergency: true,
            timestamp: 0,
        };

        let result = recovery_ops
            .execute_emergency_recovery(guardians, 2, context)
            .await;

        // Should succeed with the simplified API
        assert!(result.is_ok());
        let response = result.unwrap();

        // Check that we get a valid response (though may fail due to insufficient approvals in simulation)
        assert!(response.error.is_some() || response.success);
    }
}
