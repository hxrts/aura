//! Immutable context system for Aura handlers
//!
//! This module defines the immutable `AuraContext` structure that flows through
//! all handler operations without any mutation, ensuring thread-safe access
//! without locks.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::effects::choreographic::ChoreographicRole;
use crate::handlers::{AuraHandlerError, ExecutionMode};
use aura_core::identifiers::DeviceId;
use aura_core::{AccountId, SessionId};

/// Immutable context for choreographic operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographicContext {
    /// Current role in the choreography
    pub current_role: ChoreographicRole,
    /// All participants in the choreography
    pub participants: Arc<Vec<ChoreographicRole>>,
    /// Current epoch for coordination
    pub epoch: u64,
    /// Protocol-specific state (immutable)
    pub protocol_state: Arc<HashMap<String, Vec<u8>>>,
}

impl ChoreographicContext {
    /// Create a new choreographic context
    pub fn new(
        current_role: ChoreographicRole,
        participants: Vec<ChoreographicRole>,
        epoch: u64,
    ) -> Self {
        Self {
            current_role,
            participants: Arc::new(participants),
            epoch,
            protocol_state: Arc::new(HashMap::new()),
        }
    }

    /// Create a new context with updated state
    pub fn with_state<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<Self, AuraHandlerError> {
        let serialized = bincode::serialize(value).map_err(|e| {
            AuraHandlerError::context_error(format!("Failed to serialize state: {}", e))
        })?;

        let mut new_state = (*self.protocol_state).clone();
        new_state.insert(key.to_string(), serialized);

        Ok(Self {
            current_role: self.current_role,
            participants: self.participants.clone(),
            epoch: self.epoch,
            protocol_state: Arc::new(new_state),
        })
    }

    /// Get protocol-specific state
    pub fn get_state<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, AuraHandlerError> {
        match self.protocol_state.get(key) {
            Some(data) => {
                let value = bincode::deserialize(data).map_err(|e| {
                    AuraHandlerError::context_error(format!("Failed to deserialize state: {}", e))
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Check if we are the current deciding role
    pub fn is_decider(&self, decider: &ChoreographicRole) -> bool {
        &self.current_role == decider
    }

    /// Get the number of participants
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }
}

/// Immutable context for simulation operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationContext {
    /// Random seed for deterministic execution
    pub seed: u64,
    /// Current simulation time
    pub simulation_time: Duration,
    /// Whether time is being controlled
    pub time_controlled: bool,
    /// Active fault injection settings
    pub fault_injection: FaultInjectionSettings,
    /// Checkpoint state for time travel
    pub checkpoint_state: Option<Arc<Vec<u8>>>,
    /// Property checking configuration
    pub property_checking: PropertyCheckingConfig,
}

/// Fault injection settings for simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjectionSettings {
    /// Probability of network faults (0.0 to 1.0)
    pub network_fault_rate: f64,
    /// Probability of Byzantine behavior (0.0 to 1.0)
    pub byzantine_fault_rate: f64,
    /// Whether to inject timing faults
    pub timing_faults_enabled: bool,
    /// Maximum delay for timing faults
    pub max_timing_delay: Duration,
}

impl Default for FaultInjectionSettings {
    fn default() -> Self {
        Self {
            network_fault_rate: 0.0,
            byzantine_fault_rate: 0.0,
            timing_faults_enabled: false,
            max_timing_delay: Duration::from_millis(100),
        }
    }
}

/// Property checking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCheckingConfig {
    /// Whether to check safety properties
    pub check_safety: bool,
    /// Whether to check liveness properties
    pub check_liveness: bool,
    /// Maximum execution time before liveness violation
    pub liveness_timeout: Duration,
}

impl Default for PropertyCheckingConfig {
    fn default() -> Self {
        Self {
            check_safety: true,
            check_liveness: true,
            liveness_timeout: Duration::from_secs(30),
        }
    }
}

impl SimulationContext {
    /// Create a new simulation context
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            simulation_time: Duration::ZERO,
            time_controlled: false,
            fault_injection: FaultInjectionSettings::default(),
            checkpoint_state: None,
            property_checking: PropertyCheckingConfig::default(),
        }
    }

    /// Create context with advanced time
    pub fn with_time_advanced(&self, duration: Duration) -> Self {
        Self {
            seed: self.seed,
            simulation_time: self.simulation_time + duration,
            time_controlled: self.time_controlled,
            fault_injection: self.fault_injection.clone(),
            checkpoint_state: self.checkpoint_state.clone(),
            property_checking: self.property_checking.clone(),
        }
    }

    /// Create context with checkpoint
    pub fn with_checkpoint(&self, state: Vec<u8>) -> Self {
        Self {
            seed: self.seed,
            simulation_time: self.simulation_time,
            time_controlled: self.time_controlled,
            fault_injection: self.fault_injection.clone(),
            checkpoint_state: Some(Arc::new(state)),
            property_checking: self.property_checking.clone(),
        }
    }

    /// Create context with time control enabled
    pub fn with_time_control(&self) -> Self {
        Self {
            seed: self.seed,
            simulation_time: self.simulation_time,
            time_controlled: true,
            fault_injection: self.fault_injection.clone(),
            checkpoint_state: self.checkpoint_state.clone(),
            property_checking: self.property_checking.clone(),
        }
    }

    /// Check if a network fault should be injected
    pub fn should_inject_network_fault(&self, rng_value: f64) -> bool {
        rng_value < self.fault_injection.network_fault_rate
    }

    /// Check if Byzantine behavior should be injected
    pub fn should_inject_byzantine_fault(&self, rng_value: f64) -> bool {
        rng_value < self.fault_injection.byzantine_fault_rate
    }
}

