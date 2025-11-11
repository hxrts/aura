# Error Handling Reference

Quick reference for error handling patterns and types used throughout Aura's distributed system. Error handling follows effect system patterns enabling testable, composable, and context-aware error management. All error types implement standard traits for consistent handling across components.

Error handling integrates with the effect system to provide controllable error injection for testing. Error types follow domain boundaries matching Aura's architectural layers.

See [Effect System Guide](801_effect_system_guide.md) for error integration patterns. See [Protocol Development Guide](803_protocol_development_guide.md) for distributed error handling.

---

## Core Error Types

### Storage Errors

**Purpose**: File system, database, and content storage failures.

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("File not found at path: {path}")]
    FileNotFound { path: String },
    
    #[error("Permission denied for operation: {operation}")]
    PermissionDenied { operation: String },
    
    #[error("Storage quota exceeded: {used}/{limit} bytes")]
    QuotaExceeded { used: u64, limit: u64 },
    
    #[error("Encryption failed: {reason}")]
    EncryptionFailed { reason: String },
    
    #[error("Data corruption detected in: {location}")]
    CorruptedData { location: String },
    
    #[error("Storage backend unavailable")]
    BackendUnavailable,
    
    #[error("IO operation failed: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] bincode::Error),
}

impl StorageError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, 
            StorageError::BackendUnavailable | 
            StorageError::IoError(_)
        )
    }
    
    pub fn is_recoverable(&self) -> bool {
        !matches!(self, 
            StorageError::CorruptedData { .. } |
            StorageError::PermissionDenied { .. }
        )
    }
}
```

### Network Errors

**Purpose**: Communication failures, connectivity issues, and protocol errors.

```rust
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("Connection failed to peer: {peer_id}")]
    ConnectionFailed { peer_id: PeerId },
    
    #[error("Connection timeout after {duration:?}")]
    Timeout { duration: Duration },
    
    #[error("Message too large: {size} bytes, max: {max_size}")]
    MessageTooLarge { size: usize, max_size: usize },
    
    #[error("Network partition detected")]
    NetworkPartition,
    
    #[error("Peer not reachable: {peer_id}")]
    PeerUnreachable { peer_id: PeerId },
    
    #[error("Protocol version mismatch: expected {expected}, got {actual}")]
    ProtocolMismatch { expected: String, actual: String },
    
    #[error("Message authentication failed")]
    AuthenticationFailed,
    
    #[error("Rate limit exceeded for peer: {peer_id}")]
    RateLimitExceeded { peer_id: PeerId },
    
    #[error("Transport error: {0}")]
    TransportError(#[from] std::io::Error),
}

impl NetworkError {
    pub fn is_transient(&self) -> bool {
        matches!(self,
            NetworkError::Timeout { .. } |
            NetworkError::PeerUnreachable { .. } |
            NetworkError::TransportError(_)
        )
    }
    
    pub fn requires_backoff(&self) -> bool {
        matches!(self,
            NetworkError::RateLimitExceeded { .. } |
            NetworkError::ConnectionFailed { .. }
        )
    }
    
    pub fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            NetworkError::RateLimitExceeded { .. } => Some(Duration::from_secs(60)),
            NetworkError::ConnectionFailed { .. } => Some(Duration::from_secs(10)),
            NetworkError::Timeout { .. } => Some(Duration::from_secs(5)),
            _ => None,
        }
    }
}
```

### Crypto Errors

**Purpose**: Cryptographic operation failures, key management issues, and security violations.

```rust
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Invalid signature for data hash: {hash}")]
    InvalidSignature { hash: String },
    
    #[error("Key not found: {key_id}")]
    KeyNotFound { key_id: String },
    
    #[error("Insufficient key shares for threshold operation: {available}/{required}")]
    InsufficientShares { available: usize, required: usize },
    
    #[error("Key derivation failed at path: {path:?}")]
    KeyDerivationFailed { path: Vec<u32> },
    
    #[error("Encryption failed: {algorithm}")]
    EncryptionFailed { algorithm: String },
    
    #[error("Decryption failed: invalid ciphertext or key")]
    DecryptionFailed,
    
    #[error("Random number generation failed")]
    RandomGenerationFailed,
    
    #[error("Key format invalid: {format}")]
    InvalidKeyFormat { format: String },
    
    #[error("Algorithm not supported: {algorithm}")]
    UnsupportedAlgorithm { algorithm: String },
    
    #[error("Hardware security module error: {0}")]
    HsmError(String),
}

