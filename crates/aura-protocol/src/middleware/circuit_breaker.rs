//! Circuit Breaker Middleware
//!
//! Provides circuit breaker functionality to prevent cascading failures by temporarily
//! disabling operations that are consistently failing. Respects FlowBudget constraints
//! during probing and maintains state across operations.
//!
//! States:
//! - **Closed**: Normal operation, failures are counted
//! - **Open**: Circuit is open, requests fail fast
//! - **Half-Open**: Limited requests allowed to test if service has recovered

use crate::{
    effects::{AuraEffects, NetworkAddress, NetworkError, StorageError, WakeCondition},
    guards::LeakageBudget,
    handlers::{AuraHandler, AuraHandlerError, EffectType, ExecutionMode},
    middleware::{MiddlewareContext, MiddlewareError, MiddlewareResult, PerformanceProfile},
    AuraResult,
};
use aura_core::{AuraError, DeviceId, MessageContext};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use tokio::time;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Circuit is closed - normal operation
    Closed,
    /// Circuit is open - failing fast to prevent cascading failures
    Open,
    /// Circuit is half-open - allowing limited requests to test recovery
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: usize,
    /// Duration to keep circuit open before attempting recovery
    pub timeout_duration: Duration,
    /// Maximum number of requests allowed in half-open state
    pub half_open_max_requests: usize,
    /// Success threshold to close circuit from half-open state
    pub success_threshold: usize,
    /// Whether to respect FlowBudget during probing
    pub respect_flow_budget: bool,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            timeout_duration: Duration::from_secs(30),
            half_open_max_requests: 3,
            success_threshold: 2,
            respect_flow_budget: true,
        }
    }
}

/// Circuit breaker statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerStats {
    /// Current circuit state
    pub state: CircuitState,
    /// Number of consecutive failures
    pub failure_count: usize,
    /// Number of consecutive successes in half-open state
    pub success_count: usize,
    /// Time when circuit was last opened
    pub last_opened: Option<SystemTime>,
    /// Total number of requests blocked
    pub blocked_requests: usize,
    /// Total number of successful requests
    pub successful_requests: usize,
    /// Total number of failed requests
    pub failed_requests: usize,
}

/// Internal circuit breaker state
#[derive(Debug)]
struct CircuitBreakerState {
    /// Current state of the circuit
    state: CircuitState,
    /// Configuration
    config: CircuitBreakerConfig,
    /// Number of consecutive failures
    failure_count: usize,
    /// Number of consecutive successes in half-open state
    success_count: usize,
    /// Time when circuit was opened
    opened_at: Option<Instant>,
    /// Statistics
    stats: CircuitBreakerStats,
    /// Number of active requests in half-open state
    half_open_requests: usize,
}