/// Immutable context for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    /// Platform information
    pub platform: PlatformInfo,
    /// Authentication state
    pub auth_state: AuthenticationState,
    /// Configuration settings (immutable)
    pub config: Arc<HashMap<String, String>>,
    /// Active sessions (immutable)
    pub sessions: Arc<HashMap<SessionId, SessionMetadata>>,
}

/// Platform information for agent context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    /// Operating system
    pub os: String,
    /// Hardware capabilities
    pub has_secure_enclave: bool,
    /// Available storage backends
    pub storage_backends: Arc<Vec<String>>,
}

impl Default for PlatformInfo {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            has_secure_enclave: false,
            storage_backends: Arc::new(vec!["filesystem".to_string()]),
        }
    }
}

/// Authentication state for agent context
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthenticationState {
    /// Whether the device is authenticated
    pub authenticated: bool,
    /// Biometric authentication available
    pub biometric_available: bool,
    /// Last authentication time (epoch millis)
    pub last_auth_time: Option<u64>,
}

/// Metadata for active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// When the session was created (epoch millis)
    pub created_at: u64,
    /// Session type identifier
    pub session_type: String,
    /// Session-specific data (immutable)
    pub data: Arc<HashMap<String, Vec<u8>>>,
}

impl AgentContext {
    /// Create a new agent context
    pub fn new(_device_id: DeviceId) -> Self {
        Self {
            platform: PlatformInfo::default(),
            auth_state: AuthenticationState::default(),
            config: Arc::new(HashMap::new()),
            sessions: Arc::new(HashMap::new()),
        }
    }

    /// Create context with configuration value
    pub fn with_config(&self, key: &str, value: &str) -> Self {
        let mut new_config = (*self.config).clone();
        new_config.insert(key.to_string(), value.to_string());

        Self {
            platform: self.platform.clone(),
            auth_state: self.auth_state.clone(),
            config: Arc::new(new_config),
            sessions: self.sessions.clone(),
        }
    }

    /// Get a configuration value
    pub fn get_config(&self, key: &str) -> Option<&str> {
        self.config.get(key).map(|s| s.as_str())
    }

    /// Create context with new session
    pub fn with_session(&self, session_id: SessionId, session_type: &str, created_at: u64) -> Self {
        let mut new_sessions = (*self.sessions).clone();
        let metadata = SessionMetadata {
            created_at,
            session_type: session_type.to_string(),
            data: Arc::new(HashMap::new()),
        };
        new_sessions.insert(session_id, metadata);

        Self {
            platform: self.platform.clone(),
            auth_state: self.auth_state.clone(),
            config: self.config.clone(),
            sessions: Arc::new(new_sessions),
        }
    }

