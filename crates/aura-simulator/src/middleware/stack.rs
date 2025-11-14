//! Simulator middleware stack builder and composition system

use super::{
    Result, SimulatorContext, SimulatorError, SimulatorHandler, SimulatorMiddleware,
    SimulatorOperation,
};
use serde_json::Value;
use std::sync::Arc;

/// Composable middleware stack for simulator operations
pub struct SimulatorMiddlewareStack {
    /// Ordered list of middleware layers
    middleware_layers: Vec<Arc<dyn SimulatorMiddleware>>,
    /// Final handler for operations
    handler: Arc<dyn SimulatorHandler>,
}

impl SimulatorMiddlewareStack {
    /// Create new empty middleware stack
    pub fn new(handler: Arc<dyn SimulatorHandler>) -> Self {
        Self {
            middleware_layers: Vec::new(),
            handler,
        }
    }

    /// Add middleware layer to the stack
    pub fn add_middleware(&mut self, middleware: Arc<dyn SimulatorMiddleware>) {
        self.middleware_layers.push(middleware);
    }

    /// Process operation through the middleware stack
    pub fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
    ) -> Result<Value> {
        let stack_handler = SimulatorStackHandler::new(&self.middleware_layers, &self.handler);
        stack_handler.handle(operation, context)
    }

    /// Get middleware layer names
    pub fn middleware_names(&self) -> Vec<String> {
        self.middleware_layers
            .iter()
            .map(|m| m.name().to_string())
            .collect()
    }

    /// Get number of middleware layers
    pub fn layer_count(&self) -> usize {
        self.middleware_layers.len()
    }
}

/// Internal handler that processes operations through middleware layers
struct SimulatorStackHandler<'a> {
    middleware_layers: &'a [Arc<dyn SimulatorMiddleware>],
    handler: &'a Arc<dyn SimulatorHandler>,
}

impl<'a> SimulatorStackHandler<'a> {
    fn new(
        middleware_layers: &'a [Arc<dyn SimulatorMiddleware>],
        handler: &'a Arc<dyn SimulatorHandler>,
    ) -> Self {
        Self {
            middleware_layers,
            handler,
        }
    }
}

impl<'a> SimulatorHandler for SimulatorStackHandler<'a> {
    fn handle(&self, operation: SimulatorOperation, context: &SimulatorContext) -> Result<Value> {
        if let Some((first, rest)) = self.middleware_layers.split_first() {
            // Create next handler with remaining middleware
            let next = SimulatorStackHandler::new(rest, self.handler);

            // Process through first middleware if it handles this operation
            if first.handles(&operation) {
                first.process(operation, context, &next)
            } else {
                // Skip this middleware
                next.handle(operation, context)
            }
        } else {
            // No more middleware, call final handler
            self.handler.handle(operation, context)
        }
    }
}

/// Builder for constructing simulator middleware stacks
pub struct SimulatorStackBuilder {
    middleware_layers: Vec<Arc<dyn SimulatorMiddleware>>,
    handler: Option<Arc<dyn SimulatorHandler>>,
    base_context: Option<super::SimulatorContext>,
}

impl SimulatorStackBuilder {
    /// Create new stack builder
    pub fn new() -> Self {
        Self {
            middleware_layers: Vec::new(),
            handler: None,
            base_context: None,
        }
    }

    /// Add middleware layer to the stack
    pub fn with_middleware(mut self, middleware: Arc<dyn SimulatorMiddleware>) -> Self {
        self.middleware_layers.push(middleware);
        self
    }

    /// Set the final handler for the stack
    pub fn with_handler(mut self, handler: Arc<dyn SimulatorHandler>) -> Self {
        self.handler = Some(handler);
        self
    }

    /// Build the middleware stack
    pub fn build(self) -> Result<SimulatorMiddlewareStack> {
        let handler = self.handler.ok_or_else(|| {
            SimulatorError::InvalidConfiguration(
                "No handler specified for middleware stack".to_string(),
            )
        })?;

        let mut stack = SimulatorMiddlewareStack::new(handler);
        for middleware in self.middleware_layers {
            stack.add_middleware(middleware);
        }

        Ok(stack)
    }
}

