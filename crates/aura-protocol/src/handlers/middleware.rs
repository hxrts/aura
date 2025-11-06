//! Middleware system for Aura handlers (simplified version)
//!
//! This module defines the middleware traits and composition system that
//! enables cross-cutting concerns to be added to any handler in a
//! composable manner.
//!
//! Note: This is a simplified version that avoids trait object compatibility issues.
//! A more sophisticated implementation would use dynamic composition with proper
//! type erasure patterns.

use async_trait::async_trait;

use super::context::AuraContext;
use super::{AuraHandler, AuraHandlerError, EffectType, ExecutionMode};
use aura_types::sessions::LocalSessionType;

/// Core trait for all middleware in the Aura system (simplified)
///
/// This simplified version avoids the trait object compatibility issues
/// that arise from generic methods in trait definitions.
pub trait AuraMiddleware: Send + Sync {
    /// Get the name of this middleware for debugging
    fn name(&self) -> &'static str;

    /// Get the priority of this middleware (lower numbers execute first)
    fn priority(&self) -> u8 {
        128 // Default medium priority
    }

    /// Check if this middleware should process the given effect type
    fn should_process_effect(&self, _effect_type: EffectType) -> bool {
        true // By default, process all effect types
    }
}

/// Middleware stack for composing multiple middleware (simplified)
///
/// This simplified version stores basic configuration instead of
/// dynamic middleware chains to avoid trait object issues.
#[derive(Debug)]
pub struct MiddlewareStack {
    /// Device ID for this middleware stack
    device_id: aura_types::DeviceId,
    /// Execution mode for this stack
    execution_mode: ExecutionMode,
}

impl MiddlewareStack {
    /// Create a new middleware stack (simplified version)
    pub fn new(device_id: aura_types::DeviceId, execution_mode: ExecutionMode) -> Self {
        Self {
            device_id,
            execution_mode,
        }
    }

    /// Add middleware to the stack (simplified stub)
    pub fn add_middleware(self, _middleware_name: &str) -> Self {
        // Simplified version - just return self
        self
    }

    /// Get the number of middleware in the stack (simplified stub)
    pub fn middleware_count(&self) -> usize {
        0
    }

    /// Get middleware names for debugging (simplified stub)
    pub fn middleware_names(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Get the device ID
    pub fn device_id(&self) -> aura_types::DeviceId {
        self.device_id
    }
}

#[async_trait]
impl AuraHandler for MiddlewareStack {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        _operation: &str,
        _parameters: &[u8],
        _ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        // Simplified stub implementation - handlers should override this
        Err(AuraHandlerError::UnsupportedEffect { effect_type })
    }

    async fn execute_session(
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Simplified stub implementation
        Ok(())
    }

    fn supports_effect(&self, _effect_type: EffectType) -> bool {
        false
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

/// Passthrough middleware that doesn't modify operations (simplified)
pub struct PassthroughMiddleware {
    name: &'static str,
}

impl PassthroughMiddleware {
    /// Create a new passthrough middleware with the given name
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl AuraMiddleware for PassthroughMiddleware {
    fn name(&self) -> &'static str {
        self.name
    }
}

/// Logging middleware for debugging (simplified)
pub struct LoggingMiddleware {
    /// Component name for logging context
    #[allow(dead_code)]
    component: String,
}

impl LoggingMiddleware {
    /// Create a new logging middleware for the given component
    pub fn new(component: String) -> Self {
        Self { component }
    }
}

impl AuraMiddleware for LoggingMiddleware {
    fn name(&self) -> &'static str {
        "logging"
    }

    fn priority(&self) -> u8 {
        255 // Execute last (highest number)
    }
}

/// Metrics collection middleware (simplified)
pub struct MetricsMiddleware {
    /// Whether metrics collection is enabled
    #[allow(dead_code)]
    enabled: bool,
}

impl MetricsMiddleware {
    /// Create a new metrics middleware with the given enabled state
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl AuraMiddleware for MetricsMiddleware {
    fn name(&self) -> &'static str {
        "metrics"
    }

    fn priority(&self) -> u8 {
        200
    }
}
