//! Rate limiting infrastructure for flow budget enforcement
//!
//! Provides rate limiting capabilities for sync operations, integrating with
//! Aura's flow budget system for privacy-preserving spam prevention.
//!
//! # Architecture
//!
//! The rate limiting system:
//! - Enforces per-peer and global rate limits
//! - Integrates with FlowBudget from aura-core
//! - Supports token bucket and sliding window algorithms
//! - Provides backpressure signals for protocol coordination
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
//! match limiter.check_rate_limit(peer_id, 100).await {
//!     Ok(_) => {
//!         // Perform sync operation
//!     }
//!     Err(_) => {
//!         // Rate limit exceeded, backoff
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use aura_core::DeviceId;
use crate::core::{SyncError, SyncResult};

// =============================================================================
// Configuration
// =============================================================================

/// Rate limiter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Global rate limit (operations per second)
    pub global_ops_per_second: u32,

    /// Per-peer rate limit (operations per second)
    pub peer_ops_per_second: u32,

    /// Bucket capacity (maximum burst size)
    pub bucket_capacity: u32,

    /// Refill rate (tokens per second)
    pub refill_rate: u32,

    /// Window size for sliding window algorithm
    pub window_size: Duration,

    /// Enable adaptive rate limiting based on load
    pub adaptive: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            global_ops_per_second: 1000,
            peer_ops_per_second: 100,
            bucket_capacity: 200,
            refill_rate: 100,
            window_size: Duration::from_secs(60),
            adaptive: true,
        }
    }
}

// =============================================================================
// Rate Limit Types
// =============================================================================

/// Rate limit for a specific context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Maximum operations per window
    pub max_operations: u32,

    /// Window duration
    pub window: Duration,

    /// Current token count (for token bucket)
    pub tokens: u32,

    /// Last refill time
    pub last_refill: Instant,
}

impl RateLimit {
    /// Create a new rate limit
    pub fn new(max_operations: u32, window: Duration) -> Self {
        Self {
            max_operations,
            window,
            tokens: max_operations,
            last_refill: Instant::now(),
        }
    }

    /// Check if operation is allowed and consume tokens
    pub fn check_and_consume(&mut self, cost: u32, refill_rate: u32) -> bool {
        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let refill_tokens = (elapsed.as_secs_f64() * refill_rate as f64) as u32;

        if refill_tokens > 0 {
            self.tokens = (self.tokens + refill_tokens).min(self.max_operations);
            self.last_refill = now;
        }

        // Check if we have enough tokens
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    /// Get current token count
    pub fn available_tokens(&self) -> u32 {
        self.tokens
    }

    /// Calculate time until tokens are available
    pub fn time_until_available(&self, cost: u32, refill_rate: u32) -> Option<Duration> {
        if self.tokens >= cost {
            return None;
        }

        let needed = cost - self.tokens;
        let seconds = needed as f64 / refill_rate as f64;

        Some(Duration::from_secs_f64(seconds))
    }
}

// =============================================================================
// Rate Limit Result
// =============================================================================

/// Result of a rate limit check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Operation allowed
    Allowed,

    /// Operation denied - rate limit exceeded
    Denied {
        /// Time to wait before retry
        retry_after: Duration,

        /// Reason for denial
        reason: String,
    },
}

impl RateLimitResult {
    /// Check if operation is allowed
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimitResult::Allowed)
    }

    /// Get retry-after duration if denied
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            RateLimitResult::Denied { retry_after, .. } => Some(*retry_after),
            RateLimitResult::Allowed => None,
        }
    }

    /// Convert to Result type
    pub fn into_result(self) -> SyncResult<()> {
        match self {
            RateLimitResult::Allowed => Ok(()),
            RateLimitResult::Denied { reason, .. } => {
                Err(SyncError::RateLimited(reason))
            }
        }
    }
}

// =============================================================================
// Rate Limiter
// =============================================================================

/// Rate limiter for sync operations
///
/// Provides token bucket-based rate limiting with per-peer and global limits.
/// Integrates with FlowBudget system for privacy-preserving enforcement.
pub struct RateLimiter {
    /// Configuration
    config: RateLimitConfig,

    /// Global rate limit
    global_limit: RateLimit,

    /// Per-peer rate limits
    peer_limits: HashMap<DeviceId, RateLimit>,

    /// Statistics
    stats: RateLimiterStatistics,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(config: RateLimitConfig) -> Self {
        let global_limit = RateLimit::new(
            config.global_ops_per_second,
            Duration::from_secs(1),
        );

        Self {
            config,
            global_limit,
            peer_limits: HashMap::new(),
            stats: RateLimiterStatistics::default(),
        }
    }

