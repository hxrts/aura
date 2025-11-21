//! Transport Coordination Layer
//!
//! Layer 4: Multi-party transport coordination using choreographic protocols.
//! YES choreography - for complex distributed coordination patterns.
//! NO choreography - for local effect composition and simple orchestration.
//! Target: Each choreographic protocol <250 lines.

pub mod channel_management;
pub mod websocket;

#[cfg(all(test, feature = "transport_legacy_tests"))]
mod tests;

pub use channel_management::{ChannelEstablishmentCoordinator, ChannelTeardownCoordinator};
pub use websocket::{WebSocketHandshakeCoordinator, WebSocketSessionCoordinator};

/// Choreographic coordination configuration
#[derive(Debug, Clone)]
pub struct ChoreographicConfig {
    /// Protocol execution timeout
    pub execution_timeout: std::time::Duration,
    /// Maximum concurrent protocols
    pub max_concurrent_protocols: usize,
    /// Default flow budget per protocol
    pub default_flow_budget: u32,
    /// Capability requirements
    pub required_capabilities: Vec<String>,
}

impl Default for ChoreographicConfig {
    fn default() -> Self {
        Self {
            execution_timeout: std::time::Duration::from_secs(60),
            max_concurrent_protocols: 10,
            default_flow_budget: 1000,
            required_capabilities: vec!["choreographic_coordination".to_string()],
        }
    }
}

/// Choreographic protocol error types
#[derive(Debug, thiserror::Error)]
pub enum ChoreographicError {
    #[error("Protocol execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Session type violation: {0}")]
    SessionTypeViolation(String),
    #[error("Capability requirement not met: {0}")]
    CapabilityNotMet(String),
    #[error("Flow budget exceeded: required {required}, available {available}")]
    FlowBudgetExceeded { required: u32, available: u32 },
    #[error("Journal synchronization failed: {0}")]
    JournalSyncFailed(String),
    #[error("Transport coordination error: {0}")]
    TransportCoordination(#[from] TransportCoordinationError),
}

type ChoreographicResult<T> = Result<T, ChoreographicError>;

/// Transport coordination configuration
#[derive(Debug, Clone)]
pub struct TransportCoordinationConfig {
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Connection timeout
    pub connection_timeout: std::time::Duration,
    /// Retry attempts for choreographic protocols
    pub max_retries: u32,
    /// Default capability requirements
    pub default_capabilities: Vec<String>,
}

impl Default for TransportCoordinationConfig {
    fn default() -> Self {
        Self {
            max_connections: 100,
            connection_timeout: std::time::Duration::from_secs(30),
            max_retries: 3,
            default_capabilities: vec!["transport_basic".to_string()],
        }
    }
}

/// Transport coordination error types
#[derive(Debug, thiserror::Error)]
pub enum TransportCoordinationError {
    #[error("Protocol execution failed: {0}")]
    ProtocolFailed(String),
    #[error("Capability check failed: {0}")]
    CapabilityCheckFailed(String),
    #[error("Flow budget exceeded: {0}")]
    FlowBudgetExceeded(String),
    #[error("Journal sync failed: {0}")]
    JournalSyncFailed(String),
    #[error("Transport error: {0}")]
    Transport(#[from] aura_effects::transport::TransportError),
    #[error("Effect error: {0}")]
    Effect(String),
}

type CoordinationResult<T> = Result<T, TransportCoordinationError>;
