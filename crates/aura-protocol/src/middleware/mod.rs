//! Middleware Architecture for Aura
//!
//! This module provides the foundational middleware traits and patterns for the unified
//! AuraHandler architecture. The middleware system works with the new effect system architecture.
//!
//! ## Design Principles
//!
//! 1. **Unified Architecture**: All middleware works through the AuraHandler trait
//! 2. **Type-Level Safety**: Middleware composition is checked at compile time
//! 3. **Effect Integration**: Direct integration with the AuraEffectSystem
//! 4. **Zero-Cost Abstractions**: Middleware compiles to efficient code
//! 5. **Protocol Agnostic**: Works with any protocol or component

// pub mod circuit_breaker; // REMOVED: Uses deprecated effect interfaces

// pub use circuit_breaker::{
//     CircuitBreakerConfig, CircuitBreakerMiddleware, CircuitBreakerStats, CircuitState,
// }; // REMOVED: Uses deprecated effect interfaces

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
            #[allow(clippy::disallowed_methods)] // Needed for execution ID generation
            execution_id: format!("exec_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()),
            component: component.to_string(),
            protocol: None,
            session_id: None,
            device_id: None,
            #[allow(clippy::disallowed_methods)] // Needed for timestamp generation
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

/// Middleware operation errors
#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    /// Generic middleware error
    #[error("Middleware error: {message}")]
    General {
        /// Error message
        message: String,
    },

    /// Configuration error
    #[error("Configuration error: {reason}")]
    Configuration {
        /// Configuration failure reason
        reason: String,
    },

    /// Handler not found
    #[error("Handler not found: {handler_type}")]
    HandlerNotFound {
        /// Type of handler that was not found
        handler_type: String,
    },

    /// Timeout error
    #[error("Timeout after {duration:?}")]
    TimeoutError {
        /// Duration that caused the timeout
        duration: std::time::Duration,
    },
}

/// Type alias for middleware operation results
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
