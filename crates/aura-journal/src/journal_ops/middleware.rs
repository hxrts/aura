//! KeyJournal Middleware Stack for Composable Operation Processing
//!
//! This module provides middleware components for journal operations,
//! following the Aura middleware pattern.

use async_trait::async_trait;
use aura_types::AuraError;
use crate::journal::*;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::{
    handlers::{JournalHandler, JournalHandlerConfig},
    ops::{JournalOp, JournalOpResult},
    types::JournalState,
};

/// Middleware trait for journal operations
#[async_trait]
pub trait JournalMiddleware: Send + Sync {
    /// Process an operation with the next middleware in the chain
    async fn process(
        &self,
        op: JournalOp,
        state: &mut JournalState,
        next: Arc<dyn JournalHandler>,
    ) -> Result<JournalOpResult, AuraError>;
}

/// Middleware stack that composes multiple middleware components
pub struct JournalMiddlewareStack {
    middleware: Vec<Arc<dyn JournalMiddleware>>,
    handler: Arc<dyn JournalHandler>,
}

impl JournalMiddlewareStack {
    /// Create a new middleware stack with the given handler
    pub fn new(handler: Arc<dyn JournalHandler>) -> Self {
        Self {
            middleware: Vec::new(),
            handler,
        }
    }

    /// Add middleware to the stack
    pub fn with_middleware(mut self, middleware: Arc<dyn JournalMiddleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    /// Process an operation through the middleware stack
    pub async fn process(
        &self,
        op: JournalOp,
        state: &mut JournalState,
    ) -> Result<JournalOpResult, AuraError> {
        if self.middleware.is_empty() {
            // No middleware, apply operation directly
            self.handler.apply_operation(op, state).await
        } else {
            // Create nested handler chain
            let mut current: Arc<dyn JournalHandler> = self.handler.clone();

            // Build chain in reverse order
            for middleware in self.middleware.iter().rev() {
                current = Arc::new(MiddlewareHandler {
                    middleware: middleware.clone(),
                    next: current,
                });
            }

            current.apply_operation(op, state).await
        }
    }
}

/// Handler wrapper that applies middleware
struct MiddlewareHandler {
    middleware: Arc<dyn JournalMiddleware>,
    next: Arc<dyn JournalHandler>,
}

#[async_trait]
impl JournalHandler for MiddlewareHandler {
    async fn apply_operation(
        &self,
        op: JournalOp,
        state: &mut JournalState,
    ) -> Result<JournalOpResult, AuraError> {
        self.middleware.process(op, state, self.next.clone()).await
    }

    async fn validate_operation(
        &self,
        op: &JournalOp,
        state: &JournalState,
    ) -> Result<(), AuraError> {
        self.next.validate_operation(op, state).await
    }

