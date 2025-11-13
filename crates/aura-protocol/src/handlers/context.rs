//! Unified context system for Aura handlers
//!
//! This module defines the `AuraContext` structure that flows through all
//! handler operations, carrying state and configuration across all layers
//! of the system.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use super::{AuraHandlerError, ExecutionMode};
use crate::{effects::choreographic::ChoreographicRole, guards::flow::FlowHint};
use aura_core::{AccountId, DeviceId, SessionId};

/// Context for choreographic operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographicContext {
    /// Current role in the choreography
    pub current_role: ChoreographicRole,
    /// All participants in the choreography
    pub participants: Vec<ChoreographicRole>,
    /// Current epoch for coordination
    pub epoch: u64,
    /// Protocol-specific state
    pub protocol_state: HashMap<String, Vec<u8>>,
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
            participants,
            epoch,
            protocol_state: HashMap::new(),
        }
    }

    /// Set protocol-specific state
    pub fn set_state<T: serde::Serialize>(
        &mut self,
        key: &str,
        value: &T,
    ) -> Result<(), AuraHandlerError> {
        let serialized = bincode::serialize(value).map_err(|e| {
            AuraHandlerError::context_error(format!("Failed to serialize state: {}", e))
        })?;
        self.protocol_state.insert(key.to_string(), serialized);
        Ok(())
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

/// Context for simulation operations
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
    pub checkpoint_state: Option<Vec<u8>>,
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

    /// Advance simulation time
    pub fn advance_time(&mut self, duration: Duration) {
        self.simulation_time += duration;
    }

    /// Set a checkpoint for time travel
    pub fn set_checkpoint(&mut self, state: Vec<u8>) {
        self.checkpoint_state = Some(state);
    }

    /// Restore from checkpoint
    pub fn restore_checkpoint(&mut self) -> Option<Vec<u8>> {
        self.checkpoint_state.clone()
    }

    /// Enable time control
    pub fn enable_time_control(&mut self) {
        self.time_controlled = true;
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

/// Context for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    /// Platform information
    pub platform: PlatformInfo,
    /// Authentication state
    pub auth_state: AuthenticationState,
    /// Configuration settings
    pub config: HashMap<String, String>,
    /// Active sessions
    pub sessions: HashMap<SessionId, SessionMetadata>,
}

/// Platform information for agent context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    /// Operating system
    pub os: String,
    /// Hardware capabilities
    pub has_secure_enclave: bool,
    /// Available storage backends
    pub storage_backends: Vec<String>,
}

impl Default for PlatformInfo {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            has_secure_enclave: false, // Conservative default
            storage_backends: vec!["filesystem".to_string()],
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
    /// Last authentication time
    pub last_auth_time: Option<std::time::SystemTime>,
}

/// Metadata for active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// When the session was created
    pub created_at: std::time::SystemTime,
    /// Session type identifier
    pub session_type: String,
    /// Session-specific data
    pub data: HashMap<String, Vec<u8>>,
}

impl AgentContext {
    /// Create a new agent context
    pub fn new(_device_id: DeviceId) -> Self {
        Self {
            platform: PlatformInfo::default(),
            auth_state: AuthenticationState::default(),
            config: HashMap::new(),
            sessions: HashMap::new(),
        }
    }

    /// Set a configuration value
    pub fn set_config(&mut self, key: &str, value: &str) {
        self.config.insert(key.to_string(), value.to_string());
    }

    /// Get a configuration value
    pub fn get_config(&self, key: &str) -> Option<&str> {
        self.config.get(key).map(|s| s.as_str())
    }

    /// Create a new session
    /// NOTE: In production, should use TimeEffects for timestamp
    pub fn create_session(&mut self, session_type: &str) -> SessionId {
        let session_id = SessionId::new();
        // TODO: Replace with TimeEffects::current_timestamp()
        let metadata = SessionMetadata {
            created_at: std::time::SystemTime::UNIX_EPOCH, // Deterministic timestamp
            session_type: session_type.to_string(),
            data: HashMap::new(),
        };
        self.sessions.insert(session_id, metadata);
        session_id
    }

    /// Get session metadata
    pub fn get_session(&self, session_id: &SessionId) -> Option<&SessionMetadata> {
        self.sessions.get(session_id)
    }

    /// Remove a session
    pub fn remove_session(&mut self, session_id: &SessionId) -> Option<SessionMetadata> {
        self.sessions.remove(session_id)
    }
}

