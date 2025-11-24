//! Common authorization logic for recovery operations

use aura_core::{AccountId, ContextId, DeviceId};
use aura_protocol::guards::BiscuitGuardEvaluator;
use aura_wot::{BiscuitTokenManager, ContextOp, ResourceScope};

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

        // Check authorization
        let authorized = ge
            .check_guard(token, operation, &resource_scope)
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
    ) -> Result<(), String> {
        Self::check_recovery_authorization(
            token_manager,
            guard_evaluator,
            "initiate_guardian_setup",
            account_id,
            ContextOp::UpdateGuardianSet,
        )
        .await
    }

    /// Check membership change authorization specifically
    pub async fn check_membership_authorization(
        token_manager: Option<&BiscuitTokenManager>,
        guard_evaluator: Option<&BiscuitGuardEvaluator>,
        account_id: &AccountId,
    ) -> Result<(), String> {
        Self::check_recovery_authorization(
            token_manager,
            guard_evaluator,
            "initiate_membership_change",
            account_id,
            ContextOp::UpdateGuardianSet,
        )
        .await
    }

    /// Check key recovery authorization specifically
    pub async fn check_key_recovery_authorization(
        token_manager: Option<&BiscuitTokenManager>,
        guard_evaluator: Option<&BiscuitGuardEvaluator>,
        account_id: &AccountId,
        operation_type: ContextOp,
    ) -> Result<(), String> {
        Self::check_recovery_authorization(
            token_manager,
            guard_evaluator,
            "initiate_emergency_recovery",
            account_id,
            operation_type,
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

    #[tokio::test]
    async fn test_authorization_without_components() {
        // When components are not provided, should allow operation (test mode)
        let result = AuthorizationHelper::check_recovery_authorization(
            None,
            None,
            "test_operation",
            &AccountId::new(),
            ContextOp::RecoverDeviceKey,
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