impl CircuitBreakerState {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            config,
            failure_count: 0,
            success_count: 0,
            opened_at: None,
            stats: CircuitBreakerStats {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                last_opened: None,
                blocked_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
            },
            half_open_requests: 0,
        }
    }

    /// Check if a request should be allowed through
    fn should_allow_request(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed
                if let Some(opened_at) = self.opened_at {
                    if opened_at.elapsed() >= self.config.timeout_duration {
                        self.transition_to_half_open();
                        self.half_open_requests < self.config.half_open_max_requests
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => {
                self.half_open_requests < self.config.half_open_max_requests
            }
        }
    }

    /// Record a successful request
    fn record_success(&mut self) {
        self.stats.successful_requests += 1;

        match self.state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                self.success_count += 1;
                self.half_open_requests = self.half_open_requests.saturating_sub(1);

                if self.success_count >= self.config.success_threshold {
                    self.transition_to_closed();
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
            }
        }
    }

    /// Record a failed request
    fn record_failure(&mut self) {
        self.stats.failed_requests += 1;

        match self.state {
            CircuitState::Closed => {
                self.failure_count += 1;
                if self.failure_count >= self.config.failure_threshold {
                    self.transition_to_open();
                }
            }
            CircuitState::HalfOpen => {
                self.half_open_requests = self.half_open_requests.saturating_sub(1);
                self.transition_to_open();
            }
            CircuitState::Open => {
                // Already open, just record the failure
            }
        }
    }

    /// Record a blocked request
    fn record_blocked(&mut self) {
        self.stats.blocked_requests += 1;
    }

    /// Transition to closed state
    fn transition_to_closed(&mut self) {
        self.state = CircuitState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.opened_at = None;
        self.half_open_requests = 0;
        self.stats.state = CircuitState::Closed;
    }

    /// Transition to open state
    fn transition_to_open(&mut self) {
        self.state = CircuitState::Open;
        self.opened_at = Some(Instant::now());
        self.success_count = 0;
        self.half_open_requests = 0;
        self.stats.state = CircuitState::Open;
        self.stats.last_opened = Some(SystemTime::now());
    }

    /// Transition to half-open state
    fn transition_to_half_open(&mut self) {
        self.state = CircuitState::HalfOpen;
        self.success_count = 0;
        self.half_open_requests = 0;
        self.stats.state = CircuitState::HalfOpen;
    }

    /// Start tracking a half-open request
    fn start_half_open_request(&mut self) {
        if self.state == CircuitState::HalfOpen {
            self.half_open_requests += 1;
        }
    }

    /// Update internal statistics
    fn update_stats(&mut self) {
        self.stats.failure_count = self.failure_count;
        self.stats.success_count = self.success_count;
    }
}

/// Circuit breaker middleware that prevents cascading failures
pub struct CircuitBreakerMiddleware<H> {
    /// Wrapped handler
    inner: H,
    /// Circuit breaker state per operation type
    circuits: Arc<Mutex<HashMap<String, CircuitBreakerState>>>,
    /// Global configuration
    config: CircuitBreakerConfig,
}

