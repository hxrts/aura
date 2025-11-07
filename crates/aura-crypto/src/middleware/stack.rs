//! Crypto middleware stack for composing multiple middleware layers

use super::{CryptoContext, CryptoHandler, CryptoMiddleware};
use crate::middleware::CryptoOperation;
use crate::Result;
use std::sync::Arc;

/// A stack of crypto middleware that processes operations in order
pub struct CryptoMiddlewareStack {
    /// Middleware layers in processing order
    middleware: Vec<Arc<dyn CryptoMiddleware>>,

    /// Final handler for operations
    handler: Arc<dyn CryptoHandler>,
}

impl CryptoMiddlewareStack {
    /// Create a new middleware stack with a handler
    pub fn new(handler: Arc<dyn CryptoHandler>) -> Self {
        Self {
            middleware: Vec::new(),
            handler,
        }
    }

    /// Add middleware to the stack
    pub fn with_middleware(mut self, middleware: Arc<dyn CryptoMiddleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    /// Process an operation through the middleware stack
    pub fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
    ) -> Result<serde_json::Value> {
        // Create a chain of middleware processors
        let chain = MiddlewareChain::new(&self.middleware, &self.handler);
        chain.process(operation, context, 0)
    }
}

/// Internal structure for managing middleware chain execution
struct MiddlewareChain<'a> {
    middleware: &'a [Arc<dyn CryptoMiddleware>],
    handler: &'a Arc<dyn CryptoHandler>,
}

impl<'a> MiddlewareChain<'a> {
    fn new(
        middleware: &'a [Arc<dyn CryptoMiddleware>],
        handler: &'a Arc<dyn CryptoHandler>,
    ) -> Self {
        Self {
            middleware,
            handler,
        }
    }

    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
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

impl<'a> CryptoHandler for NextHandler<'a> {
    fn handle(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
    ) -> Result<serde_json::Value> {
        self.chain.process(operation, context, self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::{CryptoOperation, SecurityLevel};
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

    impl CryptoMiddleware for TestMiddleware {
        fn process(
            &self,
            operation: CryptoOperation,
            context: &CryptoContext,
            next: &dyn CryptoHandler,
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

    impl CryptoHandler for NoOpHandler {
        fn handle(
            &self,
            operation: CryptoOperation,
            _context: &CryptoContext,
        ) -> Result<serde_json::Value> {
            Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "handler": "no_op",
                "success": true
            }))
        }
    }

    #[test]
    fn test_crypto_middleware_stack() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let stack = CryptoMiddlewareStack::new(Arc::new(NoOpHandler))
            .with_middleware(Arc::new(TestMiddleware::new("timing")))
            .with_middleware(Arc::new(TestMiddleware::new("audit")));

        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Standard,
        );
        let operation = CryptoOperation::GenerateRandom { num_bytes: 32 };

        let result = stack.process(operation, &context).unwrap();

        // Verify that both middleware processed the request
        assert!(result.get("middleware_timing").is_some());
        assert!(result.get("middleware_audit").is_some());
        assert_eq!(result.get("handler").unwrap(), "no_op");
    }
}
