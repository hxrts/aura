//! Bridge between AuraEffectSystem and aura-wot capability evaluation
//!
//! This module implements the bridge interface that allows aura-wot to evaluate
//! capabilities without directly depending on aura-protocol's effect system.

use crate::effects::system::AuraEffectSystem;
use aura_core::DeviceId;
use aura_wot::EffectSystemInterface;
use std::collections::HashMap;

/// Implementation of EffectSystemInterface for AuraEffectSystem
impl EffectSystemInterface for AuraEffectSystem {
    /// Get the device ID for this effect system
    fn device_id(&self) -> DeviceId {
        self.device_id()
    }

    /// Query metadata from the effect system
    fn get_metadata(&self, key: &str) -> Option<String> {
        // Placeholder implementation - would query the actual effect system
        // for metadata stored in the context or configuration
        match key {
            "execution_mode" => Some(format!("{:?}", self.execution_mode())),
            "supported_effects" => Some("all".to_string()), // Would query actual supported effects
            _ => None,
        }
    }
}

/// Extension methods for AuraEffectSystem to support guard operations
pub trait GuardExtensions {
    /// Check if this effect system can perform a specific operation
    fn can_perform_operation(&self, operation: &str) -> bool;

    /// Get current security context
    fn get_security_context(&self) -> SecurityContext;
}

impl GuardExtensions for AuraEffectSystem {
    fn can_perform_operation(&self, operation: &str) -> bool {
        // Placeholder implementation - would check actual capabilities
        // For development, allow all operations
        match operation {
            "send_message" | "receive_message" | "sign_data" | "verify_signature" => true,
            _ => true, // Permissive for development
        }
    }

    fn get_security_context(&self) -> SecurityContext {
        SecurityContext {
            device_id: self.device_id(),
            execution_mode: format!("{:?}", self.execution_mode()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            metadata: HashMap::new(),
        }
    }
}

/// Security context for capability evaluation
#[derive(Debug, Clone)]
pub struct SecurityContext {
    pub device_id: DeviceId,
    pub execution_mode: String,
    pub timestamp: u64,
    pub metadata: HashMap<String, String>,
}

impl SecurityContext {
    /// Check if the context allows a specific operation
    pub fn allows_operation(&self, operation: &str) -> bool {
        // Placeholder: would implement actual security policy checks
        match operation {
            "network_send" => true,
            "crypto_sign" => true,
            "journal_write" => true,
            _ => false,
        }
    }

    /// Get context-specific restrictions
    pub fn get_restrictions(&self) -> Vec<String> {
        let mut restrictions = Vec::new();

        // Example restrictions based on execution mode
        if self.execution_mode == "Testing" {
            restrictions.push("no_network_access".to_string());
        }

        restrictions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::system::AuraEffectSystem;
    use crate::handlers::ExecutionMode;
    use aura_core::identifiers::DeviceId;

    #[test]
    fn test_effect_system_interface() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

        // Test device ID retrieval
        assert_eq!(effect_system.device_id(), device_id);

        // Test metadata retrieval
        assert_eq!(
            effect_system.get_metadata("execution_mode"),
            Some("Testing".to_string())
        );

        assert_eq!(effect_system.get_metadata("unknown_key"), None);
    }

    #[test]
    fn test_guard_extensions() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

        // Test operation permissions
        assert!(effect_system.can_perform_operation("send_message"));
        assert!(effect_system.can_perform_operation("sign_data"));

        // Test security context
        let context = effect_system.get_security_context();
        assert_eq!(context.device_id, device_id);
        assert_eq!(context.execution_mode, "Testing");
        assert!(context.allows_operation("network_send"));
    }
}
