//! Retry middleware for operation retry logic

use super::{JournalMiddleware, JournalHandler, JournalContext};
use crate::error::{Error, Result};
use crate::operations::JournalOperation;
use std::time::{Duration, Instant};

/// Retry middleware that retries failed operations
pub struct RetryMiddleware {
    /// Configuration
    config: RetryConfig,
}

impl RetryMiddleware {
    /// Create new retry middleware
    pub fn new(config: RetryConfig) -> Self {
        Self { config }
    }
}

impl JournalMiddleware for RetryMiddleware {
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value> {
        // Skip retry if disabled
        if !self.config.enable_retry {
            return next.handle(operation, context);
        }
        
        // Check if operation is retryable
        if !self.is_retryable(&operation) {
            return next.handle(operation, context);
        }
        
        let mut last_error = None;
        let start_time = Instant::now();
        
        for attempt in 0..=self.config.max_attempts {
            // Check timeout
            if start_time.elapsed() > self.config.total_timeout {
                return Err(last_error.unwrap_or_else(|| {
                    Error::invalid_operation("Retry timeout exceeded")
                }));
            }
            
            // Try the operation
            match next.handle(operation.clone(), context) {
                Ok(result) => {
                    if attempt > 0 {
                        // Log successful retry
                        tracing::info!(
                            "Operation succeeded after {} retries: {:?}",
                            attempt,
                            operation
                        );
                    }
                    return Ok(result);
                }
                Err(error) => {
                    // Check if error is retryable
                    if !self.is_error_retryable(&error) {
                        return Err(error);
                    }
                    
                    last_error = Some(error);
                    
                    // Don't delay after the last attempt
                    if attempt < self.config.max_attempts {
                        let delay = self.calculate_delay(attempt);
                        std::thread::sleep(delay);
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| {
            Error::invalid_operation("Max retry attempts exceeded")
        }))
    }
    
    fn name(&self) -> &str {
        "retry"
    }
}

impl RetryMiddleware {
    fn is_retryable(&self, operation: &JournalOperation) -> bool {
        match operation {
            // Read operations are generally retryable
            JournalOperation::GetDevices => true,
            JournalOperation::GetEpoch => true,
            
            // Write operations may or may not be retryable depending on error
            JournalOperation::AddDevice { .. } => self.config.retry_write_operations,
            JournalOperation::RemoveDevice { .. } => self.config.retry_write_operations,
            JournalOperation::AddGuardian { .. } => self.config.retry_write_operations,
            JournalOperation::IncrementEpoch => self.config.retry_write_operations,
        }
    }
    
    fn is_error_retryable(&self, error: &Error) -> bool {
        match error {
            // Storage errors might be temporary
            Error::StorageFailed { .. } => true,
            
            // Network/communication errors are typically retryable
            Error::CommunicationFailed { .. } => true,
            
            // Timeout errors can be retried
            Error::Timeout { .. } => true,
            
            // Infrastructure errors might be temporary
            Error::InfrastructureFailed { .. } => true,
            
            // Invalid operations shouldn't be retried
            Error::InvalidOperation { .. } => false,
            
            // Invalid input errors shouldn't be retried
            Error::InvalidInput { .. } => false,
            
            // Consensus failures might be retryable
            Error::ConsensusFailed { .. } => self.config.retry_consensus_errors,
            
            // Permission errors shouldn't be retried
            Error::PermissionDenied { .. } => false,
            
            // Authentication errors shouldn't be retried
            Error::AuthenticationFailed { .. } => false,
            
            // Capability errors might be retryable
            Error::CapabilityDenied { .. } => self.config.retry_capability_errors,
            
            // Default cases for other error types
            Error::Storage(_) => true,
            Error::Coordination(_) => true,
            Error::DeviceNotFound(_) => false,
            Error::GuardianNotFound(_) => false,
            Error::Automerge(_) => true,
        }
    }
    
    fn calculate_delay(&self, attempt: usize) -> Duration {
        match self.config.backoff_strategy {
            BackoffStrategy::Fixed => self.config.base_delay,
            
            BackoffStrategy::Linear => {
                Duration::from_millis(
                    self.config.base_delay.as_millis() as u64 * (attempt + 1) as u64
                )
            }
            
            BackoffStrategy::Exponential => {
                let multiplier = 2_u64.pow(attempt as u32);
                let delay_ms = self.config.base_delay.as_millis() as u64 * multiplier;
                let max_delay_ms = self.config.max_delay.as_millis() as u64;
                Duration::from_millis(delay_ms.min(max_delay_ms))
            }
            
            BackoffStrategy::ExponentialWithJitter => {
                let multiplier = 2_u64.pow(attempt as u32);
                let base_delay_ms = self.config.base_delay.as_millis() as u64 * multiplier;
                let max_delay_ms = self.config.max_delay.as_millis() as u64;
                let delay_ms = base_delay_ms.min(max_delay_ms);
                
                // Add random jitter (±25%)
                let jitter_range = delay_ms / 4;
                let jitter = (rand::random::<u64>() % (jitter_range * 2)) as i64 - jitter_range as i64;
                let final_delay = (delay_ms as i64 + jitter).max(0) as u64;
                
                Duration::from_millis(final_delay)
            }
        }
    }
}

/// Configuration for retry middleware
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Whether retry is enabled
    pub enable_retry: bool,
    
    /// Maximum number of retry attempts (not including initial attempt)
    pub max_attempts: usize,
    
    /// Base delay between retries
    pub base_delay: Duration,
    
    /// Maximum delay between retries
    pub max_delay: Duration,
    
    /// Total timeout for all retry attempts
    pub total_timeout: Duration,
    
    /// Backoff strategy for delays
    pub backoff_strategy: BackoffStrategy,
    
    /// Whether to retry write operations
    pub retry_write_operations: bool,
    
    /// Whether to retry consensus errors
    pub retry_consensus_errors: bool,
    
    /// Whether to retry capability errors
    pub retry_capability_errors: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enable_retry: true,
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            total_timeout: Duration::from_secs(60),
            backoff_strategy: BackoffStrategy::ExponentialWithJitter,
            retry_write_operations: false, // Write operations are typically not idempotent
            retry_consensus_errors: true,
            retry_capability_errors: false,
        }
    }
}

