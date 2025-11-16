//! Infrastructure utilities for sync operations
//!
//! This module provides supporting infrastructure for synchronization protocols:
//! - Peer discovery and management
//! - Connection pooling and lifecycle management
//! - Retry logic with exponential backoff
//! - Cache management with epoch tracking
//! - Rate limiting for budget enforcement
//!
//! All infrastructure components follow Layer 5 patterns:
//! - Effect-based with no direct handler dependencies
//! - Stateless where possible, explicit state management where needed
//! - Composable through trait-based interfaces

pub mod peers;
pub mod retry;
pub mod cache;
pub mod connections;
pub mod rate_limit;

// Re-export key types for convenience
pub use peers::{PeerManager, PeerInfo, PeerDiscoveryConfig, PeerStatus, PeerMetadata};
pub use retry::{RetryPolicy, BackoffStrategy, RetryContext, RetryResult};
pub use cache::{CacheManager, CacheEpochTracker, CacheInvalidation};
pub use connections::{ConnectionPool, ConnectionMetadata, PoolConfig};
pub use rate_limit::{RateLimiter, RateLimit, RateLimitConfig, RateLimitResult};