impl CryptoError {
    pub fn is_security_critical(&self) -> bool {
        matches!(self,
            CryptoError::InvalidSignature { .. } |
            CryptoError::DecryptionFailed |
            CryptoError::HsmError(_)
        )
    }
    
    pub fn should_log_security_event(&self) -> bool {
        matches!(self,
            CryptoError::InvalidSignature { .. } |
            CryptoError::KeyNotFound { .. } |
            CryptoError::HsmError(_)
        )
    }
}
```

### Journal Errors

**Purpose**: CRDT journal operation failures, consensus issues, and state conflicts.

```rust
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("Fact verification failed: {fact_id}")]
    FactVerificationFailed { fact_id: FactId },
    
    #[error("Intent timeout for operation: {intent_id}")]
    IntentTimeout { intent_id: IntentId },
    
    #[error("Insufficient supporters: {count}/{required} for intent: {intent_id}")]
    InsufficientSupport { count: usize, required: usize, intent_id: IntentId },
    
    #[error("Conflicting operations detected: {operation_a} vs {operation_b}")]
    ConflictingOperations { operation_a: String, operation_b: String },
    
    #[error("Invalid fact signature from device: {device_id}")]
    InvalidFactSignature { device_id: DeviceId },
    
    #[error("Journal state corruption detected")]
    StateCorruption,
    
    #[error("Epoch transition failed: {from_epoch} -> {to_epoch}")]
    EpochTransitionFailed { from_epoch: u64, to_epoch: u64 },
    
    #[error("CRDT merge failed: incompatible states")]
    CrdtMergeFailed,
    
    #[error("Storage backend error: {0}")]
    StorageError(#[from] StorageError),
    
    #[error("Crypto operation failed: {0}")]
    CryptoError(#[from] CryptoError),
}

impl JournalError {
    pub fn affects_consistency(&self) -> bool {
        matches!(self,
            JournalError::StateCorruption |
            JournalError::CrdtMergeFailed |
            JournalError::EpochTransitionFailed { .. }
        )
    }
    
    pub fn requires_coordination(&self) -> bool {
        matches!(self,
            JournalError::ConflictingOperations { .. } |
            JournalError::InsufficientSupport { .. }
        )
    }
}
```

### Authorization Errors

**Purpose**: Capability-based access control failures and permission violations.

```rust
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("Insufficient capability: required {required:?}")]
    InsufficientCapability { required: Capability },
    
    #[error("Missing capabilities: {missing:?}")]
    MissingCapabilities { missing: Vec<Capability> },
    
    #[error("Policy violation: {policy_id} - {violation_type:?}")]
    PolicyViolation { policy_id: String, violation_type: ViolationType },
    
    #[error("Delegation chain invalid: {reason}")]
    InvalidDelegationChain { reason: String },
    
    #[error("Emergency override required for operation")]
    EmergencyOverrideRequired,
    
    #[error("Trust level insufficient: {actual} < {required}")]
    InsufficientTrust { actual: f64, required: f64 },
    
    #[error("Device not authorized: {device_id}")]
    DeviceNotAuthorized { device_id: DeviceId },
    
    #[error("Context constraint violation: {constraint}")]
    ContextViolation { constraint: String },
    
    #[error("Capability expired at: {expiry:?}")]
    CapabilityExpired { expiry: SystemTime },
    
    #[error("Threshold not met: {actual}/{required} approvals")]
    ThresholdNotMet { actual: usize, required: usize },
}

#[derive(Debug, Clone)]
pub enum ViolationType {
    InsufficientCapabilities,
    ContextConstraint,
    TemporalConstraint,
    NetworkLocation,
}

