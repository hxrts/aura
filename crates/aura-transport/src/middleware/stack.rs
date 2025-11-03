//! Transport Middleware Stack
//!
//! Provides the middleware composition system for transport operations.

use super::handler::{TransportHandler, TransportOperation, TransportResult};
use aura_types::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::HashMap;

/// Transport middleware trait
pub trait TransportMiddleware: Send + Sync {
    /// Process a transport operation with this middleware layer
    fn process(
        &mut self,
        operation: TransportOperation,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult>;
    
    /// Get middleware name for observability
    fn middleware_name(&self) -> &'static str;
    
    /// Get middleware configuration info
    fn middleware_info(&self) -> HashMap<String, String> {
        HashMap::new()
    }
    
    /// Initialize the middleware
    fn initialize(&mut self, _context: &MiddlewareContext) -> MiddlewareResult<()> {
        Ok(())
    }
    
    /// Shutdown the middleware
    fn shutdown(&mut self, _context: &MiddlewareContext) -> MiddlewareResult<()> {
        Ok(())
    }
}

/// Transport middleware stack that composes multiple middleware layers
pub struct TransportMiddlewareStack {
    middleware_layers: Vec<Box<dyn TransportMiddleware>>,
    base_handler: Box<dyn TransportHandler>,
    context: MiddlewareContext,
}

impl TransportMiddlewareStack {
    /// Create a new transport middleware stack
    pub fn new(base_handler: Box<dyn TransportHandler>) -> Self {
        Self {
            middleware_layers: Vec::new(),
            base_handler,
            context: MiddlewareContext::new("transport".to_string()),
        }
    }
    
    /// Add a middleware layer to the stack
    pub fn add_middleware(mut self, middleware: Box<dyn TransportMiddleware>) -> Self {
        self.middleware_layers.push(middleware);
        self
    }
    
    /// Execute a transport operation through the middleware stack
    pub fn execute(
        &mut self,
        operation: TransportOperation,
        effects: &dyn AuraEffects,
    ) -> MiddlewareResult<TransportResult> {
        if self.middleware_layers.is_empty() {
            // No middleware, execute directly on base handler
            return self.base_handler.execute(operation, effects);
        }
        
        // Process through first middleware layer, which will chain to others
        if let Some(first_middleware) = self.middleware_layers.first_mut() {
            first_middleware.process(
                operation,
                &self.context,
                effects,
                self.base_handler.as_mut(),
            )
        } else {
            // No middleware, execute directly on base handler  
            self.base_handler.execute(operation, effects)
        }
    }
    
    /// Initialize all middleware layers
    pub fn initialize(&mut self) -> MiddlewareResult<()> {
        for middleware in &mut self.middleware_layers {
            middleware.initialize(&self.context)?;
        }
        Ok(())
    }
    
    /// Shutdown all middleware layers
    pub fn shutdown(&mut self) -> MiddlewareResult<()> {
        for middleware in &mut self.middleware_layers {
            middleware.shutdown(&self.context)?;
        }
        Ok(())
    }
    
    /// Get information about the middleware stack
    pub fn stack_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("middleware_count".to_string(), self.middleware_layers.len().to_string());
        
        let middleware_names: Vec<String> = self.middleware_layers
            .iter()
            .map(|m| m.middleware_name().to_string())
            .collect();
        info.insert("middleware_layers".to_string(), middleware_names.join(","));
        
        info
    }
}

impl TransportHandler for TransportMiddlewareStack {
    fn execute(
        &mut self,
        operation: TransportOperation,
        effects: &dyn AuraEffects,
    ) -> MiddlewareResult<TransportResult> {
        self.execute(operation, effects)
    }
    
    fn handler_info(&self) -> HashMap<String, String> {
        let mut info = self.stack_info();
        info.insert("handler_type".to_string(), "TransportMiddlewareStack".to_string());
        info
    }
}

/// Builder for creating transport middleware stacks
pub struct TransportStackBuilder {
    middleware_layers: Vec<Box<dyn TransportMiddleware>>,
    context: Option<MiddlewareContext>,
}

impl TransportStackBuilder {
    /// Create a new stack builder
    pub fn new() -> Self {
        Self {
            middleware_layers: Vec::new(),
            context: None,
        }
    }
    
    /// Add a middleware layer
    pub fn add_layer(mut self, middleware: Box<dyn TransportMiddleware>) -> Self {
        self.middleware_layers.push(middleware);
        self
    }
    
    /// Set the middleware context
    pub fn with_context(mut self, context: MiddlewareContext) -> Self {
        self.context = Some(context);
        self
    }
    
    /// Build the middleware stack with a base handler
    pub fn build(self, base_handler: Box<dyn TransportHandler>) -> TransportMiddlewareStack {
        let mut stack = TransportMiddlewareStack::new(base_handler);
        
        if let Some(context) = self.context {
            stack.context = context;
        }
        
        for middleware in self.middleware_layers {
            stack = stack.add_middleware(middleware);
        }
        
        stack
    }
}

impl Default for TransportStackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait for fluent middleware composition
pub trait TransportHandlerExt: TransportHandler + Sized {
    /// Add a middleware layer to this handler
    fn layer<M: TransportMiddleware + 'static>(self, middleware: M) -> TransportMiddlewareStack
    where
        Self: 'static,
    {
        TransportStackBuilder::new()
            .add_layer(Box::new(middleware))
            .build(Box::new(self))
    }
}

// Implement the extension trait for all transport handlers
impl<T: TransportHandler + 'static> TransportHandlerExt for T {}