//! Effect-based middleware for dependency injection

use super::{MiddlewareContext, AuraMiddleware};
use crate::effects::Effects;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::marker::PhantomData;

/// Middleware that provides effect injection and scoping
pub struct EffectMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Effect injector for providing scoped effects
    injector: Arc<dyn EffectInjector>,
    
    /// Effect scope configuration
    scope_config: EffectScopeConfig,
    
    /// Phantom data to use type parameters
    _phantom: PhantomData<(Req, Resp, Err)>,
}

impl<Req, Resp, Err> EffectMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create new effect middleware
    pub fn new(injector: Arc<dyn EffectInjector>) -> Self {
        Self {
            injector,
            scope_config: EffectScopeConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create effect middleware with custom scope configuration
    pub fn with_scope_config(injector: Arc<dyn EffectInjector>, scope_config: EffectScopeConfig) -> Self {
        Self {
            injector,
            scope_config,
            _phantom: PhantomData,
        }
    }
}

impl<Req, Resp, Err> AuraMiddleware for EffectMiddleware<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static + From<Box<dyn std::error::Error + Send + Sync>>,
{
    type Request = Req;
    type Response = Resp;
    type Error = Err;

    fn process<'a>(
        &'a self,
        request: Self::Request,
        context: &'a MiddlewareContext,
        effects: &'a dyn Effects,
        next: Box<dyn super::traits::MiddlewareHandler<Self::Request, Self::Response, Self::Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a>> {
        let injector = self.injector.clone();
        let scope_config = self.scope_config.clone();
        let context = context.clone();

        Box::pin(async move {
            // Create scoped effects
            let scoped_effects = injector.create_scope(&context, effects, &scope_config)?;
            
            // Execute next handler with scoped effects
            let result = next.handle(request, &context, scoped_effects.as_ref()).await;
            
            // Cleanup scope
            injector.cleanup_scope(&context, scoped_effects.as_ref())?;
            
            result
        })
    }
}

/// Effect injector trait for creating scoped effects
pub trait EffectInjector: Send + Sync {
    /// Create a scoped effect context
    fn create_scope(
        &self,
        context: &MiddlewareContext,
        parent_effects: &dyn Effects,
        scope_config: &EffectScopeConfig,
    ) -> Result<Box<dyn Effects>, Box<dyn std::error::Error + Send + Sync>>;

    /// Cleanup a scoped effect context
    fn cleanup_scope(
        &self,
        context: &MiddlewareContext,
        scoped_effects: &dyn Effects,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Get available effect types
    fn available_effects(&self) -> Vec<String>;

    /// Check if an effect type is supported
    fn supports_effect(&self, effect_type: &str) -> bool {
        self.available_effects().contains(&effect_type.to_string())
    }
}

/// Configuration for effect scoping
#[derive(Debug, Clone)]
pub struct EffectScopeConfig {
    /// Component identifier for this scope
    pub component_id: String,
    
    /// Whether to isolate time effects
    pub isolate_time: bool,
    
    /// Whether to isolate crypto effects
    pub isolate_crypto: bool,
    
    /// Whether to isolate storage effects
    pub isolate_storage: bool,
    
    /// Whether to isolate network effects
    pub isolate_network: bool,
    
    /// Whether to isolate random effects
    pub isolate_random: bool,
    
    /// Whether to isolate console effects
    pub isolate_console: bool,
    
    /// Custom effect overrides
    pub effect_overrides: std::collections::HashMap<String, serde_json::Value>,
    
    /// Resource limits for this scope
    pub resource_limits: ResourceLimits,
}

impl Default for EffectScopeConfig {
    fn default() -> Self {
        Self {
            component_id: "default".to_string(),
            isolate_time: false,
            isolate_crypto: false,
            isolate_storage: true,  // Storage should be isolated by default
            isolate_network: true,  // Network should be isolated by default
            isolate_random: false,
            isolate_console: false,
            effect_overrides: std::collections::HashMap::new(),
            resource_limits: ResourceLimits::default(),
        }
    }
}

/// Resource limits for effect scopes
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum memory usage in bytes
    pub max_memory_bytes: Option<u64>,
    
    /// Maximum execution time
    pub max_execution_time: Option<std::time::Duration>,
    
    /// Maximum number of storage operations
    pub max_storage_ops: Option<u64>,
    
    /// Maximum number of network requests
    pub max_network_requests: Option<u64>,
    
    /// Maximum file descriptor count
    pub max_file_descriptors: Option<u64>,
    
    /// Maximum CPU time
    pub max_cpu_time: Option<std::time::Duration>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: Some(100 * 1024 * 1024), // 100MB
            max_execution_time: Some(std::time::Duration::from_secs(30)),
            max_storage_ops: Some(1000),
            max_network_requests: Some(100),
            max_file_descriptors: Some(100),
            max_cpu_time: Some(std::time::Duration::from_secs(10)),
        }
    }
}

/// Effect context that tracks scope information
pub struct EffectContext {
    /// Unique context identifier
    pub context_id: String,
    
    /// Parent context (if any)
    pub parent_context_id: Option<String>,
    
    /// Component that owns this context
    pub component_id: String,
    
    /// Scope configuration
    pub scope_config: EffectScopeConfig,
    
    /// Context creation time
    pub created_at: std::time::Instant,
    