impl AuthorizationError {
    pub fn is_permanent(&self) -> bool {
        matches!(self,
            AuthorizationError::InsufficientCapability { .. } |
            AuthorizationError::DeviceNotAuthorized { .. } |
            AuthorizationError::InvalidDelegationChain { .. }
        )
    }
    
    pub fn can_be_delegated(&self) -> bool {
        matches!(self,
            AuthorizationError::InsufficientCapability { .. } |
            AuthorizationError::MissingCapabilities { .. }
        )
    }
}
```

## Effect System Error Integration

### Error Effects

**Purpose**: Testable error injection and handling through the effect system.

```rust
#[async_trait]
pub trait ErrorEffects: Send + Sync {
    async fn should_inject_error(&self, operation: &str) -> Option<Box<dyn Error + Send>>;
    async fn handle_error(&self, error: &dyn Error, context: &ErrorContext);
    async fn retry_with_backoff(&self, operation: impl Future<Output = Result<(), Box<dyn Error>>>, config: RetryConfig) -> Result<(), Box<dyn Error>>;
}

pub struct ErrorContext {
    pub operation_id: String,
    pub device_id: DeviceId,
    pub timestamp: u64,
    pub component: String,
    pub additional_context: BTreeMap<String, String>,
}

pub struct RetryConfig {
    pub max_attempts: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
}

impl RetryConfig {
    pub fn exponential() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
    
    pub fn linear() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 1.0,
        }
    }
}
```

### Mock Error Handler

**Purpose**: Controlled error injection for testing.

```rust
pub struct MockErrorHandler {
    injected_errors: Arc<RwLock<BTreeMap<String, Box<dyn Error + Send + Sync>>>>,
    error_counts: Arc<RwLock<BTreeMap<String, usize>>>,
    retry_configurations: BTreeMap<String, RetryConfig>,
}

impl MockErrorHandler {
    pub fn new() -> Self {
        Self {
            injected_errors: Arc::new(RwLock::new(BTreeMap::new())),
            error_counts: Arc::new(RwLock::new(BTreeMap::new())),
            retry_configurations: BTreeMap::new(),
        }
    }
    
    pub fn inject_error_for_operation(&self, operation: &str, error: Box<dyn Error + Send + Sync>) {
        self.injected_errors.write().unwrap()
            .insert(operation.to_string(), error);
    }
    
    pub fn inject_network_timeout(&self, peer_id: PeerId) {
        let error = NetworkError::Timeout { 
            duration: Duration::from_secs(30) 
        };
        self.inject_error_for_operation(
            &format!("send_message:{}", peer_id),
            Box::new(error),
        );
    }
    
    pub fn inject_storage_failure(&self, operation: &str) {
        let error = StorageError::BackendUnavailable;
        self.inject_error_for_operation(operation, Box::new(error));
    }
    
    pub fn error_count(&self, operation: &str) -> usize {
        self.error_counts.read().unwrap()
            .get(operation)
            .copied()
            .unwrap_or(0)
    }
}

#[async_trait]
impl ErrorEffects for MockErrorHandler {
    async fn should_inject_error(&self, operation: &str) -> Option<Box<dyn Error + Send>> {
        let mut errors = self.injected_errors.write().unwrap();
        let error = errors.remove(operation);
        
        if error.is_some() {
            let mut counts = self.error_counts.write().unwrap();
            let current_count = counts.get(operation).copied().unwrap_or(0);
            counts.insert(operation.to_string(), current_count + 1);
        }
        
        error.map(|e| e as Box<dyn Error + Send>)
    }
    
    async fn handle_error(&self, error: &dyn Error, context: &ErrorContext) {
        println!("Mock error handler: {} in {} at {}",
            error, context.component, context.timestamp);
    }
    
