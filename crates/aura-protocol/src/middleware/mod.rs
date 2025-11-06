//! Shared Middleware Architecture for Aura
//!
//! This module provides the foundational middleware traits and patterns that enable
//! the algebraic effect-style middleware architecture across all Aura components.
//!
//! ## Design Principles
//!
//! 1. **Composable Middleware**: Middleware can be composed and stacked
//! 2. **Type-Level Safety**: Middleware composition is checked at compile time
//! 3. **Effect Isolation**: Effects are passed through middleware layers
//! 4. **Zero-Cost Abstractions**: Middleware compiles to efficient code
//! 5. **Protocol Agnostic**: Works with any protocol or component
//!
//! ## Architecture Overview
//!
//! The middleware system uses algebraic effects and handlers to provide:
//! - **Request/Response Processing**: Handle incoming and outgoing requests
//! - **Effect Injection**: Provide effects (time, crypto, storage) to handlers
//! - **State Management**: Maintain middleware-specific state
//! - **Error Handling**: Consistent error handling across layers
//! - **Metrics and Observability**: Built-in metrics collection
//!
//! ## Usage Pattern
//!
//! ```rust
//! use aura_types::middleware::*;
//! use aura_protocol::effects::Effects;
//!
//! // Define a protocol handler using derive macro
//! #[derive(AuraHandler)]
//! #[handler(protocol = "dkd", middleware = "default")]
//! struct DkdHandler {
//!     threshold: u32,
//!     participants: Vec<DeviceId>,
//! }
//!
//! // Implement the handler trait
//! impl ProtocolHandler for DkdHandler {
//!     type Request = DkdRequest;
//!     type Response = DkdResponse;
//!     type Error = ProtocolError;
//!
//!     async fn handle(&mut self, request: Self::Request, effects: &dyn Effects) -> Result<Self::Response, Self::Error> {
//!         // Handle the DKD protocol request
//!         todo!()
//!     }
//! }
//!
//! // Create middleware stack
//! let middleware_stack = MiddlewareStack::new()
//!     .with_metrics()
//!     .with_auth()
//!     .with_rate_limiting()
//!     .with_logging();
//!
//! // Use with handler
//! let handler = DkdHandler::new(3, participants);
//! let result = middleware_stack.execute(handler, request, effects).await?;
//! ```

pub mod traits;
pub mod stack;
pub mod effects;
pub mod auth;
pub mod metrics;
pub mod logging;
pub mod errors;

// Re-export core middleware types
pub use traits::{
    MiddlewareHandler, ProtocolHandler, RequestHandler, ResponseHandler
};
// Note: MiddlewareContext, MiddlewareResult, HandlerMetadata, PerformanceProfile 
// are defined later in this module and don't need re-export
pub use stack::{
    MiddlewareStack, MiddlewareLayer, LayerConfig, StackBuilder
};
pub use effects::{
    EffectMiddleware, EffectInjector, EffectContext, EffectScope
};
pub use auth::{
    AuthMiddleware, AuthContext, AuthPolicy, Permission
};
pub use metrics::{
    MetricsMiddleware, MetricsCollector, MetricEvent, MetricType
};
pub use logging::{
    LoggingMiddleware, LogContext, LogLevel, StructuredLogger
};
pub use errors::{
    MiddlewareError, HandlerError, ErrorContext, ErrorHandler
};

use crate::effects::Effects;
use aura_types::AuraError;
use std::future::Future;
use std::pin::Pin;

/// Universal middleware interface for all Aura components
pub trait AuraMiddleware: Send + Sync {
    /// The type of requests this middleware processes
    type Request: Send + Sync;
    
    /// The type of responses this middleware produces
    type Response: Send + Sync;
    
    /// The type of errors this middleware can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Process a request through this middleware layer
    fn process<'a>(
        &'a self,
        request: Self::Request,
        context: &'a MiddlewareContext,
        effects: &'a dyn Effects,
        next: Box<dyn MiddlewareHandler<Self::Request, Self::Response, Self::Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a>>;

    /// Get metadata about this middleware
    fn metadata(&self) -> HandlerMetadata {
        HandlerMetadata::default()
    }

