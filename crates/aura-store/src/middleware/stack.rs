//! Storage Middleware Stack
//!
//! Provides the middleware composition system for storage operations.

use super::handler::{StorageHandler, StorageOperation, StorageResult};
use aura_protocol::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::HashMap;

/// Storage middleware trait
pub trait StorageMiddleware: Send + Sync {
    /// Process a storage operation with this middleware layer
    fn process(
        &mut self,
        operation: StorageOperation,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult>;
    
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

/// Storage middleware stack that composes multiple middleware layers
pub struct StorageMiddlewareStack {
    middleware_layers: Vec<Box<dyn StorageMiddleware>>,
    base_handler: Box<dyn StorageHandler>,
    context: MiddlewareContext,
}

impl StorageMiddlewareStack {
    /// Create a new storage middleware stack
    pub fn new(base_handler: Box<dyn StorageHandler>) -> Self {
        Self {
            middleware_layers: Vec::new(),
            base_handler,
            context: MiddlewareContext::new("storage".to_string()),
        }
    }
    
    /// Add a middleware layer to the stack
    pub fn add_middleware(mut self, middleware: Box<dyn StorageMiddleware>) -> Self {
        self.middleware_layers.push(middleware);
        self
    }
    
    /// Execute a storage operation through the middleware stack
    pub fn execute(
        &mut self,
        operation: StorageOperation,
        effects: &dyn AuraEffects,
    ) -> MiddlewareResult<StorageResult> {
        if self.middleware_layers.is_empty() {
            // No middleware, execute directly on base handler
            return self.base_handler.execute(operation, effects);
        }
        
        // Create a handler chain that processes through all middleware layers
        let current_operation = operation;
        let current_index = 0;
        
        // Process through middleware layers in order
        while current_index < self.middleware_layers.len() {
            let middleware = &mut self.middleware_layers[current_index];
            
            // Create a mock "next" handler for the current layer
            // In a real implementation, this would properly chain the middleware
            current_operation = match middleware.process(
                current_operation,
                &self.context,
                effects,
                self.base_handler.as_mut(),
            ) {
                Ok(StorageResult::Stored { chunk_id, size }) => {
                    return Ok(StorageResult::Stored { chunk_id, size });
                }
                Ok(result) => return Ok(result),
                Err(e) => return Err(e),
            };
            
            current_index += 1;
        }
        
        // Finally execute on base handler
        self.base_handler.execute(current_operation, effects)
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

impl StorageHandler for StorageMiddlewareStack {
    fn execute(
        &mut self,
        operation: StorageOperation,
        effects: &dyn AuraEffects,
    ) -> MiddlewareResult<StorageResult> {
        self.execute(operation, effects)
    }
    
    fn handler_info(&self) -> HashMap<String, String> {
        let mut info = self.stack_info();
        info.insert("handler_type".to_string(), "StorageMiddlewareStack".to_string());
        info
    }
}

/// Builder for creating storage middleware stacks
pub struct StorageStackBuilder {
    middleware_layers: Vec<Box<dyn StorageMiddleware>>,
    context: Option<MiddlewareContext>,
}

impl StorageStackBuilder {
    /// Create a new stack builder
    pub fn new() -> Self {
        Self {
            middleware_layers: Vec::new(),
            context: None,
        }
    }
    
    /// Add a middleware layer
    pub fn add_layer(mut self, middleware: Box<dyn StorageMiddleware>) -> Self {
        self.middleware_layers.push(middleware);
        self
    }
    
    /// Set the middleware context
    pub fn with_context(mut self, context: MiddlewareContext) -> Self {
        self.context = Some(context);
        self
    }
    
    /// Build the middleware stack with a base handler
    pub fn build(self, base_handler: Box<dyn StorageHandler>) -> StorageMiddlewareStack {
        let mut stack = StorageMiddlewareStack::new(base_handler);
        
        if let Some(context) = self.context {
            stack.context = context;
        }
        
        for middleware in self.middleware_layers {
            stack = stack.add_middleware(middleware);
        }
        
        stack
    }
}

impl Default for StorageStackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait for fluent middleware composition
pub trait StorageHandlerExt: StorageHandler + Sized {
    /// Add a middleware layer to this handler
    fn layer<M: StorageMiddleware + 'static>(self, middleware: M) -> StorageMiddlewareStack
    where
        Self: 'static,
    {
        StorageStackBuilder::new()
            .add_layer(Box::new(middleware))
            .build(Box::new(self))
    }
}

// Implement the extension trait for all storage handlers
impl<T: StorageHandler + 'static> StorageHandlerExt for T {}