    /// Get session metadata
    pub fn get_session(&self, session_id: &SessionId) -> Option<&SessionMetadata> {
        self.sessions.get(session_id)
    }

    /// Create context without session
    pub fn without_session(&self, session_id: &SessionId) -> Self {
        let mut new_sessions = (*self.sessions).clone();
        new_sessions.remove(session_id);

        Self {
            platform: self.platform.clone(),
            auth_state: self.auth_state.clone(),
            config: self.config.clone(),
            sessions: Arc::new(new_sessions),
        }
    }
}

/// Immutable middleware-specific context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareContext {
    /// Tracing information
    pub tracing: TracingContext,
    /// Metrics collection
    pub metrics: MetricsContext,
    /// Retry configuration
    pub retry: RetryContext,
    /// Custom middleware data (immutable)
    pub custom_data: Arc<HashMap<String, Vec<u8>>>,
}

/// Tracing context for observability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TracingContext {
    /// Trace ID for distributed tracing
    pub trace_id: Option<String>,
    /// Span ID for current operation
    pub span_id: Option<String>,
    /// Whether tracing is enabled
    pub enabled: bool,
}

/// Metrics context for monitoring
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricsContext {
    /// Whether metrics collection is enabled
    pub enabled: bool,
    /// Custom metrics labels (immutable)
    pub labels: Arc<HashMap<String, String>>,
}

/// Retry context for resilience
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryContext {
    /// Current retry attempt (0-based)
    pub attempt: u32,
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Base delay between retries
    pub base_delay: Duration,
    /// Whether exponential backoff is enabled
    pub exponential_backoff: bool,
}

impl Default for RetryContext {
    fn default() -> Self {
        Self {
            attempt: 0,
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            exponential_backoff: true,
        }
    }
}

impl MiddlewareContext {
    /// Create a new middleware context
    pub fn new() -> Self {
        Self {
            tracing: TracingContext::default(),
            metrics: MetricsContext {
                enabled: false,
                labels: Arc::new(HashMap::new()),
            },
            retry: RetryContext::default(),
            custom_data: Arc::new(HashMap::new()),
        }
    }

    /// Create context with custom data
    pub fn with_custom_data<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<Self, AuraHandlerError> {
        let serialized = bincode::serialize(value).map_err(|e| {
            AuraHandlerError::context_error(format!("Failed to serialize custom data: {}", e))
        })?;

        let mut new_data = (*self.custom_data).clone();
        new_data.insert(key.to_string(), serialized);

        Ok(Self {
            tracing: self.tracing.clone(),
            metrics: self.metrics.clone(),
            retry: self.retry.clone(),
            custom_data: Arc::new(new_data),
        })
    }

