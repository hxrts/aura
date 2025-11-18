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

pub mod cache;
pub mod connections;
pub mod peers;
pub mod rate_limit;
pub mod retry;

// Re-export key types for convenience
pub use cache::{CacheEpochTracker, CacheInvalidation, CacheManager};
pub use connections::{ConnectionMetadata, ConnectionPool, PoolConfig};
pub use peers::{PeerDiscoveryConfig, PeerInfo, PeerManager, PeerMetadata, PeerStatus};
pub use rate_limit::{RateLimit, RateLimitConfig, RateLimitResult, RateLimiter};
pub use retry::{BackoffStrategy, RetryContext, RetryPolicy, RetryResult};
