//! Middleware stack implementation for composing middleware layers

use super::{
    traits::*, MiddlewareContext, HandlerMetadata, AuraMiddleware
};
use crate::effects::AuraEffects;
use crate::AuraError;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Configuration for a middleware layer
#[derive(Debug, Clone)]
pub struct LayerConfig {
    /// Name of the layer
    pub name: String,
    
    /// Whether this layer is enabled
    pub enabled: bool,
    
    /// Priority (higher numbers execute first)
    pub priority: u32,
    
    /// Layer-specific configuration
    pub config: serde_json::Value,
    
    /// Tags for categorizing layers
    pub tags: Vec<String>,
}

impl LayerConfig {
    /// Create a new layer configuration
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            enabled: true,
            priority: 100,
            config: serde_json::Value::Null,
            tags: Vec::new(),
        }
    }

    /// Set the enabled state
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the priority
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Set the configuration
    pub fn config(mut self, config: serde_json::Value) -> Self {
        self.config = config;
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self::new("default")
    }
}

/// A middleware layer in the stack
pub struct MiddlewareLayer<Req, Resp, Err> 
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Layer configuration
    pub config: LayerConfig,
    
    /// The middleware handler
    pub handler: Arc<dyn AuraMiddleware<Request = Req, Response = Resp, Error = Err>>,
}

impl<Req, Resp, Err> MiddlewareLayer<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create a new middleware layer
    pub fn new<M>(handler: M, config: LayerConfig) -> Self 
    where
        M: AuraMiddleware<Request = Req, Response = Resp, Error = Err> + 'static,
    {
        Self {
            config,
            handler: Arc::new(handler),
        }
    }

    /// Check if this layer is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get layer metadata
    pub fn metadata(&self) -> HandlerMetadata {
        self.handler.metadata()
    }
}

/// Middleware stack that composes multiple layers
pub struct MiddlewareStack<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// The layers in the stack (sorted by priority)
    layers: Vec<MiddlewareLayer<Req, Resp, Err>>,
    
    /// Stack configuration
    config: StackConfig,
}

/// Configuration for the middleware stack
#[derive(Debug, Clone)]
pub struct StackConfig {
    /// Maximum execution time for the entire stack
    pub max_execution_time: Option<std::time::Duration>,
    
    /// Whether to continue on layer errors
    pub continue_on_error: bool,
    
    /// Maximum number of layers
    pub max_layers: Option<usize>,
    
    /// Whether to collect metrics for the stack
    pub collect_metrics: bool,
    
    /// Whether to log stack execution
    pub enable_logging: bool,
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            max_execution_time: Some(std::time::Duration::from_secs(30)),
            continue_on_error: false,
            max_layers: Some(50),
            collect_metrics: true,
            enable_logging: true,
        }
    }
}