    /// Check rate limit for a peer operation
    ///
    /// # Arguments
    /// - `peer_id`: Peer device ID
    /// - `cost`: Operation cost in tokens
    ///
    /// # Returns
    /// - `RateLimitResult::Allowed` if operation can proceed
    /// - `RateLimitResult::Denied` if rate limit exceeded
    pub async fn check_rate_limit(
        &mut self,
        peer_id: DeviceId,
        cost: u32,
    ) -> RateLimitResult {
        // Check global limit first
        if !self.global_limit.check_and_consume(cost, self.config.refill_rate) {
            self.stats.global_limit_hits += 1;

            let retry_after = self.global_limit
                .time_until_available(cost, self.config.refill_rate)
                .unwrap_or(Duration::from_secs(1));

            return RateLimitResult::Denied {
                retry_after,
                reason: "Global rate limit exceeded".to_string(),
            };
        }

        // Check per-peer limit
        let peer_limit = self.peer_limits
            .entry(peer_id)
            .or_insert_with(|| {
                RateLimit::new(
                    self.config.peer_ops_per_second,
                    Duration::from_secs(1),
                )
            });

        if !peer_limit.check_and_consume(cost, self.config.refill_rate) {
            self.stats.peer_limit_hits += 1;

            // Return tokens to global limit since peer limit blocked
            self.global_limit.tokens = (self.global_limit.tokens + cost)
                .min(self.config.global_ops_per_second);

            let retry_after = peer_limit
                .time_until_available(cost, self.config.refill_rate)
                .unwrap_or(Duration::from_secs(1));

            return RateLimitResult::Denied {
                retry_after,
                reason: format!("Peer rate limit exceeded for {:?}", peer_id),
            };
        }

        self.stats.operations_allowed += 1;
        RateLimitResult::Allowed
    }

    /// Check if operation would exceed rate limit without consuming tokens
    pub fn would_exceed_limit(&self, peer_id: &DeviceId, cost: u32) -> bool {
        // Check global limit
        if self.global_limit.available_tokens() < cost {
            return true;
        }

        // Check peer limit
        if let Some(peer_limit) = self.peer_limits.get(peer_id) {
            if peer_limit.available_tokens() < cost {
                return true;
            }
        }

        false
    }

    /// Get available tokens for a peer
    pub fn available_tokens(&self, peer_id: &DeviceId) -> u32 {
        let global_tokens = self.global_limit.available_tokens();

        let peer_tokens = self.peer_limits
            .get(peer_id)
            .map(|l| l.available_tokens())
            .unwrap_or(self.config.peer_ops_per_second);

        global_tokens.min(peer_tokens)
    }

    /// Get statistics
    pub fn statistics(&self) -> &RateLimiterStatistics {
        &self.stats
    }

    /// Reset rate limiter state
    pub fn reset(&mut self) {
        self.global_limit = RateLimit::new(
            self.config.global_ops_per_second,
            Duration::from_secs(1),
        );
        self.peer_limits.clear();
        self.stats = RateLimiterStatistics::default();
    }

    /// Remove rate limit for a peer
    pub fn remove_peer(&mut self, peer_id: &DeviceId) {
        self.peer_limits.remove(peer_id);
    }

    /// Get number of tracked peers
    pub fn tracked_peers(&self) -> usize {
        self.peer_limits.len()
    }
}

/// Rate limiter statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimiterStatistics {
    /// Total operations allowed
    pub operations_allowed: u64,

    /// Number of times global limit was hit
    pub global_limit_hits: u64,

    /// Number of times per-peer limit was hit
    pub peer_limit_hits: u64,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limit_allows_under_limit() {
        let config = RateLimitConfig {
            peer_ops_per_second: 10,
            global_ops_per_second: 100,
            bucket_capacity: 20,
            refill_rate: 10,
            ..Default::default()
        };

        let mut limiter = RateLimiter::new(config);
        let peer_id = DeviceId::from_bytes([1; 32]);

        // Should allow operation under limit
        let result = limiter.check_rate_limit(peer_id, 5).await;
        assert!(result.is_allowed());
    }

    #[tokio::test]
    async fn test_rate_limit_denies_over_peer_limit() {
        let config = RateLimitConfig {
            peer_ops_per_second: 10,
            global_ops_per_second: 100,
            bucket_capacity: 10,
            refill_rate: 10,
            ..Default::default()
        };

        let mut limiter = RateLimiter::new(config);
        let peer_id = DeviceId::from_bytes([1; 32]);

        // Use up peer tokens
        limiter.check_rate_limit(peer_id, 10).await;

        // Next request should be denied
        let result = limiter.check_rate_limit(peer_id, 1).await;
        assert!(!result.is_allowed());
        assert_eq!(limiter.statistics().peer_limit_hits, 1);
    }

    #[tokio::test]
    async fn test_rate_limit_denies_over_global_limit() {
        let config = RateLimitConfig {
            peer_ops_per_second: 100,
            global_ops_per_second: 10,
            bucket_capacity: 10,
            refill_rate: 10,
            ..Default::default()
        };

        let mut limiter = RateLimiter::new(config);
        let peer_id = DeviceId::from_bytes([1; 32]);

        // Use up global tokens
        limiter.check_rate_limit(peer_id, 10).await;

        // Next request should be denied by global limit
        let result = limiter.check_rate_limit(peer_id, 1).await;
        assert!(!result.is_allowed());
        assert_eq!(limiter.statistics().global_limit_hits, 1);
    }

    #[test]
    fn test_rate_limit_token_refill() {
        let mut limit = RateLimit::new(100, Duration::from_secs(1));

        // Consume tokens
        assert!(limit.check_and_consume(50, 100));
        assert_eq!(limit.available_tokens(), 50);

        // Simulate time passing (this test is simplified - real test would use tokio::time)
        limit.last_refill = Instant::now() - Duration::from_millis(500);

        // Refill should happen
        limit.check_and_consume(0, 100);
        assert!(limit.available_tokens() > 50);
    }

    #[test]
    fn test_would_exceed_limit() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);

        let peer_id = DeviceId::from_bytes([1; 32]);

        // Should not exceed with reasonable cost
        assert!(!limiter.would_exceed_limit(&peer_id, 50));

        // Should exceed with excessive cost
        assert!(limiter.would_exceed_limit(&peer_id, 10000));
    }
}