    /// Initialize the middleware (called once at startup)
    fn initialize(&mut self, _context: &MiddlewareContext) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Shutdown the middleware (called once at shutdown)
    fn shutdown(&mut self, _context: &MiddlewareContext) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Middleware execution context
#[derive(Debug, Clone)]
pub struct MiddlewareContext {
    /// Unique execution ID for this request
    pub execution_id: String,
    
    /// Component that initiated this request
    pub component: String,
    
    /// Protocol being executed
    pub protocol: Option<String>,
    
    /// Session information
    pub session_id: Option<String>,
    
    /// Device ID
    pub device_id: Option<String>,
    
    /// Request timestamp
    pub timestamp: std::time::Instant,
    
    /// Additional context data
    pub metadata: std::collections::HashMap<String, String>,
}

impl MiddlewareContext {
    /// Create a new middleware context
    pub fn new(component: &str) -> Self {
        Self {
            execution_id: format!("exec_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()),
            component: component.to_string(),
            protocol: None,
            session_id: None,
            device_id: None,
            timestamp: std::time::Instant::now(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Set the protocol for this context
    pub fn with_protocol(mut self, protocol: &str) -> Self {
        self.protocol = Some(protocol.to_string());
        self
    }

    /// Set the session ID for this context
    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }

    /// Set the device ID for this context
    pub fn with_device(mut self, device_id: &str) -> Self {
        self.device_id = Some(device_id.to_string());
        self
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Get elapsed time since context creation
    pub fn elapsed(&self) -> std::time::Duration {
        self.timestamp.elapsed()
    }
}

impl Default for MiddlewareContext {
    fn default() -> Self {
        Self::new("unknown")
    }
}

/// Result type for middleware operations
pub type MiddlewareResult<T> = Result<T, MiddlewareError>;

/// Handler metadata for describing middleware capabilities
#[derive(Debug, Clone, Default)]
pub struct HandlerMetadata {
    /// Name of the handler/middleware
    pub name: String,
    
    /// Version of the handler
    pub version: String,
    
    /// Description of what this handler does
    pub description: String,
    
    /// Supported protocols
    pub supported_protocols: Vec<String>,
    
    /// Required effects
    pub required_effects: Vec<String>,
    
    /// Performance characteristics
    pub performance_profile: PerformanceProfile,
    
    /// Configuration schema
    pub config_schema: Option<serde_json::Value>,
}

/// Performance characteristics of a middleware/handler
#[derive(Debug, Clone, Default)]
pub struct PerformanceProfile {
    /// Expected latency in microseconds
    pub expected_latency_us: Option<u64>,
    
    /// Memory usage in bytes
    pub memory_usage_bytes: Option<u64>,
    
    /// CPU usage percentage
    pub cpu_usage_percent: Option<f64>,
    
    /// Throughput requests per second
    pub throughput_rps: Option<u64>,
    
    /// Whether this handler blocks
    pub is_blocking: bool,
    
    /// Whether this handler is stateful
    pub is_stateful: bool,
}

/// Convenience macro for creating middleware contexts
#[macro_export]
macro_rules! middleware_context {
    ($component:expr) => {
        MiddlewareContext::new($component)
    };
    ($component:expr, protocol: $protocol:expr) => {
        MiddlewareContext::new($component).with_protocol($protocol)
    };
    ($component:expr, session: $session:expr) => {
        MiddlewareContext::new($component).with_session($session)
    };
    ($component:expr, device: $device:expr) => {
        MiddlewareContext::new($component).with_device($device)
    };
    ($component:expr, $($key:expr => $value:expr),+) => {
        {
            let mut ctx = MiddlewareContext::new($component);
            $(
                ctx = ctx.with_metadata($key, $value);
            )+
            ctx
        }
    };
}

/// Convenience macro for creating middleware stacks
#[macro_export]
macro_rules! middleware_stack {
    ($($middleware:expr),+ $(,)?) => {
        {
            let mut stack = MiddlewareStack::new();
            $(
                stack = stack.add_layer($middleware);
            )+
            stack
        }
    };
}