    async fn retry_with_backoff(
        &self, 
        operation: impl Future<Output = Result<(), Box<dyn Error>>>,
        config: RetryConfig,
    ) -> Result<(), Box<dyn Error>> {
        let mut attempts = 0;
        let mut delay = config.base_delay;
        
        loop {
            match operation.await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    attempts += 1;
                    if attempts >= config.max_attempts {
                        return Err(e);
                    }
                    
                    tokio::time::sleep(delay).await;
                    delay = (delay.as_millis() as f64 * config.backoff_multiplier) as u64;
                    delay = Duration::from_millis(delay).min(config.max_delay);
                }
            }
        }
    }
}
```

## Error Handling Patterns

### Retry with Exponential Backoff

**Purpose**: Resilient operation execution with intelligent retry strategies.

```rust
pub async fn retry_with_exponential_backoff<F, T, E>(
    operation: F,
    max_attempts: usize,
    base_delay: Duration,
    max_delay: Duration,
) -> Result<T, E>
where
    F: Fn() -> futures::future::BoxFuture<'static, Result<T, E>>,
    E: Error + Clone,
{
    let mut attempts = 0;
    let mut delay = base_delay;
    
    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(error) => {
                attempts += 1;
                
                if attempts >= max_attempts {
                    return Err(error);
                }
                
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(max_delay);
            }
        }
    }
}

// Usage example
pub async fn resilient_storage_operation<S: StorageEffects>(
    storage: &S,
    key: &str,
    data: &[u8],
) -> Result<(), StorageError> {
    retry_with_exponential_backoff(
        || Box::pin(storage.store(key, data)),
        3,
        Duration::from_millis(100),
        Duration::from_secs(10),
    ).await
}
```

### Circuit Breaker Pattern

**Purpose**: Prevent cascade failures by monitoring error rates and temporarily disabling failing services.

```rust
pub struct CircuitBreaker {
    failure_threshold: usize,
    recovery_timeout: Duration,
    current_failures: AtomicUsize,
    state: Arc<RwLock<CircuitBreakerState>>,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CircuitBreakerState {
    Closed,  // Normal operation
    Open,    // Failing, reject requests
    HalfOpen, // Testing recovery
}

impl CircuitBreaker {
    pub fn new(failure_threshold: usize, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            recovery_timeout,
            current_failures: AtomicUsize::new(0),
            state: Arc::new(RwLock::new(CircuitBreakerState::Closed)),
            last_failure_time: Arc::new(RwLock::new(None)),
        }
    }
    
    pub async fn execute<F, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: Future<Output = Result<T, E>>,
        E: Error,
    {
        // Check if circuit is open
        if self.is_open() {
            return Err(CircuitBreakerError::CircuitOpen);
        }
        
        // Try to transition to half-open if enough time has passed
        if self.should_attempt_reset() {
            self.transition_to_half_open();
        }
        
        // Execute operation
        match operation.await {
            Ok(result) => {
                self.on_success();
                Ok(result)
            }
            Err(error) => {
                self.on_failure();
                Err(CircuitBreakerError::OperationFailed(error))
            }
        }
    }
    
    fn is_open(&self) -> bool {
        matches!(*self.state.read().unwrap(), CircuitBreakerState::Open)
    }
    
    fn should_attempt_reset(&self) -> bool {
        if let Some(last_failure) = *self.last_failure_time.read().unwrap() {
            Instant::now().duration_since(last_failure) > self.recovery_timeout
        } else {
            false
        }
    }
    
    fn transition_to_half_open(&self) {
        *self.state.write().unwrap() = CircuitBreakerState::HalfOpen;
    }
    
    fn on_success(&self) {
        self.current_failures.store(0, Ordering::Relaxed);
        *self.state.write().unwrap() = CircuitBreakerState::Closed;
    }
    
