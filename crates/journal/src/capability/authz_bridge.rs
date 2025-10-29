//! Bridge functions between journal capabilities and authorization crate
//!
//! This module provides integration points between the journal's CRDT-based
//! capability system and the clean authorization crate.

use crate::capability::{
    types::{CapabilityScope, Subject},
    CapabilityError, Result,
};
use aura_authentication::AuthenticationContext;
use aura_authorization::{
    AccessDecision, Action as AuthzAction, Resource as AuthzResource, Subject as AuthzSubject,
};
use aura_types::{AccountId, DeviceId};

/// Bridge for using authorization crate logic with journal capability types
pub struct AuthorizationBridge;

impl AuthorizationBridge {
    /// Check authorization using the authorization crate's decision logic
    pub fn check_access(
        subject: &Subject,
        scope: &CapabilityScope,
        account_id: AccountId,
        auth_context: &AuthenticationContext,
    ) -> Result<AccessDecision> {
        // Convert journal types to authorization types
        let authz_subject = subject.to_authz_subject().ok_or_else(|| {
            CapabilityError::AuthorizationError(format!(
                "Cannot convert subject '{}' to authorization format",
                subject.0
            ))
        })?;

        let authz_action = scope.to_authz_action();
        let authz_resource = scope.to_authz_resource(account_id);

        // Create policy context (simplified for this integration)
        let policy_context = aura_authorization::policy::PolicyContext {
            current_time: std::time::SystemTime::now(),
            authority_graph: aura_authorization::policy::AuthorityGraph::new(),
            capabilities: vec![], // Would populate from journal state
            context_data: std::collections::HashMap::new(),
        };

        // Create access request
        let access_request = aura_authorization::decisions::AccessRequest {
            subject: authz_subject,
            action: authz_action,
            resource: authz_resource,
            capabilities: vec![], // Would populate from active capability tokens
            context: std::collections::HashMap::new(),
            timestamp: std::time::SystemTime::now(),
        };

        // Make authorization decision
        aura_authorization::decisions::make_access_decision(
            &access_request,
            auth_context,
            &policy_context,
        )
        .map_err(|e| {
            CapabilityError::AuthorizationError(format!("Authorization decision failed: {}", e))
        })
    }

    /// Convert journal Subject to authorization Subject for interoperability
    pub fn convert_subject(journal_subject: &Subject) -> Option<AuthzSubject> {
        journal_subject.to_authz_subject()
    }

    /// Convert journal CapabilityScope to authorization Action
    pub fn convert_scope_to_action(scope: &CapabilityScope) -> AuthzAction {
        scope.to_authz_action()
    }

    /// Convert journal CapabilityScope to authorization Resource  
    pub fn convert_scope_to_resource(
        scope: &CapabilityScope,
        account_id: AccountId,
    ) -> AuthzResource {
        scope.to_authz_resource(account_id)
    }
}

/// Create an authorization Subject from a DeviceId for convenience
pub fn device_subject(device_id: DeviceId) -> AuthzSubject {
    AuthzSubject::Device(device_id)
}

/// Create an authorization Subject from a GuardianId for convenience  
pub fn guardian_subject(guardian_id: uuid::Uuid) -> AuthzSubject {
    AuthzSubject::Guardian(guardian_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_subject_conversion() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let journal_subject = Subject::new(&device_id.to_string());

        let authz_subject = AuthorizationBridge::convert_subject(&journal_subject);
        assert!(authz_subject.is_some());

        if let Some(AuthzSubject::Device(converted_device_id)) = authz_subject {
            assert_eq!(device_id, converted_device_id);
        } else {
            panic!("Subject conversion failed");
        }
    }

    #[test]
    fn test_scope_to_action_conversion() {
        let scope = CapabilityScope::simple("storage", "read");
        let action = AuthorizationBridge::convert_scope_to_action(&scope);
        assert!(matches!(action, AuthzAction::Read));

        let scope = CapabilityScope::simple("capability", "delegate");
        let action = AuthorizationBridge::convert_scope_to_action(&scope);
        assert!(matches!(action, AuthzAction::Delegate));
    }

    #[test]
    fn test_scope_to_resource_conversion() {
        let effects = Effects::test();
        let account_id = AccountId::new_with_effects(&effects);

        let scope = CapabilityScope::simple("storage", "read");
        let resource = AuthorizationBridge::convert_scope_to_resource(&scope, account_id);

        // Should convert to account resource when no specific resource ID
        assert!(matches!(resource, AuthzResource::Account(id) if id == account_id));
    }
}
