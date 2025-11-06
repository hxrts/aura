//! Circuit Breaker Middleware

use super::stack::TransportMiddleware;
use super::handler::{TransportHandler, TransportOperation, TransportResult, NetworkAddress};
use aura_protocol::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult, AuraError};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub recovery_timeout_ms: u64,
    pub success_threshold: u32, // Successes needed to close circuit
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_ms: 60000, // 1 minute
            success_threshold: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum CircuitState {
    Closed,      // Normal operation
    Open,        // Failing fast
    HalfOpen,    // Testing if service recovered
}

struct CircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: u64,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: 0,
            config,
        }
    }
    
    fn can_execute(&mut self, current_time: u64) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if recovery timeout has passed
                if current_time.saturating_sub(self.last_failure_time) >= self.config.recovery_timeout_ms {
                    self.state = CircuitState::HalfOpen;
                    self.success_count = 0;
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
    
    fn record_success(&mut self) {
        match self.state {
            CircuitState::Closed => {
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    self.state = CircuitState::Closed;
                    self.failure_count = 0;
                    self.success_count = 0;
                }
            }
            CircuitState::Open => {
                // Should not happen
            }
        }
    }
    
    fn record_failure(&mut self, current_time: u64) {
        self.failure_count += 1;
        self.last_failure_time = current_time;
        
        match self.state {
            CircuitState::Closed => {
                if self.failure_count >= self.config.failure_threshold {
                    self.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                self.state = CircuitState::Open;
                self.success_count = 0;
            }
            CircuitState::Open => {
                // Already open
            }
        }
    }
}

pub struct CircuitBreakerMiddleware {
    breakers: HashMap<NetworkAddress, CircuitBreaker>,
    config: CircuitBreakerConfig,
}

impl CircuitBreakerMiddleware {
    pub fn new() -> Self {
        Self {
            breakers: HashMap::new(),
            config: CircuitBreakerConfig::default(),
        }
    }
    
    pub fn with_config(config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: HashMap::new(),
            config,
        }
    }
}

impl Default for CircuitBreakerMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportMiddleware for CircuitBreakerMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        let current_time = effects.current_timestamp() * 1000; // Convert to milliseconds
        
        // Get the target address for circuit breaker tracking
        let target_address = match &operation {
            TransportOperation::Send { destination, .. } => Some(destination.clone()),
            TransportOperation::Connect { address, .. } => Some(address.clone()),
            TransportOperation::Disconnect { address } => Some(address.clone()),
            _ => None,
        };
        
        if let Some(address) = target_address {
            // Get or create circuit breaker for this address
            let breaker = self.breakers
                .entry(address.clone())
                .or_insert_with(|| CircuitBreaker::new(self.config.clone()));
            
            // Check if we can execute the operation
            if !breaker.can_execute(current_time) {
                return Err(AuraError::Infrastructure(aura_types::InfrastructureError::Transport {
                    message: "Circuit breaker open".to_string(),
                    context: format!("address: {}", address.as_string()),
                }));
            }
            
            // Execute the operation
            match next.execute(operation, effects) {
                Ok(result) => {
                    breaker.record_success();
                    Ok(result)
                }
                Err(error) => {
                    breaker.record_failure(current_time);
                    Err(error)
                }
            }
        } else {
            // No address to track, just execute
            next.execute(operation, effects)
        }
    }
    
    fn middleware_name(&self) -> &'static str {
        "CircuitBreakerMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("failure_threshold".to_string(), self.config.failure_threshold.to_string());
        info.insert("recovery_timeout_ms".to_string(), self.config.recovery_timeout_ms.to_string());
        info.insert("success_threshold".to_string(), self.config.success_threshold.to_string());
        info.insert("tracked_addresses".to_string(), self.breakers.len().to_string());
        
        let open_circuits = self.breakers.values()
            .filter(|b| b.state == CircuitState::Open)
            .count();
        info.insert("open_circuits".to_string(), open_circuits.to_string());
        
        info
    }
}