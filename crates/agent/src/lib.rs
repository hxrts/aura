//! DeviceAgent: per-device identity and session management
//!
//! This crate implements the device-side agent that:
//! - Derives context-specific identities using DKD
//! - Issues and verifies session credentials
//! - Manages local key shares
//! - Coordinates with other devices for threshold operations
//!
//! # **SECURITY WARNING**
//!
//! The current DKD implementation is **INSECURE FOR PRODUCTION**.
//! See documentation in [`dkd`] module for details and required fixes.
//!
//! # Main Components
//!
//! - [`agent::DeviceAgent`]: Main API for device operations
//! - [`dkd`]: Deterministic key derivation (insecure)
//! - [`credential`]: Session credential issuance and verification
//! - [`types`]: Core types for agent operations

pub mod types;
// NOTE: dkd.rs DELETED - was single-device, not P2P protocol
// TODO Phase 2: Implement P2P DKD via DkdOrchestrator in aura_coordination
pub mod credential;
pub mod agent;
pub mod guardian;
pub mod recovery;

pub use types::*;
pub use credential::*;
pub use agent::*;
pub use guardian::*;
pub use recovery::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Orchestrator error: {0}")]
    OrchestratorError(String),
    
    #[error("Ledger error: {0}")]
    LedgerError(String),
    
    #[error("Invalid context: {0}")]
    InvalidContext(String),
    
    #[error("DKD failed: {0}")]
    DkdFailed(String),
    
    #[error("Session epoch mismatch: {0}")]
    EpochMismatch(String),
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Cryptographic error: {0}")]
    CryptoError(String),
    
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    
    #[error("System time error: {0}")]
    SystemTimeError(String),
}

pub type Result<T> = std::result::Result<T, AgentError>;

impl From<aura_coordination::ProtocolError> for AgentError {
    fn from(error: aura_coordination::ProtocolError) -> Self {
        AgentError::DkdFailed(format!("Protocol error: {:?}", error))
    }
}