/// Middleware-specific context for cross-cutting concerns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareContext {
    /// Tracing information
    pub tracing: TracingContext,
    /// Metrics collection
    pub metrics: MetricsContext,
    /// Retry configuration
    pub retry: RetryContext,
    /// Custom middleware data
    pub custom_data: HashMap<String, Vec<u8>>,
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
    /// Custom metrics labels
    pub labels: HashMap<String, String>,
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
            metrics: MetricsContext::default(),
            retry: RetryContext::default(),
            custom_data: HashMap::new(),
        }
    }

    /// Set custom middleware data
    pub fn set_custom_data<T: serde::Serialize>(
        &mut self,
        key: &str,
        value: &T,
    ) -> Result<(), AuraHandlerError> {
        let serialized = bincode::serialize(value).map_err(|e| {
            AuraHandlerError::context_error(format!("Failed to serialize custom data: {}", e))
        })?;
        self.custom_data.insert(key.to_string(), serialized);
        Ok(())
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

    /// Enable tracing with IDs
    pub fn enable_tracing(&mut self, trace_id: String, span_id: String) {
        self.tracing.enabled = true;
        self.tracing.trace_id = Some(trace_id);
        self.tracing.span_id = Some(span_id);
    }

    /// Enable metrics collection
    pub fn enable_metrics(&mut self) {
        self.metrics.enabled = true;
    }

    /// Add metrics label
    pub fn add_metrics_label(&mut self, key: &str, value: &str) {
        self.metrics
            .labels
            .insert(key.to_string(), value.to_string());
    }
}

