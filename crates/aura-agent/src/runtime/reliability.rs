//! Reliability patterns for Layer 4 orchestration
//!
//! This module provides coordination patterns that implement reliability concerns
//! across multiple effect handlers. These patterns follow Layer 4 principles:
//!
//! - **Stateful**: Maintain coordination state between operations
//! - **Multi-operation**: Coordinate sequences of effect operations
//! - **Cross-handler**: Orchestrate multiple effect handler types
//!
//! Unlike Layer 3 handlers, these patterns provide coordination logic for
//! managing reliability across distributed operations.

use async_trait::async_trait;
use aura_core::{
    effects::{ReliabilityEffects, ReliabilityError, TimeEffects},
    AuraError,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Circuit breaker states for managing failure patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed - operations flow normally
    Closed,
    /// Circuit is open - operations fail fast
    Open,
    /// Circuit is half-open - testing if service has recovered
    HalfOpen,
}

/// Configuration for circuit breaker behavior
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold before opening circuit
    pub failure_threshold: u32,
    /// Success threshold for closing circuit from half-open
    pub success_threshold: u32,
    /// Timeout before attempting recovery
    pub timeout: Duration,
    /// Window for counting failures
    pub failure_window: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout: Duration::from_secs(30),
            failure_window: Duration::from_secs(60),
        }
    }
}

/// Circuit breaker state tracking
#[derive(Debug)]
struct CircuitBreakerState {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    last_failure: Option<Instant>,
    last_success: Option<Instant>,
    config: CircuitBreakerConfig,
}

impl CircuitBreakerState {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure: None,
            last_success: None,
            config,
        }
    }

    /// Record a successful operation
    fn record_success(&mut self, now: Instant) {
        self.last_success = Some(now);

        match self.state {
            CircuitState::Closed => {
                // Reset failure count on success
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
                // Should not happen - open circuit blocks operations
            }
        }
    }

    /// Record a failed operation
    fn record_failure(&mut self, now: Instant) {
        self.last_failure = Some(now);

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
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

    /// Check if circuit should allow operations
    ///
    /// # Arguments
    /// - `now`: Current time instant (obtain from TimeEffects in production)
    fn should_allow_operation(&mut self, now: Instant) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                // Check if timeout has passed to transition to half-open
                if let Some(last_failure) = self.last_failure {
                    if now.duration_since(last_failure) >= self.config.timeout {
                        self.state = CircuitState::HalfOpen;
                        self.success_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    }
}

/// Retry configuration for orchestration patterns
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Base delay between retries
    pub base_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Jitter percentage (0.0 to 1.0)
    pub jitter: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            jitter: 0.1,
        }
    }
}

/// Reliability coordinator for Layer 4 orchestration
///
/// This coordinator manages reliability patterns across multiple effect handlers.
/// It provides stateful coordination for retry logic, circuit breaking, and
/// failure recovery patterns.
///
/// Following Layer 4 orchestration principles, this coordinator stores effect
/// dependencies (TimeEffects) for multi-effect coordination operations.
pub struct ReliabilityCoordinator {
    /// Circuit breakers by operation key
    circuit_breakers: Arc<Mutex<HashMap<String, CircuitBreakerState>>>,
    /// Default retry configuration
    retry_config: RetryConfig,
    /// Default circuit breaker configuration
    circuit_config: CircuitBreakerConfig,
    /// Time effects for circuit breaker timestamp tracking
    time: Arc<dyn TimeEffects>,
}

impl ReliabilityCoordinator {
    /// Create a new reliability coordinator with explicit TimeEffects dependency.
    ///
    /// # Parameters
    /// - `time`: TimeEffects implementation for circuit breaker state tracking
    ///
    /// This follows Layer 4 orchestration pattern where coordinators store effect
    /// dependencies for stateful multi-effect operations.
    pub fn new(time: Arc<dyn TimeEffects>) -> Self {
        Self {
            circuit_breakers: Arc::new(Mutex::new(HashMap::new())),
            retry_config: RetryConfig::default(),
            circuit_config: CircuitBreakerConfig::default(),
            time,
        }
    }

