//! Authorization effect handlers
//!
//! This module provides standard implementations of the `AuthorizationEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_core::effects::{AuthorizationEffects, AuthorizationError};
use aura_core::{Cap, AuthorityId};
use aura_macros::aura_effect_handlers;
use std::collections::HashMap;

// Generate both mock and real authorization handlers using the macro
aura_effect_handlers! {
    trait_name: AuthorizationEffects,
    mock: {
        struct_name: MockAuthorizationHandler,
        state: {
            default_allow: bool,
            capability_responses: HashMap<String, bool>,
            delegation_responses: HashMap<String, bool>,
        },
        features: {
            async_trait: true,
            deterministic: true,
        },
        methods: {
            verify_capability(_capabilities: &Cap, operation: &str, resource: &str) -> Result<bool, AuthorizationError> => {
                // Check for specific responses first
                let key = format!("{}:{}", operation, resource);
                if let Some(&response) = self.capability_responses.get(&key) {
                    return Ok(response);
                }

                // Check operation-only responses
                if let Some(&response) = self.capability_responses.get(operation) {
                    return Ok(response);
                }

                // Return default behavior
                Ok(self.default_allow)
            },
            delegate_capabilities(_source_capabilities: &Cap, requested_capabilities: &Cap, target_device: &AuthorityId) -> Result<Cap, AuthorizationError> => {
                // Check for specific delegation response
                let key = format!("{}:{}", target_device, "delegate");
                if let Some(&allowed) = self.delegation_responses.get(&key) {
                    if !allowed {
                        return Err(AuthorizationError::AccessDenied {
                            operation: "delegate".to_string(),
                            resource: target_device.to_string(),
                        });
                    }
                }

                // Mock implementation: simplified capability intersection
                // In a real implementation, this would use proper meet-semilattice operations
                Ok(requested_capabilities.clone())
            },
        },
    },
    real: {
        struct_name: StandardAuthorizationHandler,
        state: {
            allow_all: bool,
        },
        features: {
            async_trait: true,
        },
        methods: {
            verify_capability(capabilities: &Cap, operation: &str, resource: &str) -> Result<bool, AuthorizationError> => {
                // If allow_all is enabled, permit everything (development mode)
                if self.allow_all {
                    return Ok(true);
                }

                // Check temporal validity first
                let current_time = aura_core::current_unix_timestamp();
                if !capabilities.is_valid_at(current_time) {
                    return Ok(false);
                }

                // Check if capability applies to the resource
                if !capabilities.applies_to(resource) {
                    return Ok(false);
                }

                // Map operation to required permission
                let required_permission = Self::map_operation_to_permission(operation);

                // Check if capabilities allow the required permission
                let allowed = capabilities.allows(&required_permission);

                tracing::debug!(
                    "Authorization check: operation='{}', resource='{}', permission='{}', allowed={}",
                    operation, resource, required_permission, allowed
                );

                Ok(allowed)
            },
            delegate_capabilities(source_capabilities: &Cap, requested_capabilities: &Cap, target_device: &AuthorityId) -> Result<Cap, AuthorizationError> => {
                // Check temporal validity of source capabilities
                let current_time = aura_core::current_unix_timestamp();
                if !source_capabilities.is_valid_at(current_time) {
                    return Err(AuthorizationError::AccessDenied {
                        operation: "delegate".to_string(),
                        resource: target_device.to_string(),
                    });
                }

                // Perform meet-semilattice intersection: source ⊓ requested
                // This ensures delegation can only restrict authority, never expand it
                use aura_core::MeetSemiLattice;
                let mut delegated_capabilities = source_capabilities.meet(requested_capabilities);

                // Add delegation entry to track provenance
                delegated_capabilities.add_delegation(
                    "delegator".to_string(), // In real implementation, get from context
                    target_device.to_string(),
                    None, // No additional constraints for basic delegation
                );

                tracing::debug!(
                    "Capability delegation: target_device={}, source_permissions={:?}, requested_permissions={:?}, delegated_permissions={:?}",
                    target_device, source_capabilities.permissions(), requested_capabilities.permissions(), delegated_capabilities.permissions()
                );

                Ok(delegated_capabilities)
            },
        },
    },
}

impl MockAuthorizationHandler {
    /// Create a mock handler that allows all operations
    pub fn allow_all() -> Self {
        let mut handler = Self::new_deterministic();
        handler.default_allow = true;
        handler
    }

    /// Create a mock handler that denies all operations
    pub fn deny_all() -> Self {
        let mut handler = Self::new_deterministic();
        handler.default_allow = false;
        handler
    }

    /// Set the default allow/deny behavior
    pub fn with_default_allow(mut self, allow: bool) -> Self {
        self.default_allow = allow;
        self
    }

    /// Configure specific capability check response
    pub fn with_capability_response(
        mut self,
        operation: &str,
        resource: &str,
        allowed: bool,
    ) -> Self {
        let key = format!("{}:{}", operation, resource);
        self.capability_responses.insert(key, allowed);
        self
    }

    /// Configure capability response for operation only
    pub fn with_operation_response(mut self, operation: &str, allowed: bool) -> Self {
        self.capability_responses
            .insert(operation.to_string(), allowed);
        self
    }

    /// Configure delegation response for a specific authority
    pub fn with_delegation_response(mut self, target_device: &AuthorityId, allowed: bool) -> Self {
        let key = format!("{}:{}", target_device, "delegate");
        self.delegation_responses.insert(key, allowed);
        self
    }
}

impl StandardAuthorizationHandler {
    /// Create a handler that allows all operations (for development)
    pub fn allow_all() -> Self {
        Self { allow_all: true }
    }