    /// Get custom middleware data
    pub fn get_custom_data<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, AuraHandlerError> {
        match self.custom_data.get(key) {
            Some(data) => {
                let value = bincode::deserialize(data).map_err(|e| {
                    AuraHandlerError::context_error(format!(
                        "Failed to deserialize custom data: {}",
                        e
                    ))
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Create context with tracing enabled
    pub fn with_tracing(&self, trace_id: String, span_id: String) -> Self {
        Self {
            tracing: TracingContext {
                enabled: true,
                trace_id: Some(trace_id),
                span_id: Some(span_id),
            },
            metrics: self.metrics.clone(),
            retry: self.retry.clone(),
            custom_data: self.custom_data.clone(),
        }
    }

    /// Create context with metrics enabled
    pub fn with_metrics(&self) -> Self {
        Self {
            tracing: self.tracing.clone(),
            metrics: MetricsContext {
                enabled: true,
                labels: self.metrics.labels.clone(),
            },
            retry: self.retry.clone(),
            custom_data: self.custom_data.clone(),
        }
    }

    /// Add metrics label
    pub fn with_metrics_label(&self, key: &str, value: &str) -> Self {
        let mut new_labels = (*self.metrics.labels).clone();
        new_labels.insert(key.to_string(), value.to_string());

        Self {
            tracing: self.tracing.clone(),
            metrics: MetricsContext {
                enabled: self.metrics.enabled,
                labels: Arc::new(new_labels),
            },
            retry: self.retry.clone(),
            custom_data: self.custom_data.clone(),
        }
    }
}

impl Default for MiddlewareContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable unified context for all Aura operations
///
/// This context flows through all handler operations without any mutation,
/// ensuring thread-safe access without locks. All modifications return new
/// instances rather than mutating in place.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraContext {
    /// Core device identifier
    pub device_id: DeviceId,
    /// Execution mode (testing, production, simulation)
    pub execution_mode: ExecutionMode,
    /// Current session identifier
    pub session_id: Option<SessionId>,
    /// When this context was created (timestamp in milliseconds)
    pub created_at: u64,
    /// Active account identifier (if known)
    pub account_id: Option<AccountId>,
    /// Arbitrary metadata for higher-level components (immutable)
    pub metadata: Arc<HashMap<String, String>>,
    /// Unique operation identifier for tracing
    pub operation_id: Uuid,
    /// Epoch timestamp bound to the context
    pub epoch: u64,

    // Layer-specific contexts
    /// Choreographic operations context
    pub choreographic: Option<ChoreographicContext>,
    /// Simulation operations context
    pub simulation: Option<SimulationContext>,
    /// Agent operations context
    pub agent: Option<AgentContext>,

    // Cross-cutting context
    /// Middleware operations context
    pub middleware: MiddlewareContext,
}

impl AuraContext {
    /// Create a new context for testing mode
    pub fn for_testing(device_id: DeviceId) -> Self {
        let created_at = 0u64; // Fixed timestamp for deterministic testing
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            session_id: None,
            created_at,
            account_id: None,
            metadata: Arc::new(HashMap::new()),
            operation_id: uuid::Uuid::nil(), // Deterministic UUID for testing
            epoch: created_at,
            choreographic: None,
            simulation: None,
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create a new context for production mode
    pub fn for_production(device_id: DeviceId, created_at: u64, operation_id: Uuid) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Production,
            session_id: None,
            created_at,
            account_id: None,
            metadata: Arc::new(HashMap::new()),
            operation_id,
            epoch: created_at,
            choreographic: None,
            simulation: None,
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create a new context for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        let created_at = seed; // Use seed as deterministic timestamp
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            session_id: None,
            created_at,
            account_id: None,
            metadata: Arc::new(HashMap::new()),
            operation_id: uuid::Uuid::from_u128(seed as u128), // Deterministic UUID from seed
            epoch: created_at,
            choreographic: None,
            simulation: Some(SimulationContext::new(seed)),
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create new context with choreographic context
    pub fn with_choreographic(&self, context: ChoreographicContext) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: Some(context),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with session ID
    pub fn with_session(&self, session_id: SessionId) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: Some(session_id),
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with account identifier
    pub fn with_account(&self, account_id: AccountId) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: Some(account_id),
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with metadata entry
    pub fn with_metadata(&self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let mut new_metadata = (*self.metadata).clone();
        new_metadata.insert(key.into(), value.into());

        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: Arc::new(new_metadata),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with tracing
    pub fn with_tracing(&self, trace_id: String, span_id: String) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.with_tracing(trace_id, span_id),
        }
    }

    /// Create new context with metrics enabled
    pub fn with_metrics(&self) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.with_metrics(),
        }
    }

    /// Create a derived context for a new operation
    pub fn child_operation(&self, operation_id: Uuid) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Check if this is a deterministic execution mode
    pub fn is_deterministic(&self) -> bool {
        self.execution_mode.is_deterministic()
    }

    /// Get the simulation seed if in simulation mode
    pub fn simulation_seed(&self) -> Option<u64> {
        self.execution_mode.seed()
    }

    /// Calculate elapsed time since context creation
    pub fn elapsed_millis(&self, current_time: u64) -> u64 {
        current_time.saturating_sub(self.created_at)
    }
}

impl Default for AuraContext {
    fn default() -> Self {
        Self::for_testing(DeviceId::placeholder())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;

    #[test]
    fn test_immutable_choreographic_context() {
        let role = ChoreographicRole::new(Uuid::nil(), 0);
        let participants = vec![role];
        let ctx = ChoreographicContext::new(role, participants, 1);

        assert_eq!(ctx.current_role, role);
        assert_eq!(ctx.epoch, 1);
        assert_eq!(ctx.participant_count(), 1);

        // Test immutable state management
        let ctx2 = ctx.with_state("test", &42u32).unwrap();

        // Original context unchanged
        assert!(ctx.get_state::<u32>("test").unwrap().is_none());

        // New context has the value
        let value: Option<u32> = ctx2.get_state("test").unwrap();
        assert_eq!(value, Some(42));
    }

    #[test]
    fn test_immutable_simulation_context() {
        let ctx = SimulationContext::new(42);
        assert_eq!(ctx.seed, 42);
        assert_eq!(ctx.simulation_time, Duration::ZERO);
        assert!(!ctx.time_controlled);

        let ctx2 = ctx.with_time_advanced(Duration::from_secs(1));
        assert_eq!(ctx.simulation_time, Duration::ZERO); // Original unchanged
        assert_eq!(ctx2.simulation_time, Duration::from_secs(1)); // New has change

        let ctx3 = ctx2.with_time_control();
        assert!(!ctx2.time_controlled); // Original unchanged
        assert!(ctx3.time_controlled); // New has change
    }

    #[test]
    fn test_immutable_agent_context() {
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([1u8; 16]));
        let ctx = AgentContext::new(device_id);

        // Test immutable configuration
        let ctx2 = ctx.with_config("key", "value");
        assert_eq!(ctx.get_config("key"), None); // Original unchanged
        assert_eq!(ctx2.get_config("key"), Some("value")); // New has value

        // Test immutable sessions
        let session_id = SessionId::new();
        let ctx3 = ctx2.with_session(session_id, "test_session", 1000);

        assert!(ctx2.get_session(&session_id).is_none()); // Original unchanged
        assert!(ctx3.get_session(&session_id).is_some()); // New has session

        let ctx4 = ctx3.without_session(&session_id);
        assert!(ctx3.get_session(&session_id).is_some()); // Original unchanged
        assert!(ctx4.get_session(&session_id).is_none()); // New removed
    }

    #[test]
    fn test_immutable_middleware_context() {
        let ctx = MiddlewareContext::new();

        // Test immutable custom data
        let ctx2 = ctx.with_custom_data("test", &42u32).unwrap();
        assert!(ctx.get_custom_data::<u32>("test").unwrap().is_none()); // Original unchanged
        assert_eq!(ctx2.get_custom_data::<u32>("test").unwrap(), Some(42)); // New has value

        // Test immutable tracing
        let ctx3 = ctx2.with_tracing("trace123".to_string(), "span456".to_string());
        assert!(!ctx2.tracing.enabled); // Original unchanged
        assert!(ctx3.tracing.enabled); // New enabled

        // Test immutable metrics
        let ctx4 = ctx3.with_metrics();
        assert!(!ctx3.metrics.enabled); // Original unchanged
        assert!(ctx4.metrics.enabled); // New enabled

        let ctx5 = ctx4.with_metrics_label("service", "test");
        assert!(ctx4.metrics.labels.is_empty()); // Original unchanged
        assert_eq!(
            ctx5.metrics.labels.get("service"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_immutable_aura_context() {
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([1u8; 16]));

        let ctx = AuraContext::for_testing(device_id);
        assert_eq!(ctx.execution_mode, ExecutionMode::Testing);
        assert!(ctx.is_deterministic());

        // Test immutable metadata
        let ctx2 = ctx.with_metadata("key", "value");
        assert!(ctx.metadata.is_empty()); // Original unchanged
        assert_eq!(ctx2.metadata.get("key"), Some(&"value".to_string()));

        // Test immutable session
        let session_id = SessionId::new();
        let ctx3 = ctx2.with_session(session_id);
        assert!(ctx2.session_id.is_none()); // Original unchanged
        assert_eq!(ctx3.session_id, Some(session_id));

        // Test child operation
        let new_op_id = Uuid::from_bytes([1u8; 16]);
        let ctx4 = ctx3.child_operation(new_op_id);
        assert_ne!(ctx3.operation_id, new_op_id); // Original unchanged
        assert_eq!(ctx4.operation_id, new_op_id);
    }
}
