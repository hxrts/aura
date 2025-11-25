//! Rate limiting infrastructure for flow budget enforcement
//!
//! **DRY Consolidation**: This module now re-exports unified rate limiting types from aura-core.
//! All rate limiting implementations have been consolidated to eliminate ~350 lines of duplication
//! across aura-sync and provide a unified implementation for all crates.
//!
//! The unified implementation provides:
//! - **Token bucket algorithm**: Efficient rate limiting with burst support
//! - **Per-peer and global limits**: Multi-level rate limiting
//! - **Automatic refill**: Time-based token replenishment
//! - **Statistics tracking**: Monitoring and observability
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::infrastructure::{RateLimiter, RateLimitConfig};
//! use aura_core::DeviceId;
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = RateLimitConfig::default();
//! let mut limiter = RateLimiter::new(config);
//!
//! let peer_id = DeviceId::from_bytes([1; 32]);
//!
//! // Check if operation is allowed
//! match limiter.check_rate_limit(peer_id, 100) {
//!     result if result.is_allowed() => {
//!         // Perform sync operation
//!     }
//!     result => {
//!         // Rate limit exceeded, backoff
//!         if let Some(retry_after) = result.retry_after() {
//!             tokio::time::sleep(retry_after).await;
//!         }
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::time::{Duration, Instant};

// Re-export unified rate limiting types from aura-core
pub use aura_core::{
    RateLimit, RateLimitConfig, RateLimitResult, RateLimiter, RateLimiterStatistics,
};

use crate::core::{sync_resource_exhausted, SyncResult};
use aura_core::DeviceId;

// =============================================================================
// Helper Functions (preserved for backward compatibility)
// =============================================================================

/// Check rate limit and convert to SyncResult (convenience function)
///
/// Callers must supply a monotonic `Instant` from their time provider (e.g., TimeEffects).
pub fn check_rate_limit_sync(
    limiter: &mut RateLimiter,
    peer_id: DeviceId,
    cost: u32,
    now: Instant,
) -> SyncResult<()> {
    limiter
        .check_rate_limit(peer_id, cost, now)
        .into_result()
        .map_err(|e| sync_resource_exhausted("rate_limit", e.to_string()))
}

/// Create a default rate limiter for sync operations (convenience function)
pub fn default_sync_rate_limiter(now: Instant) -> RateLimiter {
    let config = RateLimitConfig {
        global_ops_per_second: 1000,
        peer_ops_per_second: 100,
        bucket_capacity: 200,
        refill_rate: 100,
        window_size: Duration::from_secs(60),
        adaptive: true,
    };
    RateLimiter::new(config, now)
}