    /// Create a handler with standard authorization rules
    pub fn with_standard_rules() -> Self {
        Self { allow_all: false }
    }

    /// Map operation strings to required permissions
    fn map_operation_to_permission(operation: &str) -> String {
        match operation {
            // Tree operations
            "tree:add_leaf" => "tree:write".to_string(),
            "tree:remove_leaf" => "tree:write".to_string(),
            "tree:change_policy" => "tree:admin".to_string(),
            "tree:rotate_epoch" => "tree:admin".to_string(),
            "tree:read" => "tree:read".to_string(),

            // Storage operations
            "storage:write" => "storage:write".to_string(),
            "storage:read" => "storage:read".to_string(),
            "storage:delete" => "storage:write".to_string(),
            "storage:admin" => "storage:admin".to_string(),

            // Journal operations
            "journal:read" => "journal:read".to_string(),
            "journal:modify" => "journal:write".to_string(),
            "journal:sync" => "journal:sync".to_string(),
            "journal:admin" => "journal:admin".to_string(),

            // Network/transport operations
            "network:send" => "network:send".to_string(),
            "network:receive" => "network:receive".to_string(),
            "network:broadcast" => "network:broadcast".to_string(),

            // Administrative operations
            "admin:device_management" => "admin:device".to_string(),
            "admin:capability_delegation" => "admin:delegate".to_string(),
            "admin:system_config" => "admin:system".to_string(),

            // Choreography operations
            "choreo:initiate" => "choreo:initiate".to_string(),
            "choreo:participate" => "choreo:participate".to_string(),
            "choreo:coordinate" => "choreo:coordinate".to_string(),

            // Recovery operations
            "recovery:initiate" => "recovery:initiate".to_string(),
            "recovery:approve" => "recovery:approve".to_string(),
            "recovery:dispute" => "recovery:dispute".to_string(),

            // FROST operations
            "frost:sign" => "frost:sign".to_string(),
            "frost:verify" => "frost:verify".to_string(),
            "frost:keygen" => "frost:keygen".to_string(),

            // Default: use operation as-is for unknown operations
            _ => operation.to_string(),
        }
    }
}

/// Specialized authorization handler for storage operations
pub struct StorageAuthorizationHandler {
    /// Underlying authorization handler
    auth_handler: StandardAuthorizationHandler,
}

impl StorageAuthorizationHandler {
    /// Create a new storage authorization handler
    pub fn new() -> Self {
        Self {
            auth_handler: StandardAuthorizationHandler::with_standard_rules(),
        }
    }

    /// Create a permissive handler for development
    pub fn allow_all() -> Self {
        Self {
            auth_handler: StandardAuthorizationHandler::allow_all(),
        }
    }
}

#[async_trait::async_trait]
impl AuthorizationEffects for StorageAuthorizationHandler {
    async fn verify_capability(
        &self,
        capabilities: &Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, AuthorizationError> {
        // Map storage operations to specific permissions
        let storage_operation = match operation {
            "read" | "get" | "list" => "storage:read",
            "write" | "put" | "create" => "storage:write",
            "delete" | "remove" => "storage:write", // Delete requires write permission
            "admin" | "configure" => "storage:admin",
            _ => operation, // Pass through other operations
        };

        // Ensure resource is scoped to storage
        let scoped_resource = if resource.starts_with("storage:") {
            resource
        } else {
            &format!("storage:{}", resource)
        };

        self.auth_handler
            .verify_capability(capabilities, storage_operation, scoped_resource)
            .await
    }

    async fn delegate_capabilities(
        &self,
        source_capabilities: &Cap,
        requested_capabilities: &Cap,
        target_device: &AuthorityId,
    ) -> Result<Cap, AuthorizationError> {
        self.auth_handler
            .delegate_capabilities(source_capabilities, requested_capabilities, target_device)
            .await
    }
}

/// Specialized authorization handler for tree operations
pub struct TreeAuthorizationHandler {
    /// Underlying authorization handler
    auth_handler: StandardAuthorizationHandler,
}

impl TreeAuthorizationHandler {
    /// Create a new tree authorization handler
    pub fn new() -> Self {
        Self {
            auth_handler: StandardAuthorizationHandler::with_standard_rules(),
        }
    }

    /// Create a permissive handler for development
    pub fn allow_all() -> Self {
        Self {
            auth_handler: StandardAuthorizationHandler::allow_all(),
        }
    }
}

#[async_trait::async_trait]
impl AuthorizationEffects for TreeAuthorizationHandler {
    async fn verify_capability(
        &self,
        capabilities: &Cap,
        operation: &str,
        resource: &str,
    ) -> Result<bool, AuthorizationError> {
        // Map tree operations to specific permissions
        let tree_operation = match operation {
            "add_leaf" | "add_node" | "insert" => "tree:write",
            "remove_leaf" | "remove_node" | "delete" => "tree:write",
            "read" | "get" | "query" => "tree:read",
            "change_policy" | "update_policy" => "tree:admin",
            "rotate_epoch" | "commit_epoch" => "tree:admin",
            _ => operation, // Pass through other operations
        };

        // Ensure resource is scoped to tree
        let scoped_resource = if resource.starts_with("tree:") {
            resource
        } else {
            &format!("tree:{}", resource)
        };

        self.auth_handler
            .verify_capability(capabilities, tree_operation, scoped_resource)
            .await
    }

    async fn delegate_capabilities(
        &self,
        source_capabilities: &Cap,
        requested_capabilities: &Cap,
        target_device: &AuthorityId,
    ) -> Result<Cap, AuthorizationError> {
        self.auth_handler
            .delegate_capabilities(source_capabilities, requested_capabilities, target_device)
            .await
    }
}
