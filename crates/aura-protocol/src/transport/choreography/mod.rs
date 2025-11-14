//! Transport Choreographic Protocols
//!
//! Layer 4: Multi-party choreographic protocol implementations.
//! YES choreography - complex distributed coordination patterns.
//! Target: Each protocol <250 lines, focused on session type safety.

pub mod websocket;
pub mod channel_management;
pub mod receipt_verification;

pub use websocket::{WebSocketHandshakeCoordinator, WebSocketSessionCoordinator};
pub use channel_management::{ChannelEstablishmentCoordinator, ChannelTeardownCoordinator};
pub use receipt_verification::ReceiptCoordinationProtocol;

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
    TransportCoordination(#[from] super::TransportCoordinationError),
}

type ChoreographicResult<T> = Result<T, ChoreographicError>;
