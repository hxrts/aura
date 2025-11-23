//! Layer 5: Sync Protocol Infrastructure - Peers, Connections, Caching, Rate Limiting
//!
//! Supporting utilities for synchronization protocols:
//! **PeerManager** (discovery & selection), **ConnectionPool** (pooling & reuse),
//! **RetryPolicy** (resilient operations), **CacheManager** (epoch-aware invalidation),
//! **RateLimiter** (flow budget enforcement).
//!
//! **Integration** (per docs/003_information_flow_contract.md):
//! RateLimiter enforces flow budgets; receipts propagate charges up the relay chain.
//! CacheManager tracks epochs to invalidate stale data across authority boundaries.

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
