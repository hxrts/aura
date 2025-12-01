//! Authority-based resource scopes for authorization
//!
//! This module provides domain-specific extensions and legacy conversion utilities
//! for the resource scope types that are now defined in aura-core.

// Re-export the core types from aura-core
pub use aura_core::scope::{AuthorityOp, ContextOp, ResourceScope};

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AuthorityId, ContextId};

    #[test]
    fn test_authority_scope_datalog() {
        let scope = ResourceScope::Authority {
            authority_id: AuthorityId::new_from_entropy([67u8; 32]),
            operation: AuthorityOp::UpdateTree,
        };

        let pattern = scope.to_datalog_pattern();
        assert!(pattern.contains("resource_type(\"authority\")"));
        assert!(pattern.contains("update_tree"));
    }

    #[test]
    fn test_context_scope_pattern() {
        let scope = ResourceScope::Context {
            context_id: ContextId::new_from_entropy([68u8; 32]),
            operation: ContextOp::ApproveRecovery,
        };

        let pattern = scope.resource_pattern();
        assert!(pattern.starts_with("/context/"));
        assert!(pattern.ends_with("/approve_recovery"));
    }

    #[test]
    fn test_new_authority_operations() {
        // Test new admin operations
        assert_eq!(AuthorityOp::AddGuardian.as_str(), "add_guardian");
        assert_eq!(AuthorityOp::RemoveGuardian.as_str(), "remove_guardian");
        assert_eq!(AuthorityOp::ModifyThreshold.as_str(), "modify_threshold");
        assert_eq!(AuthorityOp::RevokeDevice.as_str(), "revoke_device");
    }

    #[test]
    fn test_new_context_operations() {
        // Test new recovery operations
        assert_eq!(ContextOp::RecoverDeviceKey.as_str(), "recover_device_key");
        assert_eq!(
            ContextOp::RecoverAccountAccess.as_str(),
            "recover_account_access"
        );
        assert_eq!(ContextOp::UpdateGuardianSet.as_str(), "update_guardian_set");
        assert_eq!(ContextOp::EmergencyFreeze.as_str(), "emergency_freeze");
    }
}
