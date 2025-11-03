//! Middleware stack builder
//!
//! Provides a builder pattern for composing multiple middleware decorators.

use super::{
    observability::{MetricsMiddleware, TracingMiddleware},
    resilience::{RetryMiddleware, retry::RetryConfig},
    security::CapabilityMiddleware,
    MiddlewareConfig,
};
use crate::effects::*;
use uuid::Uuid;

/// Builder for composing middleware stacks
pub struct MiddlewareStack<H> {
    handler: H,
    device_id: Uuid,
}

impl<H> MiddlewareStack<H> {
    /// Create a new middleware stack builder
    pub fn new(handler: H, device_id: Uuid) -> Self {
        Self { handler, device_id }
    }

    /// Add tracing middleware
    pub fn with_tracing(self, service_name: String) -> MiddlewareStack<TracingMiddleware<H>> {
        MiddlewareStack {
            handler: TracingMiddleware::new(self.handler, self.device_id, service_name),
            device_id: self.device_id,
        }
    }

    /// Add metrics middleware
    pub fn with_metrics(self) -> MiddlewareStack<MetricsMiddleware<H>> {
        MiddlewareStack {
            handler: MetricsMiddleware::new(self.handler, self.device_id),
            device_id: self.device_id,
        }
    }

    /// Add retry middleware
    pub fn with_retry(self, config: RetryConfig) -> MiddlewareStack<RetryMiddleware<H>> {
        MiddlewareStack {
            handler: RetryMiddleware::new(self.handler, config),
            device_id: self.device_id,
        }
    }

    /// Add capability middleware
    pub fn with_capabilities(self) -> MiddlewareStack<CapabilityMiddleware<H>> {
        MiddlewareStack {
            handler: CapabilityMiddleware::new(self.handler),
            device_id: self.device_id,
        }
    }

    /// Build the final handler with all middleware applied
    pub fn build(self) -> H {
        self.handler
    }
}

/// Create a standard middleware stack based on configuration
/// For now, only applies one middleware to avoid type composition complexity
pub fn create_standard_stack<H>(
    handler: H,
    _device_id: Uuid,
    config: MiddlewareConfig,
) -> H
where
    H: ProtocolEffects + 'static,
{
    // For now, just return the handler as-is to avoid type composition issues
    // TODO: Implement proper middleware composition when needed
    let _ = config; // Suppress unused warning
    handler
}