    /// Create coordinator with custom configurations and TimeEffects dependency.
    ///
    /// # Parameters
    /// - `retry_config`: Configuration for retry behavior
    /// - `circuit_config`: Configuration for circuit breaker behavior
    /// - `time`: TimeEffects implementation for timestamp operations
    pub fn with_config(
        retry_config: RetryConfig,
        circuit_config: CircuitBreakerConfig,
        time: Arc<dyn TimeEffects>,
    ) -> Self {
        Self {
            circuit_breakers: Arc::new(Mutex::new(HashMap::new())),
            retry_config,
            circuit_config,
            time,
        }
    }

    /// Execute operation with circuit breaker protection
    ///
    /// # Arguments
    /// - `operation_key`: Unique identifier for this operation's circuit
    /// - `operation`: The async operation to execute
    /// - `now`: Current time instant (obtain from TimeEffects in production)
    pub async fn execute_with_circuit_breaker<T, F, Fut>(
        &self,
        operation_key: &str,
        operation: F,
        now: Instant,
    ) -> Result<T, AuraError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, AuraError>>,
    {
        // Check circuit breaker
        let should_allow = {
            let mut breakers = self.circuit_breakers.lock().unwrap();
            let breaker = breakers
                .entry(operation_key.to_string())
                .or_insert_with(|| CircuitBreakerState::new(self.circuit_config.clone()));
            breaker.should_allow_operation(now)
        };

        if !should_allow {
            return Err(AuraError::internal(format!(
                "Circuit breaker open for operation: {}",
                operation_key
            )));
        }

        // Execute operation
        match operation().await {
            Ok(result) => {
                // Record success
                let mut breakers = self.circuit_breakers.lock().unwrap();
                if let Some(breaker) = breakers.get_mut(operation_key) {
                    breaker.record_success(now);
                }
                Ok(result)
            }
            Err(error) => {
                // Record failure
                let mut breakers = self.circuit_breakers.lock().unwrap();
                if let Some(breaker) = breakers.get_mut(operation_key) {
                    breaker.record_failure(now);
                }
                Err(error)
            }
        }
    }

    /// Calculate retry delay with exponential backoff and jitter
    fn calculate_retry_delay(&self, attempt: u32) -> Duration {
        let base_delay_ms = self.retry_config.base_delay.as_millis() as f64;
        let delay_ms = base_delay_ms * self.retry_config.backoff_multiplier.powi(attempt as i32);

        // Apply maximum delay cap
        let capped_delay_ms = delay_ms.min(self.retry_config.max_delay.as_millis() as f64);

        // Apply jitter
        let jitter_amount = capped_delay_ms * self.retry_config.jitter;
        let jittered_delay_ms =
            capped_delay_ms + (rand::random::<f64>() - 0.5) * 2.0 * jitter_amount;

        Duration::from_millis(jittered_delay_ms.max(0.0) as u64)
    }

    /// Get circuit breaker state for monitoring
    pub fn get_circuit_state(&self, operation_key: &str) -> Option<CircuitState> {
        let breakers = self.circuit_breakers.lock().unwrap();
        breakers.get(operation_key).map(|b| b.state)
    }

    /// Reset circuit breaker state
    pub fn reset_circuit_breaker(&self, operation_key: &str) {
        let mut breakers = self.circuit_breakers.lock().unwrap();
        breakers.remove(operation_key);
    }
}

// Note: No Default impl - ReliabilityCoordinator requires explicit TimeEffects dependency
// to follow Layer 4 orchestration pattern for stateful multi-effect coordination.

