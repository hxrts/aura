//! Rate limiting middleware for operation throttling

use super::{JournalMiddleware, JournalHandler, JournalContext};
use crate::error::{Error, Result};
use crate::operations::JournalOperation;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Rate limiting middleware that throttles operations
pub struct RateLimitingMiddleware {
    /// Rate limiter storage
    limiters: Arc<RwLock<HashMap<String, TokenBucket>>>,
    
    /// Configuration
    config: RateLimitingConfig,
}

impl RateLimitingMiddleware {
    /// Create new rate limiting middleware
    pub fn new(config: RateLimitingConfig) -> Self {
        Self {
            limiters: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }
    
    /// Get rate limiting statistics
    pub fn stats(&self) -> RateLimitingStats {
        let limiters = self.limiters.read().unwrap();
        let mut total_requests = 0;
        let mut total_allowed = 0;
        let mut total_rejected = 0;
        
        for bucket in limiters.values() {
            total_requests += bucket.total_requests();
            total_allowed += bucket.allowed_requests();
            total_rejected += bucket.rejected_requests();
        }
        
        RateLimitingStats {
            active_limiters: limiters.len(),
            total_requests,
            total_allowed,
            total_rejected,
            rejection_rate: if total_requests > 0 {
                total_rejected as f64 / total_requests as f64
            } else {
                0.0
            },
        }
    }
    
    /// Clear all rate limiters
    pub fn clear(&self) {
        self.limiters.write().unwrap().clear();
    }
}

impl JournalMiddleware for RateLimitingMiddleware {
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value> {
        // Skip rate limiting if disabled
        if !self.config.enable_rate_limiting {
            return next.handle(operation, context);
        }
        
        // Determine rate limiting key
        let key = self.get_rate_limit_key(&operation, context);
        
        // Get or create token bucket
        let mut limiters = self.limiters.write().map_err(|_| {
            Error::storage_failed("Failed to acquire write lock on rate limiters")
        })?;
        
        let bucket = limiters.entry(key).or_insert_with(|| {
            TokenBucket::new(self.config.default_rate_limit.clone())
        });
        
        // Check rate limit
        if !bucket.try_consume() {
            return Err(Error::invalid_operation(format!(
                "Rate limit exceeded for operation {:?}",
                operation
            )));
        }
        
        // Rate limit passed, proceed with operation
        next.handle(operation, context)
    }
    
    fn name(&self) -> &str {
        "rate_limiting"
    }
}

impl RateLimitingMiddleware {
    fn get_rate_limit_key(&self, operation: &JournalOperation, context: &JournalContext) -> String {
        match self.config.rate_limit_scope {
            RateLimitScope::Global => {
                "global".to_string()
            }
            
            RateLimitScope::PerAccount => {
                context.account_id.to_string()
            }
            
            RateLimitScope::PerDevice => {
                format!("{}:{}", context.account_id, context.device_id)
            }
            
            RateLimitScope::PerOperation => {
                format!("{:?}", operation)
            }
            
            RateLimitScope::PerAccountAndOperation => {
                format!("{}:{:?}", context.account_id, operation)
            }
            
            RateLimitScope::PerDeviceAndOperation => {
                format!("{}:{}:{:?}", context.account_id, context.device_id, operation)
            }
        }
    }
}

/// Configuration for rate limiting middleware
#[derive(Debug, Clone)]
pub struct RateLimitingConfig {
    /// Whether rate limiting is enabled
    pub enable_rate_limiting: bool,
    
    /// Default rate limit (requests per second)
    pub default_rate_limit: RateLimit,
    
    /// Rate limiting scope
    pub rate_limit_scope: RateLimitScope,
    
    /// Operation-specific rate limits
    pub operation_limits: HashMap<String, RateLimit>,
    
    /// Whether to cleanup expired limiters
    pub cleanup_expired_limiters: bool,
    
    /// How often to cleanup expired limiters
    pub cleanup_interval: Duration,
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            enable_rate_limiting: true,
            default_rate_limit: RateLimit {
                requests_per_second: 10.0,
                burst_capacity: 20,
            },
            rate_limit_scope: RateLimitScope::PerDevice,
            operation_limits: HashMap::new(),
            cleanup_expired_limiters: true,
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        }
    }
}

/// Rate limiting scope
#[derive(Debug, Clone)]
pub enum RateLimitScope {
    /// Global rate limit across all operations
    Global,
    
    /// Rate limit per account
    PerAccount,
    
    /// Rate limit per device
    PerDevice,
    
    /// Rate limit per operation type
    PerOperation,
    
    /// Rate limit per account and operation type
    PerAccountAndOperation,
    
    /// Rate limit per device and operation type
    PerDeviceAndOperation,
}

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimit {
    /// Requests per second
    pub requests_per_second: f64,
    
    /// Burst capacity (maximum tokens)
    pub burst_capacity: u32,
}

/// Token bucket for rate limiting
#[derive(Debug)]
struct TokenBucket {
    /// Rate limit configuration
    rate_limit: RateLimit,
    
    /// Current token count
    tokens: f64,
    
    /// Last refill time
    last_refill: Instant,
    
    /// Statistics
    total_requests: u64,
    allowed_requests: u64,
    rejected_requests: u64,
}

