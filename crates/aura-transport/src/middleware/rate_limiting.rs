//! Rate Limiting Middleware

use super::stack::TransportMiddleware;
use super::handler::{TransportHandler, TransportOperation, TransportResult, NetworkAddress};
use aura_protocol::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult, AuraError};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst_size: u32,
    pub window_size_ms: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 100,
            burst_size: 10,
            window_size_ms: 1000, // 1 second
        }
    }
}

struct RateLimiter {
    tokens: u32,
    last_refill: u64,
    config: RateLimitConfig,
}

impl RateLimiter {
    fn new(config: RateLimitConfig) -> Self {
        Self {
            tokens: config.burst_size,
            last_refill: 0,
            config,
        }
    }
    
    fn try_acquire(&mut self, current_time: u64) -> bool {
        self.refill_tokens(current_time);
        
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
    
    fn refill_tokens(&mut self, current_time: u64) {
        if self.last_refill == 0 {
            self.last_refill = current_time;
            return;
        }
        
        let elapsed_ms = current_time.saturating_sub(self.last_refill);
        if elapsed_ms >= self.config.window_size_ms {
            let windows_passed = elapsed_ms / self.config.window_size_ms;
            let tokens_to_add = (windows_passed * self.config.requests_per_second as u64) as u32;
            
            self.tokens = (self.tokens + tokens_to_add).min(self.config.burst_size);
            self.last_refill = current_time;
        }
    }
}

pub struct RateLimitingMiddleware {
    global_limiter: RateLimiter,
    per_host_limiters: HashMap<NetworkAddress, RateLimiter>,
    config: RateLimitConfig,
}

impl RateLimitingMiddleware {
    pub fn new() -> Self {
        let config = RateLimitConfig::default();
        Self {
            global_limiter: RateLimiter::new(config.clone()),
            per_host_limiters: HashMap::new(),
            config,
        }
    }
    
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            global_limiter: RateLimiter::new(config.clone()),
            per_host_limiters: HashMap::new(),
            config,
        }
    }
}

impl Default for RateLimitingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportMiddleware for RateLimitingMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        let current_time = effects.current_timestamp() * 1000; // Convert to milliseconds
        
        // Check rate limits for operations that count against limits
        match &operation {
            TransportOperation::Send { destination, .. } |
            TransportOperation::Connect { address: destination, .. } => {
                // Check global rate limit
                if !self.global_limiter.try_acquire(current_time) {
                    return Err(AuraError::Infrastructure(aura_types::InfrastructureError::Transport {
                        message: "Rate limit exceeded".to_string(),
                        context: format!("destination: {}", destination.as_string()),
                    }));
                }
                
                // Check per-host rate limit
                let host_limiter = self.per_host_limiters
                    .entry(destination.clone())
                    .or_insert_with(|| RateLimiter::new(self.config.clone()));
                
                if !host_limiter.try_acquire(current_time) {
                    return Err(AuraError::Infrastructure(aura_types::InfrastructureError::Transport {
                        message: "Rate limit exceeded".to_string(),
                        context: format!("destination: {}", destination.as_string()),
                    }));
                }
            }
            _ => {
                // Other operations don't count against rate limits
            }
        }
        
        // Rate limit passed, execute the operation
        next.execute(operation, effects)
    }
    
    fn middleware_name(&self) -> &'static str {
        "RateLimitingMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("requests_per_second".to_string(), self.config.requests_per_second.to_string());
        info.insert("burst_size".to_string(), self.config.burst_size.to_string());
        info.insert("window_size_ms".to_string(), self.config.window_size_ms.to_string());
        info.insert("global_tokens".to_string(), self.global_limiter.tokens.to_string());
        info.insert("tracked_hosts".to_string(), self.per_host_limiters.len().to_string());
        info
    }
}