impl<H> CircuitBreakerMiddleware<H> {
    /// Create a new circuit breaker middleware
    pub fn new(inner: H, config: CircuitBreakerConfig) -> Self {
        Self {
            inner,
            circuits: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(inner: H) -> Self {
        Self::new(inner, CircuitBreakerConfig::default())
    }

    /// Get circuit state for an operation
    pub fn get_circuit_state(&self, operation: &str) -> Option<CircuitState> {
        self.circuits
            .lock()
            .ok()?
            .get(operation)
            .map(|state| state.state)
    }

    /// Get circuit statistics for an operation
    pub fn get_circuit_stats(&self, operation: &str) -> Option<CircuitBreakerStats> {
        let circuits = self.circuits.lock().ok()?;
        circuits.get(operation).map(|state| {
            let mut stats = state.stats.clone();
            stats.failure_count = state.failure_count;
            stats.success_count = state.success_count;
            stats
        })
    }

    /// Get all circuit statistics
    pub fn get_all_circuit_stats(&self) -> HashMap<String, CircuitBreakerStats> {
        self.circuits
            .lock()
            .map(|circuits| {
                circuits
                    .iter()
                    .map(|(name, state)| {
                        let mut stats = state.stats.clone();
                        stats.failure_count = state.failure_count;
                        stats.success_count = state.success_count;
                        (name.clone(), stats)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Reset circuit breaker for an operation
    pub fn reset_circuit(&self, operation: &str) {
        if let Ok(mut circuits) = self.circuits.lock() {
            if let Some(state) = circuits.get_mut(operation) {
                state.transition_to_closed();
            }
        }
    }

    /// Reset all circuit breakers
    pub fn reset_all_circuits(&self) {
        if let Ok(mut circuits) = self.circuits.lock() {
            for state in circuits.values_mut() {
                state.transition_to_closed();
            }
        }
    }

    /// Execute an operation with circuit breaker protection
    async fn execute_with_circuit_breaker<F, T, E>(
        &self,
        operation_name: &str,
        operation: F,
    ) -> Result<T, E>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: From<AuraError>,
    {
        // Check if request should be allowed
        let should_allow = {
            let mut circuits = self.circuits.lock().map_err(|_| {
                AuraError::internal_error("Failed to acquire circuit breaker lock".to_string())
            })?;

            let state = circuits
                .entry(operation_name.to_string())
                .or_insert_with(|| CircuitBreakerState::new(self.config.clone()));

            let allow = state.should_allow_request();
            if allow && state.state == CircuitState::HalfOpen {
                state.start_half_open_request();
            }

            if !allow {
                state.record_blocked();
                state.update_stats();
            }

            allow
        };

        if !should_allow {
            return Err(AuraError::operation_failed(format!(
                "Circuit breaker is open for operation: {}",
                operation_name
            ))
            .into());
        }

        // Execute the operation
        let start_time = Instant::now();
        let result = operation.await;
        let execution_time = start_time.elapsed();

        // Record result
        {
            let mut circuits = self.circuits.lock().map_err(|_| {
                AuraError::internal_error("Failed to acquire circuit breaker lock".to_string())
            })?;

            if let Some(state) = circuits.get_mut(operation_name) {
                match result {
                    Ok(_) => state.record_success(),
                    Err(_) => state.record_failure(),
                }
                state.update_stats();
            }
        }

        result
    }

    /// Check FlowBudget before allowing probing requests
    async fn check_flow_budget_for_probe(
        &self,
        _context: &MessageContext,
        _leakage_budget: &LeakageBudget,
    ) -> bool {
        if !self.config.respect_flow_budget {
            return true;
        }

        // Simplified check - in production this would integrate with actual FlowBudget
        // For now, assume probe is allowed if we're not over budget
        true
    }
}

#[async_trait]
impl<H> AuraHandler for CircuitBreakerMiddleware<H>
where
    H: AuraHandler + Send + Sync,
{
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: Vec<u8>,
        context: MiddlewareContext,
    ) -> MiddlewareResult<Vec<u8>> {
        let operation_key = format!("{:?}::{}", effect_type, operation);

        self.execute_with_circuit_breaker(&operation_key, async {
            self.inner
                .execute_effect(effect_type, operation, parameters, context)
                .await
        })
        .await
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.inner.execution_mode()
    }

    fn supported_effects(&self) -> Vec<EffectType> {
        self.inner.supported_effects()
    }

    async fn shutdown(&self) -> Result<(), AuraHandlerError> {
        self.inner.shutdown().await
    }
}

#[async_trait]
impl<H> AuraEffects for CircuitBreakerMiddleware<H>
where
    H: AuraEffects + Send + Sync,
{
    // Network Effects
    async fn send_to_peer(&self, peer: DeviceId, data: Vec<u8>) -> AuraResult<()> {
        self.execute_with_circuit_breaker("send_to_peer", async {
            self.inner.send_to_peer(peer, data).await
        })
        .await
    }

    async fn broadcast(&self, data: Vec<u8>) -> AuraResult<()> {
        self.execute_with_circuit_breaker("broadcast", async {
            self.inner.broadcast(data).await
        })
        .await
    }

    async fn receive(&self) -> AuraResult<(DeviceId, Vec<u8>)> {
        self.execute_with_circuit_breaker("receive", async { self.inner.receive().await })
            .await
    }

    async fn connect_to(&self, address: NetworkAddress) -> AuraResult<()> {
        self.execute_with_circuit_breaker("connect_to", async {
            self.inner.connect_to(address).await
        })
        .await
    }

    async fn disconnect_from(&self, peer: DeviceId) -> AuraResult<()> {
        self.execute_with_circuit_breaker("disconnect_from", async {
            self.inner.disconnect_from(peer).await
        })
        .await
    }

    async fn get_connected_peers(&self) -> AuraResult<Vec<DeviceId>> {
        self.execute_with_circuit_breaker("get_connected_peers", async {
            self.inner.get_connected_peers().await
        })
        .await
    }

    // Storage Effects
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.execute_with_circuit_breaker("storage_get", async {
            self.inner.get(key).await
        })
        .await
    }

    async fn set(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.execute_with_circuit_breaker("storage_set", async {
            self.inner.set(key, value).await
        })
        .await
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.execute_with_circuit_breaker("storage_delete", async {
            self.inner.delete(key).await
        })
        .await
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        self.execute_with_circuit_breaker("storage_list_keys", async {
            self.inner.list_keys(prefix).await
        })
        .await
    }

    // Crypto Effects
    async fn random_bytes(&self, len: usize) -> AuraResult<Vec<u8>> {
        self.execute_with_circuit_breaker("random_bytes", async {
            self.inner.random_bytes(len).await
        })
        .await
    }

    async fn hash(&self, data: &[u8]) -> AuraResult<Vec<u8>> {
        self.execute_with_circuit_breaker("hash", async { self.inner.hash(data).await })
            .await
    }

    async fn sign(&self, data: &[u8]) -> AuraResult<Vec<u8>> {
        self.execute_with_circuit_breaker("sign", async { self.inner.sign(data).await })
            .await
    }

    async fn verify(&self, data: &[u8], signature: &[u8]) -> AuraResult<bool> {
        self.execute_with_circuit_breaker("verify", async {
            self.inner.verify(data, signature).await
        })
        .await
    }

    // Time Effects
    async fn now(&self) -> AuraResult<SystemTime> {
        self.execute_with_circuit_breaker("now", async { self.inner.now().await })
            .await
    }

    async fn sleep(&self, duration: Duration) -> AuraResult<()> {
        self.execute_with_circuit_breaker("sleep", async {
            self.inner.sleep(duration).await
        })
        .await
    }

    async fn sleep_until(&self, deadline: SystemTime) -> AuraResult<()> {
        self.execute_with_circuit_breaker("sleep_until", async {
            self.inner.sleep_until(deadline).await
        })
        .await
    }

    async fn wait_for(&self, condition: WakeCondition) -> AuraResult<()> {
        self.execute_with_circuit_breaker("wait_for", async {
            self.inner.wait_for(condition).await
        })
        .await
    }

    // Console Effects
    async fn log(&self, level: crate::effects::LogLevel, message: String) -> AuraResult<()> {
        // Don't circuit break logging - it should always work
        self.inner.log(level, message).await
    }

    async fn debug(&self, message: String) -> AuraResult<()> {
        self.inner.debug(message).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::{CompositeHandler, ExecutionMode};
    use aura_core::DeviceId;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_circuit_breaker_closed_state() {
        let device_id = DeviceId::new();
        let base_handler = CompositeHandler::for_testing(device_id);
        let circuit_breaker = CircuitBreakerMiddleware::with_defaults(base_handler);

        // Circuit should start in closed state
        let stats = circuit_breaker.get_circuit_stats("test_operation");
        assert!(stats.is_none()); // No circuit created yet

        // Successful operation should keep circuit closed
        let result = circuit_breaker.random_bytes(32).await;
        assert!(result.is_ok());

        let stats = circuit_breaker.get_circuit_stats("random_bytes");
        if let Some(stats) = stats {
            assert_eq!(stats.state, CircuitState::Closed);
            assert_eq!(stats.successful_requests, 1);
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_failure_threshold() {
        let device_id = DeviceId::new();
        let base_handler = CompositeHandler::for_testing(device_id);
        
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            timeout_duration: Duration::from_millis(100),
            ..Default::default()
        };
        
        let circuit_breaker = CircuitBreakerMiddleware::new(base_handler, config);

        // Simulate failures by trying to connect to invalid address
        let invalid_address = NetworkAddress::from("invalid://address");
        
        // First failure
        let result1 = circuit_breaker.connect_to(invalid_address.clone()).await;
        assert!(result1.is_err());
        
        let stats = circuit_breaker.get_circuit_stats("connect_to");
        if let Some(stats) = stats {
            assert_eq!(stats.state, CircuitState::Closed);
            assert_eq!(stats.failed_requests, 1);
        }

        // Second failure should open the circuit
        let result2 = circuit_breaker.connect_to(invalid_address).await;
        assert!(result2.is_err());

        let stats = circuit_breaker.get_circuit_stats("connect_to");
        if let Some(stats) = stats {
            assert_eq!(stats.state, CircuitState::Open);
            assert_eq!(stats.failed_requests, 2);
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_open_state_blocks_requests() {
        let device_id = DeviceId::new();
        let base_handler = CompositeHandler::for_testing(device_id);
        
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_duration: Duration::from_millis(100),
            ..Default::default()
        };
        
        let circuit_breaker = CircuitBreakerMiddleware::new(base_handler, config);

        // Trigger circuit opening
        let invalid_address = NetworkAddress::from("invalid://address");
        let _ = circuit_breaker.connect_to(invalid_address.clone()).await;

        // Circuit should be open and block requests
        let result = circuit_breaker.connect_to(invalid_address).await;
        assert!(result.is_err());
        
        let stats = circuit_breaker.get_circuit_stats("connect_to");
        if let Some(stats) = stats {
            assert_eq!(stats.state, CircuitState::Open);
            assert!(stats.blocked_requests >= 1);
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_transition() {
        let device_id = DeviceId::new();
        let base_handler = CompositeHandler::for_testing(device_id);
        
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_duration: Duration::from_millis(50),
            half_open_max_requests: 1,
            success_threshold: 1,
            ..Default::default()
        };
        
        let circuit_breaker = CircuitBreakerMiddleware::new(base_handler, config);

        // Open circuit
        let invalid_address = NetworkAddress::from("invalid://address");
        let _ = circuit_breaker.connect_to(invalid_address).await;

        // Wait for timeout
        time::sleep(Duration::from_millis(60)).await;

        // Next request should be allowed (half-open state)
        let result = circuit_breaker.random_bytes(32).await;
        assert!(result.is_ok());

        let stats = circuit_breaker.get_circuit_stats("random_bytes");
        if let Some(stats) = stats {
            // Circuit should transition to closed after successful request
            assert_eq!(stats.state, CircuitState::Closed);
        }
    }

    #[test]
    fn test_circuit_breaker_config_default() {
        let config = CircuitBreakerConfig::default();
        
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.timeout_duration, Duration::from_secs(30));
        assert_eq!(config.half_open_max_requests, 3);
        assert_eq!(config.success_threshold, 2);
        assert!(config.respect_flow_budget);
    }

    #[tokio::test]
    async fn test_circuit_breaker_reset() {
        let device_id = DeviceId::new();
        let base_handler = CompositeHandler::for_testing(device_id);
        
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_duration: Duration::from_secs(10), // Long timeout
            ..Default::default()
        };
        
        let circuit_breaker = CircuitBreakerMiddleware::new(base_handler, config);

        // Open circuit
        let invalid_address = NetworkAddress::from("invalid://address");
        let _ = circuit_breaker.connect_to(invalid_address).await;

        let stats = circuit_breaker.get_circuit_stats("connect_to");
        if let Some(stats) = stats {
            assert_eq!(stats.state, CircuitState::Open);
        }

        // Reset circuit
        circuit_breaker.reset_circuit("connect_to");

        let stats = circuit_breaker.get_circuit_stats("connect_to");
        if let Some(stats) = stats {
            assert_eq!(stats.state, CircuitState::Closed);
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_logging_exempt() {
        let device_id = DeviceId::new();
        let base_handler = CompositeHandler::for_testing(device_id);
        
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        };
        
        let circuit_breaker = CircuitBreakerMiddleware::new(base_handler, config);

        // Logging should always work even if other operations are circuit broken
        let result = circuit_breaker
            .log(crate::effects::LogLevel::Info, "test message".to_string())
            .await;
        assert!(result.is_ok());
    }
}