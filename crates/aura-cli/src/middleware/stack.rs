//! CLI middleware stack implementation

use super::{CliHandler, CliOperation, CliContext};
use crate::CliError;
use std::sync::Arc;

/// Trait for CLI middleware components
pub trait CliMiddleware: Send + Sync {
    /// Process a CLI operation
    fn process(
        &self,
        operation: CliOperation,
        context: &CliContext,
        next: &dyn CliHandler,
    ) -> Result<serde_json::Value, CliError>;
    
    /// Get middleware name for debugging
    fn name(&self) -> &str;
}

/// Stack of CLI middleware
pub struct CliMiddlewareStack {
    middleware: Vec<Arc<dyn CliMiddleware>>,
    handler: Option<Arc<dyn CliHandler>>,
}

impl CliMiddlewareStack {
    /// Create a new empty middleware stack
    pub fn new() -> Self {
        Self {
            middleware: Vec::new(),
            handler: None,
        }
    }
    
    /// Add middleware to the stack
    pub fn add_middleware(&mut self, middleware: Arc<dyn CliMiddleware>) {
        self.middleware.push(middleware);
    }
    
    /// Set the final handler
    pub fn set_handler(&mut self, handler: Arc<dyn CliHandler>) {
        self.handler = Some(handler);
    }
    
    /// Process an operation through the middleware stack
    pub fn process(
        &self,
        operation: CliOperation,
        context: &CliContext,
    ) -> Result<serde_json::Value, CliError> {
        if self.middleware.is_empty() {
            if let Some(handler) = &self.handler {
                handler.handle(operation, context)
            } else {
                Err(CliError::Configuration(
                    "No handler configured".to_string()
                ))
            }
        } else {
            let chain = MiddlewareChain::new(&self.middleware, &self.handler);
            chain.execute(operation, context, 0)
        }
    }
    
    /// Get number of middleware layers
    pub fn len(&self) -> usize {
        self.middleware.len()
    }
    
    /// Check if stack is empty
    pub fn is_empty(&self) -> bool {
        self.middleware.is_empty()
    }
    
    /// Get middleware names for debugging
    pub fn middleware_names(&self) -> Vec<&str> {
        self.middleware.iter().map(|m| m.name()).collect()
    }
}

impl Default for CliMiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for CLI middleware stacks
pub struct CliStackBuilder {
    stack: CliMiddlewareStack,
}

impl CliStackBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            stack: CliMiddlewareStack::new(),
        }
    }
    
    /// Add middleware to the builder
    pub fn with_middleware(mut self, middleware: Arc<dyn CliMiddleware>) -> Self {
        self.stack.add_middleware(middleware);
        self
    }
    
    /// Set the final handler
    pub fn with_handler(mut self, handler: Arc<dyn CliHandler>) -> Self {
        self.stack.set_handler(handler);
        self
    }
    
    /// Build the middleware stack
    pub fn build(self) -> CliMiddlewareStack {
        self.stack
    }
}

impl Default for CliStackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal middleware chain execution
struct MiddlewareChain<'a> {
    middleware: &'a [Arc<dyn CliMiddleware>],
    handler: &'a Option<Arc<dyn CliHandler>>,
}

impl<'a> MiddlewareChain<'a> {
    fn new(
        middleware: &'a [Arc<dyn CliMiddleware>],
        handler: &'a Option<Arc<dyn CliHandler>>,
    ) -> Self {
        Self { middleware, handler }
    }
    
    fn execute(
        &self,
        operation: CliOperation,
        context: &CliContext,
        index: usize,
    ) -> Result<serde_json::Value, CliError> {
        if index >= self.middleware.len() {
            if let Some(handler) = self.handler {
                handler.handle(operation, context)
            } else {
                Err(CliError::Configuration(
                    "No handler configured".to_string()
                ))
            }
        } else {
            let next_handler = NextHandler {
                chain: self,
                index: index + 1,
            };
            self.middleware[index].process(operation, context, &next_handler)
        }
    }
}

/// Handler for the next middleware in the chain
struct NextHandler<'a> {
    chain: &'a MiddlewareChain<'a>,
    index: usize,
}

impl<'a> CliHandler for NextHandler<'a> {
    fn handle(
        &self,
        operation: CliOperation,
        context: &CliContext,
    ) -> Result<serde_json::Value, CliError> {
        self.chain.execute(operation, context, self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    
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
    
    impl CliMiddleware for TestMiddleware {
        fn process(
            &self,
            operation: CliOperation,
            context: &CliContext,
            next: &dyn CliHandler,
        ) -> Result<serde_json::Value, CliError> {
            // Just pass through to next
            next.handle(operation, context)
        }
        
        fn name(&self) -> &str {
            &self.name
        }
    }
    
    struct TestHandler;
    
    impl CliHandler for TestHandler {
        fn handle(
            &self,
            _operation: CliOperation,
            _context: &CliContext,
        ) -> Result<serde_json::Value, CliError> {
            Ok(json!({"status": "success"}))
        }
    }
    
    #[test]
    fn test_middleware_stack_creation() {
        let stack = CliStackBuilder::new()
            .with_middleware(Arc::new(TestMiddleware::new("test1")))
            .with_middleware(Arc::new(TestMiddleware::new("test2")))
            .with_handler(Arc::new(TestHandler))
            .build();
        
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.middleware_names(), vec!["test1", "test2"]);
    }
    
    #[test]
    fn test_middleware_stack_execution() {
        let stack = CliStackBuilder::new()
            .with_middleware(Arc::new(TestMiddleware::new("test")))
            .with_handler(Arc::new(TestHandler))
            .build();
        
        let context = CliContext::new("test".to_string(), vec![]);
        let result = stack.process(CliOperation::Command { args: vec![] }, &context);
        
        assert!(result.is_ok());
        let value = result.unwrap();
        assert_eq!(value["status"], "success");
    }
}