impl<Req, Resp, Err> MiddlewareStack<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create a new middleware stack
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            config: StackConfig::default(),
        }
    }

    /// Create a new middleware stack with configuration
    pub fn with_config(config: StackConfig) -> Self {
        Self {
            layers: Vec::new(),
            config,
        }
    }

    /// Add a middleware layer to the stack
    pub fn add_layer<M>(mut self, middleware: M) -> Self
    where
        M: AuraMiddleware<Request = Req, Response = Resp, Error = Err> + 'static,
    {
        let config = LayerConfig::new(&format!("layer_{}", self.layers.len()));
        let layer = MiddlewareLayer::new(middleware, config);
        self.layers.push(layer);
        self.sort_layers();
        self
    }

    /// Add a middleware layer with configuration
    pub fn add_layer_with_config<M>(mut self, middleware: M, config: LayerConfig) -> Self
    where
        M: AuraMiddleware<Request = Req, Response = Resp, Error = Err> + 'static,
    {
        let layer = MiddlewareLayer::new(middleware, config);
        self.layers.push(layer);
        self.sort_layers();
        self
    }

    /// Execute the middleware stack
    pub async fn execute(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        final_handler: Box<dyn MiddlewareHandler<Req, Resp, Err>>,
    ) -> Result<Resp, Err> {
        let start_time = std::time::Instant::now();
        
        // Check timeout
        if let Some(max_time) = self.config.max_execution_time {
            if start_time.elapsed() > max_time {
                return Err(self.timeout_error());
            }
        }

        // Filter enabled layers
        let enabled_layers: Vec<_> = self.layers
            .iter()
            .filter(|layer| layer.is_enabled())
            .collect();

        // Execute layers in order
        self.execute_layers(request, context, effects, &enabled_layers, 0, final_handler).await
    }

    /// Recursively execute middleware layers
    fn execute_layers(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        layers: &[&MiddlewareLayer<Req, Resp, Err>],
        layer_index: usize,
        final_handler: Box<dyn MiddlewareHandler<Req, Resp, Err>>,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Err>> + Send + '_>> {
        Box::pin(async move {
            if layer_index >= layers.len() {
                // No more layers, execute final handler
                return final_handler.handle(request, context, effects).await;
            }

            let current_layer = layers[layer_index];
            
            // Create next handler that continues the chain
            let next_handler = NextHandler {
                stack: self,
                layers,
                layer_index: layer_index + 1,
                final_handler,
            };

            // Execute current layer
            current_layer.handler.process(
                request,
                context,
                effects,
                Box::new(next_handler),
            ).await
        })
    }

    /// Sort layers by priority (highest first)
    fn sort_layers(&mut self) {
        self.layers.sort_by(|a, b| b.config.priority.cmp(&a.config.priority));
    }

    /// Get the number of layers in the stack
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Get metadata for all layers
    pub fn stack_metadata(&self) -> Vec<HandlerMetadata> {
        self.layers.iter().map(|layer| layer.metadata()).collect()
    }

    /// Get stack configuration
    pub fn config(&self) -> &StackConfig {
        &self.config
    }

    /// Update stack configuration
    pub fn update_config(&mut self, config: StackConfig) {
        self.config = config;
    }

    /// Remove layers by tag
    pub fn remove_layers_by_tag(&mut self, tag: &str) {
        self.layers.retain(|layer| !layer.config.tags.contains(&tag.to_string()));
    }

    /// Enable/disable layers by tag
    pub fn set_layers_enabled_by_tag(&mut self, tag: &str, enabled: bool) {
        for layer in &mut self.layers {
            if layer.config.tags.contains(&tag.to_string()) {
                layer.config.enabled = enabled;
            }
        }
    }

    /// Find layers by tag
    pub fn find_layers_by_tag(&self, tag: &str) -> Vec<&MiddlewareLayer<Req, Resp, Err>> {
        self.layers
            .iter()
            .filter(|layer| layer.config.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Create a timeout error (placeholder)
    fn timeout_error(&self) -> Err {
        // This is a placeholder - in real implementation would convert from AuraError
        panic!("Timeout error conversion not implemented")
    }
}

impl<Req, Resp, Err> Default for MiddlewareStack<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Handler for continuing middleware chain execution
struct NextHandler<'a, Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    stack: &'a MiddlewareStack<Req, Resp, Err>,
    layers: &'a [&'a MiddlewareLayer<Req, Resp, Err>],
    layer_index: usize,
    final_handler: Box<dyn MiddlewareHandler<Req, Resp, Err>>,
}

impl<'a, Req, Resp, Err> MiddlewareHandler<Req, Resp, Err> for NextHandler<'a, Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    fn handle(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Err>> + Send>> {
        self.stack.execute_layers(
            request,
            context,
            effects,
            self.layers,
            self.layer_index,
            // Note: This is a simplified version - real implementation would handle handler ownership properly
            Box::new(FinalHandlerProxy { handler: &self.final_handler }),
        )
    }
}

/// Proxy for the final handler to work around ownership issues
struct FinalHandlerProxy<'a, Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    handler: &'a Box<dyn MiddlewareHandler<Req, Resp, Err>>,
}

impl<'a, Req, Resp, Err> MiddlewareHandler<Req, Resp, Err> for FinalHandlerProxy<'a, Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    fn handle(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Err>> + Send>> {
        self.handler.handle(request, context, effects)
    }
}

/// Builder for creating middleware stacks fluently
pub struct StackBuilder<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    stack: MiddlewareStack<Req, Resp, Err>,
}

impl<Req, Resp, Err> StackBuilder<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create a new stack builder
    pub fn new() -> Self {
        Self {
            stack: MiddlewareStack::new(),
        }
    }

    /// Add a middleware layer
    pub fn with_layer<M>(mut self, middleware: M) -> Self
    where
        M: AuraMiddleware<Request = Req, Response = Resp, Error = Err> + 'static,
    {
        self.stack = self.stack.add_layer(middleware);
        self
    }

    /// Add a middleware layer with configuration
    pub fn with_layer_config<M>(mut self, middleware: M, config: LayerConfig) -> Self
    where
        M: AuraMiddleware<Request = Req, Response = Resp, Error = Err> + 'static,
    {
        self.stack = self.stack.add_layer_with_config(middleware, config);
        self
    }

    /// Set stack configuration
    pub fn with_stack_config(mut self, config: StackConfig) -> Self {
        self.stack.update_config(config);
        self
    }

    /// Build the middleware stack
    pub fn build(self) -> MiddlewareStack<Req, Resp, Err> {
        self.stack
    }
}

impl<Req, Resp, Err> Default for StackBuilder<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}