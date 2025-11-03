//! Transport Middleware System
//!
//! This module implements the algebraic effect-style middleware pattern for transport operations.
//! All networking functionality is implemented as composable middleware layers that can be
//! stacked and configured for different use cases.

pub mod stack;
pub mod handler;
pub mod connection_pooling;
pub mod rate_limiting;
pub mod circuit_breaker;
pub mod compression;
pub mod encryption;
pub mod discovery;
pub mod reliability;
pub mod monitoring;

// Re-export core middleware types
pub use stack::{TransportMiddlewareStack, TransportStackBuilder};
pub use handler::{TransportHandler, TransportOperation, TransportResult};
pub use aura_types::{MiddlewareContext, MiddlewareResult};

// Re-export middleware implementations
pub use connection_pooling::ConnectionPoolingMiddleware;
pub use rate_limiting::RateLimitingMiddleware;
pub use circuit_breaker::CircuitBreakerMiddleware;
pub use compression::CompressionMiddleware;
pub use encryption::EncryptionMiddleware;
pub use discovery::DiscoveryMiddleware;
pub use reliability::ReliabilityMiddleware;
pub use monitoring::MonitoringMiddleware;