    fn config(&self) -> &JournalHandlerConfig {
        self.next.config()
    }
}

/// Logging middleware that traces all journal operations
pub struct LoggingMiddleware {
    component: String,
}

impl LoggingMiddleware {
    pub fn new(component: String) -> Self {
        Self { component }
    }
}

#[async_trait]
impl JournalMiddleware for LoggingMiddleware {
    async fn process(
        &self,
        op: JournalOp,
        state: &mut JournalState,
        next: Arc<dyn JournalHandler>,
    ) -> Result<JournalOpResult, AuraError> {
        let op_type = op.op_type();
        let resource = op
            .target_resource()
            .unwrap_or_else(|| "unknown".to_string());

        debug!(
            component = %self.component,
            operation = %op_type,
            resource = %resource,
            "Processing journal operation"
        );

        let start_time = std::time::Instant::now();
        let result = next.apply_operation(op, state).await;
        let duration = start_time.elapsed();

        match &result {
            Ok(op_result) => {
                if op_result.success {
                    info!(
                        component = %self.component,
                        operation = %op_type,
                        resource = %resource,
                        duration_ms = duration.as_millis(),
                        "Journal operation completed successfully"
                    );
                } else {
                    warn!(
                        component = %self.component,
                        operation = %op_type,
                        resource = %resource,
                        error = ?op_result.error,
                        duration_ms = duration.as_millis(),
                        "Journal operation failed"
                    );
                }
            }
            Err(error) => {
                warn!(
                    component = %self.component,
                    operation = %op_type,
                    resource = %resource,
                    error = %error,
                    duration_ms = duration.as_millis(),
                    "Journal operation error"
                );
            }
        }

        result
    }
}

/// Validation middleware that performs additional validation checks
pub struct ValidationMiddleware {
    strict_mode: bool,
}

impl ValidationMiddleware {
    pub fn new(strict_mode: bool) -> Self {
        Self { strict_mode }
    }
}

#[async_trait]
impl JournalMiddleware for ValidationMiddleware {
    async fn process(
        &self,
        op: JournalOp,
        state: &mut JournalState,
        next: Arc<dyn JournalHandler>,
    ) -> Result<JournalOpResult, AuraError> {
        // Perform pre-validation
        self.validate_pre_operation(&op, state).await?;

        // Apply operation
        let result = next.apply_operation(op, state).await?;

        // Perform post-validation if in strict mode
        if self.strict_mode {
            self.validate_post_operation(&result, state).await?;
        }

        Ok(result)
    }
}

impl ValidationMiddleware {
    async fn validate_pre_operation(
        &self,
        op: &JournalOp,
        state: &JournalState,
    ) -> Result<(), AuraError> {
        // Additional validation logic
        match op {
            JournalOp::AddNode { node } => {
                // Check for duplicate display names
                if let Some(display_name) = node.display_name() {
                    for existing_node in state.journal.nodes.values() {
                        if existing_node.id != node.id {
                            if let Some(existing_name) = existing_node.display_name() {
                                if existing_name == display_name {
                                    return Err(AuraError::Data(format!(
                                        "Display name '{}' already in use",
                                        display_name
                                    )));
                                }
                            }
                        }
                    }
                }
            }

            JournalOp::UpdateNodePolicy { node, policy } => {
                // Check if policy change is compatible with current children
                if let Some(existing_node) = state.journal.nodes.get(node) {
                    let children = state.journal.get_children(node);

                    match policy {
                        NodePolicy::Threshold { m, n } => {
                            if children.len() != *n as usize {
                                return Err(AuraError::Data(format!(
                                    "Cannot set {}-of-{} policy on node with {} children",
                                    m,
                                    n,
                                    children.len()
                                )));
                            }
                        }
                        _ => {} // Other policies are more flexible
                    }
                }
            }

            _ => {} // No additional validation for other operations
        }

        Ok(())
    }

