#![allow(clippy::disallowed_methods)]

//! Bridge between GuardEffectSystem and aura-wot capability evaluation
//!
//! This module implements the bridge interface that allows aura-wot to evaluate
//! capabilities without directly depending on a concrete effect system implementation.

use super::effect_system_trait::{GuardEffectSystem, SecurityContext};
use crate::authorization::BiscuitAuthorizationBridge;
use aura_wot::ResourceScope;
use biscuit_auth::Biscuit;
// use crate::wot::EffectSystemInterface; // Legacy interface removed

/// Wrapper type to avoid coherence issues
pub struct GuardEffectSystemWrapper<E>(pub E);

/// Biscuit-based authorization integration for GuardEffectSystem
pub struct BiscuitGuardIntegration<E: GuardEffectSystem> {
    /// The underlying effect system
    pub effect_system: E,
    /// Biscuit authorization bridge for token verification
    pub auth_bridge: BiscuitAuthorizationBridge,
}

impl<E: GuardEffectSystem> BiscuitGuardIntegration<E> {
    /// Create a new Biscuit guard integration
    pub fn new(effect_system: E, auth_bridge: BiscuitAuthorizationBridge) -> Self {
        Self {
            effect_system,
            auth_bridge,
        }
    }

    /// Authorize an operation with a Biscuit token
    pub fn authorize_with_token(
        &self,
        token: &Biscuit,
        operation: &str,
        resource_scope: &ResourceScope,
    ) -> Result<bool, aura_wot::BiscuitError> {
        // Use the authorization bridge to verify the token
        let auth_result = self.auth_bridge.authorize(token, operation, resource_scope)?;
        
        // Check if the effect system can perform this operation
        if !self.effect_system.can_perform_operation(operation) {
            tracing::debug!(
                operation = %operation,
                "Effect system cannot perform operation despite valid token"
            );
            return Ok(false);
        }

        tracing::debug!(
            operation = %operation,
            authorized = auth_result.authorized,
            delegation_depth = ?auth_result.delegation_depth,
            "Biscuit authorization completed"
        );

        Ok(auth_result.authorized)
    }

    /// Get the security context for authorization decisions
    pub fn get_authorization_context(&self) -> SecurityContext {
        self.effect_system.get_security_context()
    }

    /// Check if an operation is allowed without a specific token
    /// (for operations that don't require Biscuit authorization)
    pub fn check_operation_allowed(&self, operation: &str) -> bool {
        self.effect_system.can_perform_operation(operation)
    }
}

/// Extension methods for GuardEffectSystem to support guard operations
pub trait GuardExtensions {
    /// Check if this effect system can perform a specific operation
    fn can_perform_operation(&self, operation: &str) -> bool;

    /// Get current security context
    fn get_security_context(&self) -> SecurityContext;
}

impl<E: GuardEffectSystem> GuardExtensions for E {
    fn can_perform_operation(&self, operation: &str) -> bool {
        GuardEffectSystem::can_perform_operation(self, operation)
    }

    fn get_security_context(&self) -> SecurityContext {
        SecurityContext {
            authority_id: self.authority_id(),
            security_level: super::effect_system_trait::SecurityLevel::Normal,
            hardware_secure: false,
        }
    }
}

#[cfg(all(test, feature = "fixture_effects"))]
mod tests {
    use super::*;
    use aura_core::identifiers::AuthorityId;
    use aura_macros::aura_test;
    use aura_testkit::*;

    #[aura_test]
    async fn test_effect_system_interface() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let authority_id = AuthorityId::from_uuid((fixture.device_id()).0);
        let effect_system = fixture.effects();

        // Test authority ID retrieval
        assert_eq!(effect_system.device_id(), authority_id);

        // Test metadata retrieval
        assert_eq!(
            effect_system.get_metadata("execution_mode"),
            Some("Testing".to_string())
        );

        assert_eq!(effect_system.get_metadata("unknown_key"), None);
        Ok(())
    }

    #[aura_test]
    async fn test_guard_extensions() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let authority_id = AuthorityId::from_uuid((fixture.device_id()).0);
        let effect_system = fixture.effects();

        // Test operation permissions
        assert!(effect_system.can_perform_operation("send_message"));
        assert!(effect_system.can_perform_operation("sign_data"));

        // Test security context
        let context = effect_system.get_security_context();
        assert_eq!(context.authority_id, authority_id);
        Ok(())
    }
}