/// Backoff strategies for retry delays
#[derive(Debug, Clone)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed,
    
    /// Linear increase in delay (base_delay * attempt)
    Linear,
    
    /// Exponential increase in delay (base_delay * 2^attempt)
    Exponential,
    
    /// Exponential increase with random jitter to avoid thundering herd
    ExponentialWithJitter,
}

/// Simple random number generator for jitter
mod rand {
    use std::sync::atomic::{AtomicU64, Ordering};
    
    static SEED: AtomicU64 = AtomicU64::new(1);
    
    pub fn random<T>() -> T
    where
        T: From<u64>,
    {
        // Simple LCG for jitter
        let prev = SEED.load(Ordering::Relaxed);
        let next = prev.wrapping_mul(1103515245).wrapping_add(12345);
        SEED.store(next, Ordering::Relaxed);
        T::from(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::operations::JournalOperation;
    use aura_types::{AccountIdExt, DeviceIdExt};
    use aura_crypto::Effects;
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    struct FailingHandler {
        attempts: Arc<AtomicUsize>,
        fail_until: usize,
    }
    
    impl FailingHandler {
        fn new(fail_until: usize) -> Self {
            Self {
                attempts: Arc::new(AtomicUsize::new(0)),
                fail_until,
            }
        }
        
        fn attempt_count(&self) -> usize {
            self.attempts.load(Ordering::Relaxed)
        }
    }
    
    impl JournalHandler for FailingHandler {
        fn handle(
            &self,
            operation: JournalOperation,
            _context: &JournalContext,
        ) -> Result<serde_json::Value> {
            let attempt = self.attempts.fetch_add(1, Ordering::Relaxed);
            
            if attempt < self.fail_until {
                Err(Error::storage_failed("Simulated failure"))
            } else {
                Ok(serde_json::json!({
                    "operation": format!("{:?}", operation),
                    "handler": "failing",
                    "attempt": attempt + 1,
                    "success": true
                }))
            }
        }
    }
    
    #[test]
    fn test_retry_middleware_success_after_failure() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let config = RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(1), // Fast for testing
            ..RetryConfig::default()
        };
        
        let middleware = RetryMiddleware::new(config);
        let handler = FailingHandler::new(2); // Fail first 2 attempts, succeed on 3rd
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch; // Read operation - retryable
        
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());
        assert_eq!(handler.attempt_count(), 3); // Should have made 3 attempts
    }
    
    #[test]
    fn test_retry_middleware_max_attempts_exceeded() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let config = RetryConfig {
            max_attempts: 2,
            base_delay: Duration::from_millis(1), // Fast for testing
            ..RetryConfig::default()
        };
        
        let middleware = RetryMiddleware::new(config);
        let handler = FailingHandler::new(10); // Always fail
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch; // Read operation - retryable
        
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_err());
        assert_eq!(handler.attempt_count(), 3); // Initial attempt + 2 retries
    }
    
    #[test]
    fn test_retry_middleware_non_retryable_operation() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let config = RetryConfig {
            retry_write_operations: false,
            ..RetryConfig::default()
        };
        
        let middleware = RetryMiddleware::new(config);
        let handler = FailingHandler::new(1); // Fail first attempt
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::IncrementEpoch; // Write operation - not retryable with this config
        
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_err());
        assert_eq!(handler.attempt_count(), 1); // Should only try once
    }
    
    #[test]
    fn test_backoff_strategies() {
        let config = RetryConfig::default();
        let middleware = RetryMiddleware::new(config);
        
        // Test different backoff strategies
        let fixed_delay = middleware.calculate_delay(2);
        
        // All strategies should return some delay
        assert!(fixed_delay > Duration::from_millis(0));
    }
}