    async fn validate_post_operation(
        &self,
        result: &JournalOpResult,
        state: &JournalState,
    ) -> Result<(), AuraError> {
        if !result.success {
            return Ok(()); // Don't validate failed operations
        }

        // Validate journal structural integrity
        use super::graph::JournalGraph;
        JournalGraph::validate_journal(&state.journal)
            .map_err(|e| AuraError::Data(format!("Post-operation validation failed: {}", e)))?;

        Ok(())
    }
}

/// Metrics middleware that collects operation statistics
pub struct MetricsMiddleware {
    operation_counts: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
}

impl MetricsMiddleware {
    pub fn new() -> Self {
        Self {
            operation_counts: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub fn get_operation_count(&self, op_type: &str) -> u64 {
        self.operation_counts
            .lock()
            .unwrap()
            .get(op_type)
            .copied()
            .unwrap_or(0)
    }

    pub fn get_all_counts(&self) -> std::collections::HashMap<String, u64> {
        self.operation_counts.lock().unwrap().clone()
    }
}

#[async_trait]
impl JournalMiddleware for MetricsMiddleware {
    async fn process(
        &self,
        op: JournalOp,
        state: &mut JournalState,
        next: Arc<dyn JournalHandler>,
    ) -> Result<JournalOpResult, AuraError> {
        let op_type = op.op_type().to_string();

        // Increment operation counter
        {
            let mut counts = self.operation_counts.lock().unwrap();
            *counts.entry(op_type.clone()).or_insert(0) += 1;
        }

        // Apply operation
        let result = next.apply_operation(op, state).await;

        // Additional metrics collection could go here
        // (e.g., success/failure rates, latency histograms)

        result
    }
}

/// Authorization middleware that checks capability tokens
/// (Placeholder implementation - will be completed in Phase 4)
pub struct AuthorizationMiddleware {
    enabled: bool,
}

impl AuthorizationMiddleware {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

#[async_trait]
impl JournalMiddleware for AuthorizationMiddleware {
    async fn process(
        &self,
        op: JournalOp,
        state: &mut JournalState,
        next: Arc<dyn JournalHandler>,
    ) -> Result<JournalOpResult, AuraError> {
        if !self.enabled {
            // Authorization disabled for MVP
            return next.apply_operation(op, state).await;
        }

        // Check if operation requires capability
        if op.requires_capability() {
            // TODO: Implement capability token verification in Phase 4
            debug!("Authorization check for operation: {}", op.op_type());
        }

        next.apply_operation(op, state).await
    }
}

/// Builder for composing middleware stacks
pub struct JournalMiddlewareBuilder {
    middleware: Vec<Arc<dyn JournalMiddleware>>,
}

impl JournalMiddlewareBuilder {
    pub fn new() -> Self {
        Self {
            middleware: Vec::new(),
        }
    }

    pub fn with_logging(self, component: String) -> Self {
        self.with_middleware(Arc::new(LoggingMiddleware::new(component)))
    }

    pub fn with_validation(self, strict_mode: bool) -> Self {
        self.with_middleware(Arc::new(ValidationMiddleware::new(strict_mode)))
    }

    pub fn with_metrics(self) -> Self {
        self.with_middleware(Arc::new(MetricsMiddleware::new()))
    }

    pub fn with_authorization(self, enabled: bool) -> Self {
        self.with_middleware(Arc::new(AuthorizationMiddleware::new(enabled)))
    }

    pub fn with_middleware(mut self, middleware: Arc<dyn JournalMiddleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    pub fn build(self, handler: Arc<dyn JournalHandler>) -> JournalMiddlewareStack {
        let mut stack = JournalMiddlewareStack::new(handler);

        for middleware in self.middleware {
            stack = stack.with_middleware(middleware);
        }

        stack
    }
}

impl Default for JournalMiddlewareBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{effects::JournalEffectsAdapter, handlers::JournalHandlerImpl};
    use crate::journal::{NodeKind, NodePolicy};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_middleware_stack() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = Arc::new(JournalHandlerImpl::with_effects(effects));

        let stack = JournalMiddlewareBuilder::new()
            .with_logging("test".to_string())
            .with_validation(true)
            .with_metrics()
            .build(handler);

        let mut state = JournalState::new();
        let node = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let op = JournalOp::AddNode { node };

        let result = stack.process(op, &mut state).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_logging_middleware() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = Arc::new(JournalHandlerImpl::with_effects(effects));
        let middleware = Arc::new(LoggingMiddleware::new("test".to_string()));

        let mut state = JournalState::new();
        let node = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let op = JournalOp::AddNode { node };

        let result = middleware.process(op, &mut state, handler).await.unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn test_metrics_middleware() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = Arc::new(JournalHandlerImpl::with_effects(effects));
        let metrics = Arc::new(MetricsMiddleware::new());

        let mut state = JournalState::new();
        let node = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let op = JournalOp::AddNode { node };

        assert_eq!(metrics.get_operation_count("add_node"), 0);

        let result = metrics.process(op, &mut state, handler).await.unwrap();
        assert!(result.success);
        assert_eq!(metrics.get_operation_count("add_node"), 1);
    }

    #[tokio::test]
    async fn test_validation_middleware() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = Arc::new(JournalHandlerImpl::with_effects(effects));
        let validation = Arc::new(ValidationMiddleware::new(true));

        let mut state = JournalState::new();

        // Test duplicate display name validation
        let mut node1 = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        node1.set_display_name("test-device".to_string());

        let mut node2 = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        node2.set_display_name("test-device".to_string()); // Same name

        // First node should succeed
        let op1 = JournalOp::AddNode { node: node1 };
        let result1 = validation
            .process(op1, &mut state, handler.clone())
            .await
            .unwrap();
        assert!(result1.success);

        // Second node should fail due to duplicate name
        let op2 = JournalOp::AddNode { node: node2 };
        let result2 = validation.process(op2, &mut state, handler).await;
        assert!(result2.is_err());
    }
}