impl Default for MiddlewareContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified context for all Aura operations
///
/// This context flows through all handler operations, carrying state and
/// configuration across all layers of the system. It provides a consistent
/// interface for accessing layer-specific context while maintaining clean
/// separation of concerns.
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
    /// Arbitrary metadata for higher-level components
    pub metadata: HashMap<String, String>,
    /// Unique operation identifier for tracing
    pub operation_id: Uuid,
    /// Epoch timestamp bound to the context
    pub epoch: u64,
    /// Pending flow hint (not serialized)
    #[serde(skip)]
    pub flow_hint: Option<FlowHint>,

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
        // Use deterministic values for testing
        let created_at = 0u64; // Fixed timestamp for deterministic testing
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            session_id: None,
            created_at,
            account_id: None,
            metadata: HashMap::new(),
            operation_id: uuid::Uuid::nil(), // Deterministic UUID for testing
            epoch: created_at,
            flow_hint: None,
            choreographic: None,
            simulation: None,
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create a new context for production mode
    /// NOTE: In production, this should use TimeEffects for current timestamp
    /// and RandomEffects for operation_id generation
    pub fn for_production(device_id: DeviceId) -> Self {
        // TODO: Replace with TimeEffects::current_timestamp_millis()
        let created_at = 0u64; // Placeholder - should use TimeEffects
        Self {
            device_id,
            execution_mode: ExecutionMode::Production,
            session_id: None,
            created_at,
            account_id: None,
            metadata: HashMap::new(),
            operation_id: uuid::Uuid::nil(), // TODO: Replace with RandomEffects
            epoch: created_at,
            flow_hint: None,
            choreographic: None,
            simulation: None,
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create a new context for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        // Use deterministic timestamp for simulation reproducibility
        let created_at = seed; // Use seed as deterministic timestamp
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            session_id: None,
            created_at,
            account_id: None,
            metadata: HashMap::new(),
            operation_id: uuid::Uuid::from_u128(seed as u128), // Deterministic UUID from seed
            epoch: created_at,
            flow_hint: None,
            choreographic: None,
            simulation: Some(SimulationContext::new(seed)),
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Set choreographic context
    pub fn with_choreographic(mut self, context: ChoreographicContext) -> Self {
        self.choreographic = Some(context);
        self
    }

    /// Set session ID
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self.flow_hint = None;
        self
    }

    /// Attach account identifier to the context.
    pub fn with_account(mut self, account_id: AccountId) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Add metadata entry.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Enable tracing
    pub fn with_tracing(mut self, trace_id: String, span_id: String) -> Self {
        self.middleware.enable_tracing(trace_id, span_id);
        self
    }

    /// Enable metrics
    pub fn with_metrics(mut self) -> Self {
        self.middleware.enable_metrics();
        self
    }

    /// Get or create a session ID
    pub fn session_id(&mut self) -> SessionId {
        match self.session_id {
            Some(id) => id,
            None => {
                let id = SessionId::new();
                self.session_id = Some(id);
                id
            }
        }
    }

    /// Create a derived context for a new operation.
    /// NOTE: In production, should use RandomEffects for operation_id generation
    pub fn child_operation(&self) -> Self {
        let mut child = self.clone();
        // TODO: Replace with RandomEffects::random_uuid()
        child.operation_id = uuid::Uuid::nil(); // Placeholder - should use RandomEffects
        child.flow_hint = None;
        child
    }

    /// Set a pending flow hint for FlowGuard integration.
    pub fn set_flow_hint(&mut self, hint: FlowHint) -> &mut Self {
        self.flow_hint = Some(hint);
        self
    }

    /// Take the pending flow hint if present.
    pub fn take_flow_hint(&mut self) -> Option<FlowHint> {
        self.flow_hint.take()
    }

    /// Check if this is a deterministic execution mode
    pub fn is_deterministic(&self) -> bool {
        self.execution_mode.is_deterministic()
    }

    /// Get the simulation seed if in simulation mode
    pub fn simulation_seed(&self) -> Option<u64> {
        self.execution_mode.seed()
    }

    /// Get elapsed time since context creation
    /// NOTE: In production, should use TimeEffects for current timestamp
    pub fn elapsed(&self) -> Duration {
        // TODO: Replace with TimeEffects::current_timestamp_millis()
        // For now return zero duration to avoid disallowed method
        Duration::ZERO
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use aura_core::DeviceId;
    use uuid::Uuid;

    #[test]
    fn test_session_id() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        assert_ne!(id1, id2);

        let uuid = Uuid::new_v4();
        let id3 = SessionId::from_uuid(uuid);
        assert_eq!(id3.0, uuid);
    }

    #[test]
    fn test_choreographic_context() {
        let role = ChoreographicRole::new(Uuid::new_v4(), 0);
        let participants = vec![role];
        let mut ctx = ChoreographicContext::new(role, participants, 1);

        assert_eq!(ctx.current_role, role);
        assert_eq!(ctx.epoch, 1);
        assert_eq!(ctx.participant_count(), 1);

        // Test state management
        ctx.set_state("test", &42u32).unwrap();
        let value: Option<u32> = ctx.get_state("test").unwrap();
        assert_eq!(value, Some(42));
    }

    #[test]
    fn test_simulation_context() {
        let mut ctx = SimulationContext::new(42);
        assert_eq!(ctx.seed, 42);
        assert_eq!(ctx.simulation_time, Duration::ZERO);
        assert!(!ctx.time_controlled);

        ctx.advance_time(Duration::from_secs(1));
        assert_eq!(ctx.simulation_time, Duration::from_secs(1));

        ctx.enable_time_control();
        assert!(ctx.time_controlled);

        // Test fault injection
        assert!(!ctx.should_inject_network_fault(0.5)); // Rate is 0.0
        ctx.fault_injection.network_fault_rate = 0.3;
        assert!(!ctx.should_inject_network_fault(0.5)); // 0.5 > 0.3
        assert!(ctx.should_inject_network_fault(0.2)); // 0.2 < 0.3
    }

    #[test]
    fn test_agent_context() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let mut ctx = AgentContext::new(device_id);

        // Test configuration
        ctx.set_config("key", "value");
        assert_eq!(ctx.get_config("key"), Some("value"));
        assert_eq!(ctx.get_config("missing"), None);

        // Test sessions
        let session_id = ctx.create_session("test_session");
        assert!(ctx.get_session(&session_id).is_some());
        assert_eq!(
            ctx.get_session(&session_id).unwrap().session_type,
            "test_session"
        );

        let removed = ctx.remove_session(&session_id);
        assert!(removed.is_some());
        assert!(ctx.get_session(&session_id).is_none());
    }

    #[test]
    fn test_middleware_context() {
        let mut ctx = MiddlewareContext::new();

        // Test custom data
        ctx.set_custom_data("test", &42u32).unwrap();
        let value: Option<u32> = ctx.get_custom_data("test").unwrap();
        assert_eq!(value, Some(42));

        // Test tracing
        assert!(!ctx.tracing.enabled);
        ctx.enable_tracing("trace123".to_string(), "span456".to_string());
        assert!(ctx.tracing.enabled);
        assert_eq!(ctx.tracing.trace_id, Some("trace123".to_string()));

        // Test metrics
        assert!(!ctx.metrics.enabled);
        ctx.enable_metrics();
        assert!(ctx.metrics.enabled);

        ctx.add_metrics_label("service", "test");
        assert_eq!(ctx.metrics.labels.get("service"), Some(&"test".to_string()));
    }

    #[test]
    fn test_aura_context() {
        let device_id = DeviceId::from(Uuid::new_v4());

        // Test factory methods
        let testing_ctx = AuraContext::for_testing(device_id);
        assert_eq!(testing_ctx.execution_mode, ExecutionMode::Testing);
        assert!(testing_ctx.is_deterministic());
        assert!(testing_ctx.agent.is_some());

        let production_ctx = AuraContext::for_production(device_id);
        assert_eq!(production_ctx.execution_mode, ExecutionMode::Production);
        assert!(!production_ctx.is_deterministic());

        let simulation_ctx = AuraContext::for_simulation(device_id, 42);
        assert_eq!(
            simulation_ctx.execution_mode,
            ExecutionMode::Simulation { seed: 42 }
        );
        assert!(simulation_ctx.is_deterministic());
        assert_eq!(simulation_ctx.simulation_seed(), Some(42));
        assert!(simulation_ctx.simulation.is_some());

        // Test session ID management
        let mut ctx = AuraContext::for_testing(device_id);
        assert!(ctx.session_id.is_none());
        let session_id = ctx.session_id();
        assert!(ctx.session_id.is_some());
        assert_eq!(ctx.session_id(), session_id); // Same ID returned
    }
}
