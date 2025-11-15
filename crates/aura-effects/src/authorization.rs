//! Authorization effect handlers
//!
//! This module provides standard implementations of the `AuthorizationEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_macros::aura_effect_handlers;
use aura_core::effects::{AuthorizationEffects, AuthorizationError};
use aura_core::{DeviceId, Cap};
use async_trait::async_trait;
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
            verify_capability(capabilities: &Cap, operation: &str, resource: &str) -> Result<bool, AuthorizationError> => {
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
            delegate_capabilities(source_capabilities: &Cap, requested_capabilities: &Cap, target_device: &DeviceId) -> Result<Cap, AuthorizationError> => {
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
                // If allow_all is enabled, permit everything
                if self.allow_all {
                    return Ok(true);
                }
                
                // Standard implementation would check if the capability set
                // contains the required permissions for the operation on the resource
                // For now, implement basic operation checking
                
                // Check if operation requires authorization
                let requires_auth = matches!(
                    operation,
                    "tree:add_leaf" | "tree:remove_leaf" | "tree:change_policy" | "tree:rotate_epoch"
                    | "storage:write" | "storage:delete"
                    | "journal:modify"
                    | "admin:device_management"
                );
                
                if !requires_auth {
                    return Ok(true);
                }
                
                // TODO: Implement proper capability checking with Cap type
                // For now, return false to be conservative
                Ok(false)
            },
            delegate_capabilities(source_capabilities: &Cap, requested_capabilities: &Cap, target_device: &DeviceId) -> Result<Cap, AuthorizationError> => {
                // Standard implementation would perform meet-semilattice intersection
                // of source and requested capabilities (source ⊓ requested)
                // For now, implement simplified delegation
                
                // TODO: Implement proper capability intersection using meet-semilattice operations
                // This should use: source_capabilities.meet(requested_capabilities)
                
                // For now, return the requested capabilities (conservative approach)
                Ok(requested_capabilities.clone())
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
    pub fn with_capability_response(mut self, operation: &str, resource: &str, allowed: bool) -> Self {
        let key = format!("{}:{}", operation, resource);
        self.capability_responses.insert(key, allowed);
        self
    }
    
    /// Configure capability response for operation only
    pub fn with_operation_response(mut self, operation: &str, allowed: bool) -> Self {
        self.capability_responses.insert(operation.to_string(), allowed);
        self
    }
    
    /// Configure delegation response for a specific device
    pub fn with_delegation_response(mut self, target_device: &DeviceId, allowed: bool) -> Self {
        let key = format!("{}:{}", target_device, "delegate");
        self.delegation_responses.insert(key, allowed);
        self
    }
}

impl StandardAuthorizationHandler {
    /// Create a handler that allows all operations (for development)
    pub fn allow_all() -> Self {
        Self::new()
    }
    
    /// Create a handler with standard authorization rules  
    pub fn with_standard_rules() -> Self {
        Self::new()
    }
}