    /// Resource usage tracking
    pub resource_usage: ResourceUsage,
    
    /// Context metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl EffectContext {
    /// Create a new effect context
    pub fn new(component_id: &str, scope_config: EffectScopeConfig) -> Self {
        Self {
            context_id: format!("ctx_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()),
            parent_context_id: None,
            component_id: component_id.to_string(),
            scope_config,
            created_at: std::time::Instant::now(),
            resource_usage: ResourceUsage::default(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create a child context
    pub fn create_child(&self, component_id: &str, scope_config: EffectScopeConfig) -> Self {
        Self {
            context_id: format!("ctx_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()),
            parent_context_id: Some(self.context_id.clone()),
            component_id: component_id.to_string(),
            scope_config,
            created_at: std::time::Instant::now(),
            resource_usage: ResourceUsage::default(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Get the age of this context
    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Check if resource limits are exceeded
    pub fn check_limits(&self) -> Result<(), ResourceLimitError> {
        let limits = &self.scope_config.resource_limits;
        
        if let Some(max_memory) = limits.max_memory_bytes {
            if self.resource_usage.memory_bytes > max_memory {
                return Err(ResourceLimitError::MemoryLimitExceeded {
                    used: self.resource_usage.memory_bytes,
                    limit: max_memory,
                });
            }
        }
        
        if let Some(max_time) = limits.max_execution_time {
            if self.age() > max_time {
                return Err(ResourceLimitError::TimeLimitExceeded {
                    elapsed: self.age(),
                    limit: max_time,
                });
            }
        }
        
        if let Some(max_storage_ops) = limits.max_storage_ops {
            if self.resource_usage.storage_operations > max_storage_ops {
                return Err(ResourceLimitError::StorageOpLimitExceeded {
                    used: self.resource_usage.storage_operations,
                    limit: max_storage_ops,
                });
            }
        }
        
        if let Some(max_network_requests) = limits.max_network_requests {
            if self.resource_usage.network_requests > max_network_requests {
                return Err(ResourceLimitError::NetworkLimitExceeded {
                    used: self.resource_usage.network_requests,
                    limit: max_network_requests,
                });
            }
        }
        
        Ok(())
    }

    /// Update resource usage
    pub fn update_usage<F>(&mut self, updater: F) -> Result<(), ResourceLimitError>
    where
        F: FnOnce(&mut ResourceUsage),
    {
        updater(&mut self.resource_usage);
        self.check_limits()
    }
}

/// Resource usage tracking
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// Memory usage in bytes
    pub memory_bytes: u64,
    
    /// Number of storage operations performed
    pub storage_operations: u64,
    
    /// Number of network requests made
    pub network_requests: u64,
    
    /// Number of file descriptors opened
    pub file_descriptors: u64,
    
    /// CPU time used
    pub cpu_time: std::time::Duration,
}

/// Resource limit violation errors
#[derive(Debug, thiserror::Error)]
pub enum ResourceLimitError {
    #[error("Memory limit exceeded: used {used} bytes, limit {limit} bytes")]
    MemoryLimitExceeded { used: u64, limit: u64 },
    
    #[error("Time limit exceeded: elapsed {elapsed:?}, limit {limit:?}")]
    TimeLimitExceeded { elapsed: std::time::Duration, limit: std::time::Duration },
    
    #[error("Storage operation limit exceeded: used {used}, limit {limit}")]
    StorageOpLimitExceeded { used: u64, limit: u64 },
    
    #[error("Network request limit exceeded: used {used}, limit {limit}")]
    NetworkLimitExceeded { used: u64, limit: u64 },
    
    #[error("File descriptor limit exceeded: used {used}, limit {limit}")]
    FileDescriptorLimitExceeded { used: u64, limit: u64 },
    
    #[error("CPU time limit exceeded: used {used:?}, limit {limit:?}")]
    CpuTimeLimitExceeded { used: std::time::Duration, limit: std::time::Duration },
}

/// Effect scope for managing effect lifecycles
pub struct EffectScope {
    /// Scope context
    pub context: EffectContext,
    
    /// Scoped effects instance
    pub effects: Box<dyn Effects>,
    
    /// Cleanup handlers
    cleanup_handlers: Vec<Box<dyn FnOnce() -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send>>,
}

impl EffectScope {
    /// Create a new effect scope
    pub fn new(context: EffectContext, effects: Box<dyn Effects>) -> Self {
        Self {
            context,
            effects,
            cleanup_handlers: Vec::new(),
        }
    }

    /// Add a cleanup handler
    pub fn add_cleanup_handler<F>(&mut self, handler: F)
    where
        F: FnOnce() -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
    {
        self.cleanup_handlers.push(Box::new(handler));
    }

    /// Run all cleanup handlers
    pub fn cleanup(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        for handler in self.cleanup_handlers {
            handler()?;
        }
        Ok(())
    }

    /// Get the effects instance
    pub fn effects(&self) -> &dyn Effects {
        self.effects.as_ref()
    }

    /// Get mutable access to the context
    pub fn context_mut(&mut self) -> &mut EffectContext {
        &mut self.context
    }

    /// Check resource limits
    pub fn check_limits(&self) -> Result<(), ResourceLimitError> {
        self.context.check_limits()
    }
}