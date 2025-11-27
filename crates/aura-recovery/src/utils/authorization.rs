//! Common authorization logic for recovery operations

use aura_core::scope::{ContextOp, ResourceScope};
use aura_core::{effects::PhysicalTimeEffects, AccountId, ContextId, DeviceId};
use aura_protocol::guards::BiscuitGuardEvaluator;
use aura_wot::BiscuitTokenManager;

/// Helper for common Biscuit authorization patterns in recovery operations
pub struct AuthorizationHelper;

impl AuthorizationHelper {
    /// Check authorization for a recovery operation using Biscuit tokens
    ///
    /// # Parameters
    /// - `token_manager`: Optional token manager (if None, authorization passes for testing)
    /// - `guard_evaluator`: Optional guard evaluator (if None, authorization passes for testing)
    /// - `operation`: The operation name to check (e.g., "initiate_guardian_setup")
    /// - `account_id`: Account being operated on
    /// - `operation_type`: Type of context operation
    /// - `time_effects`: Time effects for retrieving the current timestamp
    ///
    /// # Returns
    /// - `Ok(())` if authorized or components not available (test mode)
    /// - `Err(String)` with error message if authorization fails
    pub async fn check_recovery_authorization(
        token_manager: Option<&BiscuitTokenManager>,
        guard_evaluator: Option<&BiscuitGuardEvaluator>,
        operation: &str,
        account_id: &AccountId,
        operation_type: ContextOp,
        time_effects: &dyn PhysicalTimeEffects,
    ) -> Result<(), String> {
        let (tm, ge) = match (token_manager, guard_evaluator) {
            (Some(tm), Some(ge)) => (tm, ge),
            // If biscuit components are not wired (e.g., testing), allow the operation
            _ => return Ok(()),
        };

        let token = tm.current_token();

        let resource_scope = ResourceScope::Context {
            context_id: ContextId::from_uuid(account_id.0),
            operation: operation_type,
        };

        // Check authorization using injected time effects
        let current_time = time_effects
            .physical_time()
            .await
            .map(|pt| pt.ts_ms / 1000) // Convert milliseconds to seconds
            .map_err(|e| format!("Unable to read physical time: {e}"))?;
        let authorized = ge
            .check_guard(token, operation, &resource_scope, current_time)
            .map_err(|e| format!("Biscuit authorization error: {}", e))?;

        if !authorized {
            return Err(format!(
                "Biscuit token does not grant permission for operation: {}",
                operation
            ));
        }

        Ok(())
    }

    /// Check setup authorization specifically
    pub async fn check_setup_authorization(
        token_manager: Option<&BiscuitTokenManager>,
        guard_evaluator: Option<&BiscuitGuardEvaluator>,
        account_id: &AccountId,
        time_effects: &dyn PhysicalTimeEffects,
    ) -> Result<(), String> {
        Self::check_recovery_authorization(
            token_manager,
            guard_evaluator,
            "initiate_guardian_setup",
            account_id,
            ContextOp::UpdateGuardianSet,
            time_effects,
        )
        .await
    }

    /// Check membership change authorization specifically
    pub async fn check_membership_authorization(
        token_manager: Option<&BiscuitTokenManager>,
        guard_evaluator: Option<&BiscuitGuardEvaluator>,
        account_id: &AccountId,
        time_effects: &dyn PhysicalTimeEffects,
    ) -> Result<(), String> {
        Self::check_recovery_authorization(
            token_manager,
            guard_evaluator,
            "initiate_membership_change",
            account_id,
            ContextOp::UpdateGuardianSet,
            time_effects,
        )
        .await
    }

    /// Check key recovery authorization specifically
    pub async fn check_key_recovery_authorization(
        token_manager: Option<&BiscuitTokenManager>,
        guard_evaluator: Option<&BiscuitGuardEvaluator>,
        account_id: &AccountId,
        operation_type: ContextOp,
        time_effects: &dyn PhysicalTimeEffects,
    ) -> Result<(), String> {
        Self::check_recovery_authorization(
            token_manager,
            guard_evaluator,
            "initiate_emergency_recovery",
            account_id,
            operation_type,
            time_effects,
        )
        .await
    }

    /// Generate a unique ceremony ID for a recovery operation
    pub fn generate_ceremony_id(
        prefix: &str,
        account_id: &AccountId,
        device_id: &DeviceId,
    ) -> String {
        format!("{}_{}__{}", prefix, account_id, device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::time::controllable_time::ControllableTimeSource;

    #[tokio::test]
    async fn test_authorization_without_components() {
        // When components are not provided, should allow operation (test mode)
        let time = ControllableTimeSource::new(0);
        let result = AuthorizationHelper::check_recovery_authorization(
            None,
            None,
            "test_operation",
            &AccountId::new(),
            ContextOp::RecoverDeviceKey,
            &time,
        )
        .await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_ceremony_id_generation() {
        let account_id = AccountId::new();
        let device_id = DeviceId::new();

        let id = AuthorizationHelper::generate_ceremony_id("setup", &account_id, &device_id);

        assert!(id.starts_with("setup_"));
        assert!(id.contains(&account_id.to_string()));
        assert!(id.contains(&device_id.to_string()));
    }
}