impl Default for SimulatorStackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Enhanced builder methods for testkit integration
///
/// These methods provide convenient ways to create simulator stacks that integrate
/// with aura-testkit foundations using the stateless effect system architecture.
impl SimulatorStackBuilder {
    /// Create stack from testkit fixture using stateless effects
    pub async fn from_testkit_fixture(
        fixture: aura_testkit::ProtocolTestFixture,
        scenario_id: String,
    ) -> Result<Self> {
        use crate::testkit_bridge::TestkitSimulatorBridge;

        let context = TestkitSimulatorBridge::fixture_to_context(&fixture, scenario_id).await?;

        // Create middleware configuration from fixture
        let config = crate::testkit_bridge::MiddlewareConfig::for_simulation(fixture.device_id());
        let stateless_middleware = Arc::new(crate::StatelessEffectsMiddleware::new(
            fixture.device_id(),
            config,
        )?);

        Ok(Self::new()
            .with_base_context(context)
            .with_middleware(stateless_middleware))
    }

    /// Create stack from choreography harness using stateless effects
    pub fn from_choreography_harness(
        harness: aura_testkit::ChoreographyTestHarness,
    ) -> Result<Self> {
        use crate::testkit_bridge::TestkitSimulatorBridge;

        TestkitSimulatorBridge::harness_to_stack(harness)
    }

    /// Create stack from device fixtures using stateless effects
    pub async fn from_device_fixtures(
        fixtures: Vec<aura_testkit::DeviceTestFixture>,
        scenario_id: String,
        seed: u64,
    ) -> Result<Self> {
        use crate::testkit_bridge::TestkitSimulatorBridge;

        let effect_systems = TestkitSimulatorBridge::create_simulation_effects(&fixtures, seed)?;

        let context = super::SimulatorContext::new(scenario_id, uuid::Uuid::new_v4().to_string())
            .with_seed(seed)
            .with_participants(fixtures.len(), 2); // Default 2-of-N threshold

        let mut builder = Self::new().with_base_context(context);

        // Add stateless effects middleware for each device
        for (device_id, _effect_system) in effect_systems {
            let config = crate::testkit_bridge::MiddlewareConfig::for_simulation(device_id);
            let middleware =
                crate::StatelessEffectsMiddleware::new(device_id, config).map_err(|e| {
                    SimulatorError::OperationFailed(format!("Middleware creation failed: {}", e))
                })?;
            builder = builder.with_middleware(Arc::new(middleware));
        }

        Ok(builder)
    }

    /// Quick setup for chaos testing using stateless effects
    pub async fn for_chaos_testing(device_count: usize, _threshold: usize) -> Result<Self> {
        use crate::testkit_bridge::TestkitSimulatorBridge;

        let fixtures = TestkitSimulatorBridge::create_device_fixtures(device_count, 42);

        let mut builder =
            Self::from_device_fixtures(fixtures, "chaos-test".to_string(), 42).await?;

        builder = builder
            .with_middleware(Arc::new(crate::FaultSimulationMiddleware::new()))
            .with_middleware(Arc::new(crate::ChaosCoordinationMiddleware::new()))
            .with_middleware(Arc::new(crate::PropertyCheckingMiddleware::new()));

        Ok(builder)
    }

    /// Quick setup for performance testing using stateless effects
    pub async fn for_performance_testing(device_count: usize) -> Result<Self> {
        use crate::testkit_bridge::TestkitSimulatorBridge;

        let fixtures = TestkitSimulatorBridge::create_device_fixtures(device_count, 42);

        let builder = Self::from_device_fixtures(fixtures, "perf-test".to_string(), 42).await?;

        // Note: These middleware types need to be implemented or imported properly
        // For now, we'll return the builder without these additional middleware
        // builder = builder
        //     .with_middleware(Arc::new(crate::PerformanceMonitoringMiddleware::new()))
        //     .with_middleware(Arc::new(crate::ResourceProfilingMiddleware::new()));

        Ok(builder)
    }

    /// Create stack with testkit foundation and simulator orchestration
    pub async fn with_testkit_foundation(
        device_count: usize,
        _threshold: usize,
        _scenario_id: String,
    ) -> Result<Self> {
        use aura_testkit::{ChoreographyTestHarness, DeviceTestFixture, TestExecutionMode};

        // Use testkit to create clean foundation
        let fixtures: Vec<DeviceTestFixture> =
            (0..device_count).map(DeviceTestFixture::new).collect();

        let harness =
            ChoreographyTestHarness::from_fixtures(fixtures, TestExecutionMode::Simulation)
                .await
                .map_err(|e| {
                    SimulatorError::OperationFailed(format!("Harness creation failed: {:?}", e))
                })?;

        // Convert to simulator stack with stateless effects
        Self::from_choreography_harness(harness)
    }

