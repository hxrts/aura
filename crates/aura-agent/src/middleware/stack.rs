//! Agent middleware stack for composing multiple middleware layers

use super::{AgentContext, AgentHandler, AgentMiddleware};
use crate::error::Result;
use crate::middleware::AgentOperation;
use std::sync::Arc;

/// A stack of agent middleware that processes operations in order
pub struct AgentMiddlewareStack {
    /// Middleware layers in processing order
    middleware: Vec<Arc<dyn AgentMiddleware>>,

    /// Final handler for operations
    handler: Arc<dyn AgentHandler>,
}

impl AgentMiddlewareStack {
    /// Create a new middleware stack with a handler
    pub fn new(handler: Arc<dyn AgentHandler>) -> Self {
        Self {
            middleware: Vec::new(),
            handler,
        }
    }

    /// Add middleware to the stack
    pub fn with_middleware(mut self, middleware: Arc<dyn AgentMiddleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    /// Process an operation through the middleware stack
    pub fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
    ) -> Result<serde_json::Value> {
        // Create a chain of middleware processors
        let chain = MiddlewareChain::new(&self.middleware, &self.handler);
        chain.process(operation, context, 0)
    }
}

/// Internal structure for managing middleware chain execution
struct MiddlewareChain<'a> {
    middleware: &'a [Arc<dyn AgentMiddleware>],
    handler: &'a Arc<dyn AgentHandler>,
}

impl<'a> MiddlewareChain<'a> {
    fn new(middleware: &'a [Arc<dyn AgentMiddleware>], handler: &'a Arc<dyn AgentHandler>) -> Self {
        Self {
            middleware,
            handler,
        }
    }

    fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
        index: usize,
    ) -> Result<serde_json::Value> {
        if index >= self.middleware.len() {
            // Reached the end of middleware chain, call the final handler
            self.handler.handle(operation, context)
        } else {
            // Call the next middleware in the chain
            let next_handler = NextHandler {
                chain: self,
                index: index + 1,
            };

            self.middleware[index].process(operation, context, &next_handler)
        }
    }
}

/// Handler that represents the next step in the middleware chain
struct NextHandler<'a> {
    chain: &'a MiddlewareChain<'a>,
    index: usize,
}

impl<'a> AgentHandler for NextHandler<'a> {
    fn handle(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
    ) -> Result<serde_json::Value> {
        self.chain.process(operation, context, self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::AgentOperation;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    struct TestMiddleware {
        name: String,
    }

    impl TestMiddleware {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    impl AgentMiddleware for TestMiddleware {
        fn process(
            &self,
            operation: AgentOperation,
            context: &AgentContext,
            next: &dyn AgentHandler,
        ) -> Result<serde_json::Value> {
            // Call next handler and add our middleware info
            let mut result = next.handle(operation, context)?;

            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    format!("middleware_{}", self.name),
                    serde_json::Value::String("processed".to_string()),
                );
            }

            Ok(result)
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    struct NoOpHandler;

    impl AgentHandler for NoOpHandler {
        fn handle(
            &self,
            operation: AgentOperation,
            _context: &AgentContext,
        ) -> Result<serde_json::Value> {
            Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "handler": "no_op",
                "success": true
            }))
        }
    }

    #[test]
    fn test_agent_middleware_stack() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let stack = AgentMiddlewareStack::new(Arc::new(NoOpHandler))
            .with_middleware(Arc::new(TestMiddleware::new("identity")))
            .with_middleware(Arc::new(TestMiddleware::new("session")));

        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::GetStatus;

        let result = stack.process(operation, &context).unwrap();

        // Verify that both middleware processed the request
        assert!(result.get("middleware_identity").is_some());
        assert!(result.get("middleware_session").is_some());
        assert_eq!(result.get("handler").unwrap(), "no_op");
    }
}
