//! Aura Agent: Capability-driven identity and session management
//!
//! This crate provides capability-based agents for the Aura platform:
//! - Pure capability-driven authorization (no legacy policies)
//! - Group messaging with BeeKEM for secure communication
//! - Causal encryption for forward secrecy
//! - Network transport and storage integration
//! - Distributed protocol coordination
//!
//! # Agent Architecture
//!
//! ## [`CapabilityAgent`]: Core capability-driven functionality
//! - Pure capability-based authorization and group messaging
//! - No external dependencies (transport, storage)
//! - Ideal for testing, embedded systems, library integration
//! - Methods: `check_capability()`, `create_group()`, `encrypt()`, `decrypt()`
//!
//! ## [`IntegratedAgent`]: Full system integration
//! - All CapabilityAgent features plus transport and storage
//! - Network-aware capability delegation and revocation
//! - Encrypted data storage with capability-based access control
//! - Methods: `bootstrap()`, `network_connect()`, `store()`, `retrieve()`
//!
//!
//! # Method Naming Conventions
//!
//! - **Core operations**: `check_capability()`, `require_capability()`
//! - **Group operations**: `create_group()`, `list_groups()`
//! - **Data operations**: `encrypt()`, `decrypt()`, `store()`, `retrieve()`
//! - **Network operations**: `network_` prefix for distributed operations
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use aura_agent::{CapabilityAgent, IntegratedAgent};
//! use aura_journal::{DeviceId, AccountId, CapabilityScope};
//!
//! // Core capability agent
//! let device_id = DeviceId::new();
//! let account_id = AccountId::new();
//! let mut agent = CapabilityAgent::new(device_id, account_id);
//!
//! // Bootstrap new account
//! agent.bootstrap_account(vec![device_id], 2)?;
//!
//! // Check capabilities
//! let scope = CapabilityScope::simple("mls", "admin");
//! if agent.check_capability(&scope) {
//!     // Create group with new naming
//!     agent.create_group("team-chat", vec![])?;
//! }
//!
//! // Full integrated agent
//! let integrated = IntegratedAgent::new(device_id, account_id, storage_path).await?;
//! integrated.bootstrap(initial_devices, threshold).await?;
//! integrated.network_connect(peer_id, "127.0.0.1:8080").await?;
//! ```

/// Core types and data structures used throughout the agent system
pub mod types;
// NOTE: dkd.rs DELETED - was single-device, not P2P protocol
// TODO Phase 2: Implement P2P DKD via DkdOrchestrator in aura_coordination
/// Core agent functionality and protocol orchestration
pub mod agent;
/// Credential management and session tickets for agent authentication
pub mod credential;
/// Guardian management for account recovery and delegation
pub mod guardian;
/// Recovery protocols for restoring access to compromised accounts
pub mod recovery;
/// Session-typed agent implementation with protocol safety
pub mod session_agent;

// New capability-driven agent architecture
/// Pure capability-driven agent with no external dependencies
pub mod capability_agent;
/// Integrated agent with transport and storage capabilities
pub mod integrated_agent;

pub use agent::*;
pub use credential::*;
pub use guardian::*;
pub use recovery::*;
pub use types::*;

// Export new capability-driven agents
pub use capability_agent::{AgentConfig, CapabilityAgent};
pub use integrated_agent::{IntegratedAgent, NetworkStats, StorageStats};

// Export session-typed agents
pub use session_agent::{SessionTypedDeviceAgent, DeviceAgentCompat};

use thiserror::Error;

/// Agent operation errors
///
/// Comprehensive error types covering all agent operations including
/// protocol coordination, capability checking, and integration with
/// transport and storage layers.
#[derive(Error, Debug)]
pub enum AgentError {
    /// Error in protocol orchestration or coordination
    #[error("Orchestrator error: {0}")]
    OrchestratorError(String),

    /// Error in ledger operations or state management
    #[error("Ledger error: {0}")]
    LedgerError(String),

    /// Invalid context provided for operation
    #[error("Invalid context: {0}")]
    InvalidContext(String),

    /// Invalid credential or signature
    #[error("Invalid credential: {0}")]
    InvalidCredential(String),

    /// Deterministic Key Derivation protocol failure
    #[error("DKD failed: {0}")]
    DkdFailed(String),

    /// Session epoch mismatch between participants
    #[error("Session epoch mismatch: {0}")]
    EpochMismatch(String),

    /// Requested device not found in account
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Data serialization or deserialization failure
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Cryptographic operation failure
    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    /// Feature not yet implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// System time access or manipulation error
    #[error("System time error: {0}")]
    SystemTimeError(String),

    // Capability-specific errors
    /// Operation requires capability not possessed by agent
    #[error("Insufficient capability: {0}")]
    InsufficientCapability(String),

    /// General capability system error
    #[error("Capability error: {0}")]
    CapabilityError(String),

    /// Account bootstrap or initialization failure
    #[error("Bootstrap error: {0}")]
    BootstrapError(String),

    /// Continuous Group Key Agreement protocol error
    #[error("CGKA error: {0}")]
    CgkaError(String),

    // Transport and storage errors
    /// Network transport layer error
    #[error("Transport error: {0}")]
    TransportError(String),

    /// Storage layer operation failure
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Network communication error
    #[error("Network error: {0}")]
    NetworkError(String),
}

/// Result type alias for agent operations
///
/// Provides a convenient Result<T> that defaults to AgentError for error cases.
pub type Result<T> = std::result::Result<T, AgentError>;

impl From<aura_crypto::CryptoError> for AgentError {
    fn from(error: aura_crypto::CryptoError) -> Self {
        AgentError::CryptoError(format!("Crypto error: {:?}", error))
    }
}

// TODO: Re-enable when coordination is fixed
// impl From<aura_coordination::ProtocolError> for AgentError {
//     fn from(error: aura_coordination::ProtocolError) -> Self {
//         AgentError::DkdFailed(format!("Protocol error: {:?}", error))
//     }
// }