    /// Set base context (internal helper for testkit integration)
    fn with_base_context(mut self, context: super::SimulatorContext) -> Self {
        // Store context for later use - this would be enhanced once the
        // stack supports context initialization
        self.base_context = Some(context);
        self
    }
}

/// Extension trait for ergonomic middleware composition
pub trait SimulatorMiddlewareExt<H> {
    /// Add a middleware layer using a closure
    fn layer<F, M>(self, middleware_fn: F) -> SimulatorStackBuilder
    where
        F: FnOnce() -> M,
        M: SimulatorMiddleware + 'static;
}

impl<H: SimulatorHandler + 'static> SimulatorMiddlewareExt<H> for H {
    fn layer<F, M>(self, middleware_fn: F) -> SimulatorStackBuilder
    where
        F: FnOnce() -> M,
        M: SimulatorMiddleware + 'static,
    {
        SimulatorStackBuilder::new()
            .with_handler(Arc::new(self))
            .with_middleware(Arc::new(middleware_fn()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpSimulatorHandler;
    use crate::middleware::{SimulatorContext, SimulatorOperation};
    use serde_json::json;

    struct TestMiddleware {
        name: String,
    }

    impl TestMiddleware {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    impl SimulatorMiddleware for TestMiddleware {
        fn process(
            &self,
            operation: SimulatorOperation,
            context: &SimulatorContext,
            next: &dyn SimulatorHandler,
        ) -> Result<Value> {
            // Add metadata about this middleware
            let mut enhanced_context = context.clone();
            enhanced_context
                .metadata
                .insert(format!("processed_by_{}", self.name), "true".to_string());

            // Call next handler
            let mut result = next.handle(operation, &enhanced_context)?;

            // Add our layer to the result
            if let Some(obj) = result.as_object_mut() {
                obj.insert(format!("middleware_{}", self.name), json!("processed"));
            }

            Ok(result)
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn test_empty_stack() {
        let handler = Arc::new(NoOpSimulatorHandler);
        let stack = SimulatorMiddlewareStack::new(handler);
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = stack.process(
            SimulatorOperation::InitializeScenario {
                scenario_id: "test".to_string(),
            },
            &context,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_single_middleware() {
        let middleware = Arc::new(TestMiddleware::new("test1"));
        let handler = Arc::new(NoOpSimulatorHandler);

        let stack = SimulatorStackBuilder::new()
            .with_middleware(middleware)
            .with_handler(handler)
            .build()
            .unwrap();

        let context = SimulatorContext::new("test".to_string(), "run1".to_string());
        let result = stack
            .process(
                SimulatorOperation::InitializeScenario {
                    scenario_id: "test".to_string(),
                },
                &context,
            )
            .unwrap();

        assert_eq!(result["middleware_test1"], "processed");
    }

    #[test]
    fn test_multiple_middleware() {
        let middleware1 = Arc::new(TestMiddleware::new("first"));
        let middleware2 = Arc::new(TestMiddleware::new("second"));
        let handler = Arc::new(NoOpSimulatorHandler);

        let stack = SimulatorStackBuilder::new()
            .with_middleware(middleware1)
            .with_middleware(middleware2)
            .with_handler(handler)
            .build()
            .unwrap();

        let context = SimulatorContext::new("test".to_string(), "run1".to_string());
        let result = stack
            .process(
                SimulatorOperation::InitializeScenario {
                    scenario_id: "test".to_string(),
                },
                &context,
            )
            .unwrap();

        // Both middleware should have processed the operation
        assert_eq!(result["middleware_first"], "processed");
        assert_eq!(result["middleware_second"], "processed");
    }

    #[test]
    fn test_stack_info() {
        let middleware1 = Arc::new(TestMiddleware::new("first"));
        let middleware2 = Arc::new(TestMiddleware::new("second"));
        let handler = Arc::new(NoOpSimulatorHandler);

        let stack = SimulatorStackBuilder::new()
            .with_middleware(middleware1)
            .with_middleware(middleware2)
            .with_handler(handler)
            .build()
            .unwrap();

        assert_eq!(stack.layer_count(), 2);
        assert_eq!(stack.middleware_names(), vec!["first", "second"]);
    }

    #[test]
    fn test_builder_without_handler() {
        let result = SimulatorStackBuilder::new().build();
        assert!(result.is_err());
    }
}
