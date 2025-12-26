#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Orchestration layer coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! Layer 4: Transport Coordination - Multi-Party Protocol Orchestration
//!
//! Multi-party transport coordination orchestrating choreographic protocols at transport layer.
//! Coordinates AMP (Attestation Multi-Party), channel establishment/teardown, WebSocket handshakes,
//! with flow budgets and guard chain integration.
//!
//! **Integration** (per docs/108_transport_and_information_flow.md):
//! - Messages from all protocol layers flow through this transport coordination layer
//! - Guard chain evaluation (CapGuard → FlowGuard → JournalCoupler) happens at layer entry
//! - Choreographic protocols project to per-role local types (aura-mpst Layer 2)
//! - Receipt propagation for flow budget accounting (per docs/003_information_flow_contract.md)
//!
//! **Key Coordinators**:
//! - **AMP Transport**: Threshold cryptography message coordination
//! - **Channel Management**: Connection establishment/teardown choreography
//! - **WebSocket**: Browser-compatible real-time bidirectional communication
//!
//! **Flow Budget Enforcement**: Each message send atomically increments spent counter;
//! receipts prove charges for relayed messages

pub mod channel_management;
pub mod websocket;

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
