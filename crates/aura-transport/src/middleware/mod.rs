//! Transport Middleware System
//!
//! This module implements the algebraic effect-style middleware pattern for transport operations.
//! All networking functionality is implemented as composable middleware layers that can be
//! stacked and configured for different use cases.

pub mod circuit_breaker;
pub mod compression;
pub mod connection_pooling;
pub mod discovery;
pub mod encryption;
pub mod handler;
pub mod monitoring;
pub mod rate_limiting;
pub mod reliability;
pub mod stack;

// Re-export core middleware types
pub use aura_protocol::middleware::{MiddlewareContext, MiddlewareResult};
pub use handler::{NetworkAddress, TransportHandler, TransportOperation, TransportResult};
pub use stack::{TransportMiddlewareStack, TransportStackBuilder};

// Re-export middleware implementations
pub use circuit_breaker::CircuitBreakerMiddleware;
pub use compression::CompressionMiddleware;
pub use connection_pooling::ConnectionPoolingMiddleware;
pub use discovery::DiscoveryMiddleware;
pub use encryption::EncryptionMiddleware;
pub use monitoring::MonitoringMiddleware;
pub use rate_limiting::RateLimitingMiddleware;
pub use reliability::ReliabilityMiddleware;
