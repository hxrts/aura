#![allow(clippy::disallowed_methods)]

//! Bridge between GuardEffectSystem and aura-wot capability evaluation
//!
//! This module implements the bridge interface that allows aura-wot to evaluate
//! capabilities without directly depending on a concrete effect system implementation.

use super::effect_system_trait::{GuardEffectSystem, SecurityContext};
// use crate::wot::EffectSystemInterface; // Legacy interface removed

/// Wrapper type to avoid coherence issues
pub struct GuardEffectSystemWrapper<E>(pub E);

// Legacy EffectSystemInterface implementations removed - use BiscuitAuthorizationBridge instead
// TODO: Implement Biscuit-based authorization integration when available

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