impl TokenBucket {
    fn new(rate_limit: RateLimit) -> Self {
        Self {
            tokens: rate_limit.burst_capacity as f64,
            rate_limit,
            last_refill: Instant::now(),
            total_requests: 0,
            allowed_requests: 0,
            rejected_requests: 0,
        }
    }
    
    fn try_consume(&mut self) -> bool {
        self.total_requests += 1;
        self.refill();
        
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            self.allowed_requests += 1;
            true
        } else {
            self.rejected_requests += 1;
            false
        }
    }
    
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        
        let tokens_to_add = elapsed * self.rate_limit.requests_per_second;
        self.tokens = (self.tokens + tokens_to_add).min(self.rate_limit.burst_capacity as f64);
        self.last_refill = now;
    }
    
    fn total_requests(&self) -> u64 {
        self.total_requests
    }
    
    fn allowed_requests(&self) -> u64 {
        self.allowed_requests
    }
    
    fn rejected_requests(&self) -> u64 {
        self.rejected_requests
    }
}

/// Rate limiting statistics
#[derive(Debug, Clone)]
pub struct RateLimitingStats {
    /// Number of active rate limiters
    pub active_limiters: usize,
    
    /// Total requests processed
    pub total_requests: u64,
    
    /// Total requests allowed
    pub total_allowed: u64,
    
    /// Total requests rejected
    pub total_rejected: u64,
    
    /// Rejection rate (0.0 to 1.0)
    pub rejection_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::operations::JournalOperation;
    use aura_types::{AccountIdExt, DeviceIdExt};
    use aura_crypto::Effects;
    use std::thread;
    use std::time::Duration;
    
    #[test]
    fn test_rate_limiting_middleware_allows_under_limit() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let config = RateLimitingConfig {
            default_rate_limit: RateLimit {
                requests_per_second: 100.0, // High limit
                burst_capacity: 100,
            },
            ..RateLimitingConfig::default()
        };
        
        let middleware = RateLimitingMiddleware::new(config);
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;
        
        // Should allow several requests under the limit
        for _ in 0..5 {
            let result = middleware.process(operation.clone(), &context, &handler);
            assert!(result.is_ok());
        }
        
        let stats = middleware.stats();
        assert_eq!(stats.total_requests, 5);
        assert_eq!(stats.total_allowed, 5);
        assert_eq!(stats.total_rejected, 0);
    }
    
    #[test]
    fn test_rate_limiting_middleware_blocks_over_limit() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let config = RateLimitingConfig {
            default_rate_limit: RateLimit {
                requests_per_second: 1.0, // Low limit
                burst_capacity: 2, // Small burst
            },
            ..RateLimitingConfig::default()
        };
        
        let middleware = RateLimitingMiddleware::new(config);
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;
        
        // First 2 requests should succeed (burst capacity)
        let result1 = middleware.process(operation.clone(), &context, &handler);
        assert!(result1.is_ok());
        
        let result2 = middleware.process(operation.clone(), &context, &handler);
        assert!(result2.is_ok());
        
        // Third request should be rate limited
        let result3 = middleware.process(operation, &context, &handler);
        assert!(result3.is_err());
        
        let stats = middleware.stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_allowed, 2);
        assert_eq!(stats.total_rejected, 1);
    }
    
    #[test]
    fn test_rate_limiting_disabled() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let config = RateLimitingConfig {
            enable_rate_limiting: false,
            default_rate_limit: RateLimit {
                requests_per_second: 1.0, // Low limit
                burst_capacity: 1,
            },
            ..RateLimitingConfig::default()
        };
        
        let middleware = RateLimitingMiddleware::new(config);
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;
        
        // Should allow many requests when disabled
        for _ in 0..10 {
            let result = middleware.process(operation.clone(), &context, &handler);
            assert!(result.is_ok());
        }
    }
    
    #[test]
    fn test_token_bucket_refill() {
        let rate_limit = RateLimit {
            requests_per_second: 10.0,
            burst_capacity: 10,
        };
        
        let mut bucket = TokenBucket::new(rate_limit);
        
        // Consume all tokens
        for _ in 0..10 {
            assert!(bucket.try_consume());
        }
        
        // Should be out of tokens
        assert!(!bucket.try_consume());
        
        // Wait a bit for refill (this is a simplified test)
        thread::sleep(Duration::from_millis(200)); // 0.2 seconds should add ~2 tokens at 10/sec
        
        // Should have some tokens again
        assert!(bucket.try_consume());
    }
    
    #[test]
    fn test_rate_limit_scopes() {
        let effects = Effects::test(42);
        let account_id1 = aura_types::AccountId::new_with_effects(&effects);
        let account_id2 = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        // Test per-account scope
        let config = RateLimitingConfig {
            rate_limit_scope: RateLimitScope::PerAccount,
            default_rate_limit: RateLimit {
                requests_per_second: 1.0,
                burst_capacity: 1,
            },
            ..RateLimitingConfig::default()
        };
        
        let middleware = RateLimitingMiddleware::new(config);
        let handler = NoOpHandler;
        
        let context1 = JournalContext::new(account_id1, device_id.clone(), "test".to_string());
        let context2 = JournalContext::new(account_id2, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;
        
        // Each account should have its own rate limit
        let result1 = middleware.process(operation.clone(), &context1, &handler);
        assert!(result1.is_ok());
        
        let result2 = middleware.process(operation, &context2, &handler);
        assert!(result2.is_ok()); // Should succeed because it's a different account
    }
}