    fn on_failure(&self) {
        let failures = self.current_failures.fetch_add(1, Ordering::Relaxed) + 1;
        *self.last_failure_time.write().unwrap() = Some(Instant::now());
        
        if failures >= self.failure_threshold {
            *self.state.write().unwrap() = CircuitBreakerState::Open;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError<E: Error> {
    #[error("Circuit breaker is open, operation rejected")]
    CircuitOpen,
    
    #[error("Operation failed: {0}")]
    OperationFailed(E),
}

// Usage example
pub struct ResilientNetworkHandler {
    inner: Arc<dyn NetworkEffects>,
    circuit_breaker: CircuitBreaker,
}

impl ResilientNetworkHandler {
    pub fn new(inner: Arc<dyn NetworkEffects>) -> Self {
        let circuit_breaker = CircuitBreaker::new(
            5, // Fail after 5 consecutive errors
            Duration::from_secs(30), // Try recovery after 30 seconds
        );
        
        Self { inner, circuit_breaker }
    }
}

#[async_trait]
impl NetworkEffects for ResilientNetworkHandler {
    async fn send_message(&self, peer: PeerId, message: Vec<u8>) -> Result<(), NetworkError> {
        self.circuit_breaker.execute(
            self.inner.send_message(peer, message)
        ).await.map_err(|e| match e {
            CircuitBreakerError::CircuitOpen => NetworkError::TransportError(
                std::io::Error::new(std::io::ErrorKind::Other, "Network circuit breaker open")
            ),
            CircuitBreakerError::OperationFailed(network_error) => network_error,
        })
    }
    
    // ... other methods
}
```

### Error Aggregation

**Purpose**: Collect and analyze multiple errors for coordinated error handling.

```rust
pub struct ErrorCollector {
    errors: Vec<CollectedError>,
    error_counts: BTreeMap<String, usize>,
    collection_window: Duration,
    window_start: Instant,
}

pub struct CollectedError {
    pub error: Box<dyn Error + Send + Sync>,
    pub timestamp: Instant,
    pub component: String,
    pub operation: String,
    pub context: BTreeMap<String, String>,
}

impl ErrorCollector {
    pub fn new(window: Duration) -> Self {
        Self {
            errors: Vec::new(),
            error_counts: BTreeMap::new(),
            collection_window: window,
            window_start: Instant::now(),
        }
    }
    
    pub fn add_error(
        &mut self,
        error: Box<dyn Error + Send + Sync>,
        component: String,
        operation: String,
        context: BTreeMap<String, String>,
    ) {
        // Check if we need to reset the window
        if self.window_start.elapsed() > self.collection_window {
            self.reset_window();
        }
        
        let error_key = format!("{}:{}", component, operation);
        *self.error_counts.entry(error_key).or_insert(0) += 1;
        
        self.errors.push(CollectedError {
            error,
            timestamp: Instant::now(),
            component,
            operation,
            context,
        });
    }
    
    pub fn analyze(&self) -> ErrorAnalysis {
        let total_errors = self.errors.len();
        let unique_error_types = self.error_counts.len();
        
        // Find most common error
        let most_common = self.error_counts.iter()
            .max_by_key(|(_, count)| *count)
            .map(|(error_type, count)| (error_type.clone(), *count));
        
        // Identify error patterns
        let patterns = self.identify_patterns();
        
        // Assess severity
        let severity = self.assess_severity();
        
        ErrorAnalysis {
            total_errors,
            unique_error_types,
            most_common,
            patterns,
            severity,
            window_duration: self.collection_window,
        }
    }
    
    fn reset_window(&mut self) {
        self.errors.clear();
        self.error_counts.clear();
        self.window_start = Instant::now();
    }
    
    fn identify_patterns(&self) -> Vec<ErrorPattern> {
        let mut patterns = Vec::new();
        
        // Check for cascade failures
        if self.has_cascade_pattern() {
            patterns.push(ErrorPattern::CascadeFailure);
        }
        
        // Check for network partition pattern
        if self.has_partition_pattern() {
            patterns.push(ErrorPattern::NetworkPartition);
        }
        
        // Check for resource exhaustion pattern
        if self.has_resource_exhaustion_pattern() {
            patterns.push(ErrorPattern::ResourceExhaustion);
        }
        
        patterns
    }
    
    fn assess_severity(&self) -> ErrorSeverity {
        let error_rate = self.errors.len() as f64 / self.collection_window.as_secs() as f64;
        
        if error_rate > 10.0 {
            ErrorSeverity::Critical
        } else if error_rate > 5.0 {
            ErrorSeverity::High
        } else if error_rate > 1.0 {
            ErrorSeverity::Medium
        } else {
            ErrorSeverity::Low
        }
    }
    
    fn has_cascade_pattern(&self) -> bool {
        // Look for rapid succession of failures across multiple components
        let mut component_failures = BTreeMap::new();
        
        for error in &self.errors {
            *component_failures.entry(error.component.clone()).or_insert(0) += 1;
        }
        
        component_failures.len() > 3 && component_failures.values().all(|&count| count > 1)
    }
    
    fn has_partition_pattern(&self) -> bool {
        self.errors.iter()
            .any(|error| error.operation.contains("network") || error.operation.contains("connection"))
    }
    
    fn has_resource_exhaustion_pattern(&self) -> bool {
        self.errors.iter()
            .any(|error| {
                error.operation.contains("quota") || 
                error.operation.contains("memory") ||
                error.operation.contains("disk")
            })
    }
}

pub struct ErrorAnalysis {
    pub total_errors: usize,
    pub unique_error_types: usize,
    pub most_common: Option<(String, usize)>,
    pub patterns: Vec<ErrorPattern>,
    pub severity: ErrorSeverity,
    pub window_duration: Duration,
}

#[derive(Debug, Clone)]
pub enum ErrorPattern {
    CascadeFailure,
    NetworkPartition,
    ResourceExhaustion,
    DependencyFailure,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}
```

## Error Recovery Strategies

### Graceful Degradation

**Purpose**: Maintain partial functionality when components fail.

```rust
pub trait FallbackProvider<T> {
    fn provide_fallback(&self) -> Option<T>;
}

pub struct GracefulService<T, F> {
    primary_service: T,
    fallback_provider: F,
    degraded_mode: AtomicBool,
}

impl<T, F> GracefulService<T, F>
where
    F: FallbackProvider<T>,
{
    pub fn new(primary: T, fallback: F) -> Self {
        Self {
            primary_service: primary,
            fallback_provider: fallback,
            degraded_mode: AtomicBool::new(false),
        }
    }
    
    pub async fn execute_with_fallback<R, E>(
        &self,
        operation: impl Fn(&T) -> futures::future::BoxFuture<'static, Result<R, E>>,
    ) -> Result<R, ServiceError<E>>
    where
        E: Error + 'static,
    {
        // Try primary service first
        match operation(&self.primary_service).await {
            Ok(result) => {
                // If we were in degraded mode, try to recover
                if self.degraded_mode.load(Ordering::Relaxed) {
                    self.attempt_recovery();
                }
                Ok(result)
            }
            Err(error) => {
                // Mark as degraded
                self.degraded_mode.store(true, Ordering::Relaxed);
                
                // Try fallback if available
                if let Some(fallback_service) = self.fallback_provider.provide_fallback() {
                    match operation(&fallback_service).await {
                        Ok(result) => Ok(result),
                        Err(fallback_error) => {
                            Err(ServiceError::BothFailed {
                                primary_error: Box::new(error),
                                fallback_error: Box::new(fallback_error),
                            })
                        }
                    }
                } else {
                    Err(ServiceError::PrimaryFailed {
                        error: Box::new(error),
                        fallback_unavailable: true,
                    })
                }
            }
        }
    }
    
    fn attempt_recovery(&self) {
        // Implementation would test primary service health
        // For now, just clear the degraded flag
        self.degraded_mode.store(false, Ordering::Relaxed);
    }
    
    pub fn is_degraded(&self) -> bool {
        self.degraded_mode.load(Ordering::Relaxed)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError<E: Error + 'static> {
    #[error("Primary service failed: {error}, fallback unavailable")]
    PrimaryFailed {
        error: Box<E>,
        fallback_unavailable: bool,
    },
    
    #[error("Both primary and fallback services failed")]
    BothFailed {
        primary_error: Box<dyn Error + Send + Sync>,
        fallback_error: Box<dyn Error + Send + Sync>,
    },
}

// Usage example with storage fallback
pub struct CachedStorageFallback {
    cache: Arc<RwLock<BTreeMap<String, Vec<u8>>>>,
}

impl FallbackProvider<MockStorageHandler> for CachedStorageFallback {
    fn provide_fallback(&self) -> Option<MockStorageHandler> {
        // Return cache-backed storage handler
        Some(MockStorageHandler::from_cache(self.cache.clone()))
    }
}
```

### Error Context Preservation

**Purpose**: Maintain error context through async call chains.

```rust
pub struct ErrorContext {
    pub operation_id: String,
    pub component: String,
    pub device_id: Option<DeviceId>,
    pub chain: Vec<ErrorLink>,
    pub metadata: BTreeMap<String, String>,
}

pub struct ErrorLink {
    pub component: String,
    pub operation: String,
    pub timestamp: SystemTime,
    pub error_type: String,
}

impl ErrorContext {
    pub fn new(operation_id: impl Into<String>, component: impl Into<String>) -> Self {
        Self {
            operation_id: operation_id.into(),
            component: component.into(),
            device_id: None,
            chain: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }
    
    pub fn with_device(mut self, device_id: DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }
    
    pub fn add_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
    
    pub fn add_link(mut self, component: String, operation: String, error_type: String) -> Self {
        self.chain.push(ErrorLink {
            component,
            operation,
            timestamp: SystemTime::now(),
            error_type,
        });
        self
    }
    
    pub fn trace(&self) -> String {
        let mut trace = format!("Operation: {} in {}", self.operation_id, self.component);
        
        if let Some(device_id) = self.device_id {
            trace.push_str(&format!(" (device: {})", device_id));
        }
        
        for (i, link) in self.chain.iter().enumerate() {
            trace.push_str(&format!(
                "\n  {}: {} -> {} [{}]",
                i + 1,
                link.component,
                link.operation,
                link.error_type
            ));
        }
        
        trace
    }
}

pub trait ContextualError: Error {
    fn with_context(self, context: ErrorContext) -> ContextualErrorWrapper<Self>
    where
        Self: Sized,
    {
        ContextualErrorWrapper::new(self, context)
    }
    
    fn add_context_link(self, component: String, operation: String) -> ContextualErrorWrapper<Self>
    where
        Self: Sized,
    {
        let context = ErrorContext::new("unknown", &component)
            .add_link(component, operation, format!("{}", self));
        
        ContextualErrorWrapper::new(self, context)
    }
}

impl<E: Error> ContextualError for E {}

pub struct ContextualErrorWrapper<E> {
    error: E,
    context: ErrorContext,
}

impl<E> ContextualErrorWrapper<E> {
    pub fn new(error: E, context: ErrorContext) -> Self {
        Self { error, context }
    }
    
    pub fn inner(&self) -> &E {
        &self.error
    }
    
    pub fn context(&self) -> &ErrorContext {
        &self.context
    }
    
    pub fn add_link(mut self, component: String, operation: String, error_type: String) -> Self {
        self.context = self.context.add_link(component, operation, error_type);
        self
    }
}

impl<E: std::fmt::Display> std::fmt::Display for ContextualErrorWrapper<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\n\nError trace:\n{}", self.error, self.context.trace())
    }
}

impl<E: std::fmt::Debug> std::fmt::Debug for ContextualErrorWrapper<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextualErrorWrapper")
            .field("error", &self.error)
            .field("context", &self.context)
            .finish()
    }
}

impl<E: Error> Error for ContextualErrorWrapper<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

// Usage example
pub async fn layered_operation() -> Result<String, ContextualErrorWrapper<StorageError>> {
    let context = ErrorContext::new("layered_operation", "storage_service")
        .with_device(DeviceId::from_seed(123))
        .add_metadata("operation_type", "read")
        .add_metadata("retry_attempt", "1");
    
    let result = storage_operation().await
        .map_err(|e| e.with_context(context))?;
    
    Ok(result)
}

async fn storage_operation() -> Result<String, StorageError> {
    Err(StorageError::FileNotFound { 
        path: "/tmp/missing.txt".to_string() 
    })
}
```

This error handling reference provides comprehensive patterns for robust error management in distributed systems. The patterns integrate with Aura's effect system to enable testable, composable error handling while maintaining clear error context through complex async operations.