#[async_trait]
impl ReliabilityEffects for ReliabilityCoordinator {
    async fn with_retry<T, F, Fut>(
        &self,
        operation: F,
        max_attempts: u32,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send,
    {
        let mut last_error = None;

        for attempt in 0..max_attempts.max(1) {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);

                    // Don't delay after the last attempt
                    if attempt < max_attempts - 1 {
                        let delay = self.calculate_retry_delay(attempt);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        let final_error =
            last_error.unwrap_or_else(|| AuraError::internal("Retry operation failed"));
        Err(ReliabilityError::RetryExhausted {
            attempts: max_attempts,
            last_error: final_error,
        })
    }

    async fn with_circuit_breaker<T, F, Fut>(
        &self,
        operation: F,
        circuit_id: &str,
        failure_threshold: u32,
        timeout: Duration,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send,
    {
        // Use stored TimeEffects dependency for circuit breaker state tracking
        let now = self.time.now_instant().await;

        self.execute_with_circuit_breaker(circuit_id, || operation(), now)
            .await
            .map_err(|e| ReliabilityError::OperationError(e))
    }

    async fn with_timeout<T, F, Fut>(
        &self,
        operation: F,
        timeout: Duration,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send,
    {
        match tokio::time::timeout(timeout, operation()).await {
            Ok(result) => result.map_err(ReliabilityError::OperationError),
            Err(_) => Err(ReliabilityError::Timeout { timeout }),
        }
    }

    async fn with_rate_limit<T, F, Fut>(
        &self,
        operation: F,
        rate_limit_id: &str,
        max_operations_per_second: f64,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send,
    {
        // Simple rate limiting implementation (placeholder)
        // In a real implementation, this would track operation rates
        operation().await.map_err(ReliabilityError::OperationError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_circuit_breaker_failure_threshold() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();

        let coordinator = ReliabilityCoordinator::with_config(
            RetryConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 2,
                success_threshold: 1,
                timeout: Duration::from_millis(100),
                failure_window: Duration::from_secs(60),
            },
        );

        let call_count = Arc::new(AtomicU32::new(0));

        // First call should succeed
        let result = coordinator
            .execute_with_circuit_breaker("test_op", || {
                call_count.fetch_add(1, Ordering::SeqCst);
                async { Ok::<(), AuraError>(()) }
            }, now)
            .await;
        assert!(result.is_ok());

        // Next two calls should fail and open the circuit
        for _ in 0..2 {
            let result = coordinator
                .execute_with_circuit_breaker("test_op", || {
                    call_count.fetch_add(1, Ordering::SeqCst);
                    async { Err(AuraError::internal("Test failure")) }
                }, now)
                .await;
            assert!(result.is_err());
        }

        // Circuit should now be open - next call should fail fast
        let result = coordinator
            .execute_with_circuit_breaker("test_op", || {
                call_count.fetch_add(1, Ordering::SeqCst);
                async { Ok::<(), AuraError>(()) }
            }, now)
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Circuit breaker open"));

        // Should have made 3 calls (not 4)
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_circuit_breaker_recovery() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();

        let coordinator = ReliabilityCoordinator::with_config(
            RetryConfig::default(),
            CircuitBreakerConfig {
                failure_threshold: 1,
                success_threshold: 1,
                timeout: Duration::from_millis(10),
                failure_window: Duration::from_secs(60),
            },
        );

        // Fail to open circuit
        let result = coordinator
            .execute_with_circuit_breaker("test_op", || async {
                Err(AuraError::internal("Test failure"))
            }, now)
            .await;
        assert!(result.is_err());

        // Circuit should be open
        assert_eq!(
            coordinator.get_circuit_state("test_op"),
            Some(CircuitState::Open)
        );

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(20)).await;

        #[allow(clippy::disallowed_methods)]
        let now_after_sleep = Instant::now();

        // Next call should succeed and close circuit
        let result = coordinator
            .execute_with_circuit_breaker("test_op", || async { Ok(42u32) }, now_after_sleep)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);

        // Circuit should be closed
        assert_eq!(
            coordinator.get_circuit_state("test_op"),
            Some(CircuitState::Closed)
        );
    }

    #[tokio::test]
    async fn test_retry_with_exponential_backoff() {
        let coordinator = ReliabilityCoordinator::with_config(
            RetryConfig {
                max_attempts: 3,
                base_delay: Duration::from_millis(1), // Fast for testing
                max_delay: Duration::from_millis(10),
                backoff_multiplier: 2.0,
                jitter: 0.0, // No jitter for predictable testing
            },
            CircuitBreakerConfig::default(),
        );

        let call_count = Arc::new(AtomicU32::new(0));
        let call_count_clone = call_count.clone();

        // TODO: Get from TimeEffects instead of direct call
        let start = Instant::now();
        let result = coordinator
            .with_retry(
                move || {
                    let count = call_count_clone.fetch_add(1, Ordering::SeqCst) + 1;
                    async move {
                        if count < 3 {
                            Err(AuraError::internal("Retry test"))
                        } else {
                            Ok(count)
                        }
                    }
                },
                3,
                Duration::from_millis(1),
                Duration::from_millis(10),
            )
            .await;

        let duration = start.elapsed();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 3);
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
        // Should have taken some time due to delays
        assert!(duration >= Duration::from_millis(3)); // 1 + 2 ms delays
    }
}
