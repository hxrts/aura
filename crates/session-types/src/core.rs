//! Core session type traits and foundational types
//!
//! This module defines the fundamental abstractions for session types in Aura's
//! choreographic programming model.

use crate::RuntimeWitness;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

/// Core trait for all session-typed protocols
///
/// This trait provides the foundation for compile-time safe protocol state management.
/// All protocol state machines implement this trait to enable type-safe transitions.
pub trait SessionProtocol: Send + Sync + Clone + fmt::Debug {
    /// The phantom type representing the current state
    type State: SessionState;
    
    /// The output type for this protocol
    type Output: Send + Sync;
    
    /// The error type for this protocol
    type Error: Send + Sync + fmt::Debug;
    
    /// Get the protocol's unique session identifier
    fn session_id(&self) -> Uuid;
    
    /// Get the current state name for debugging and logging
    fn state_name(&self) -> &'static str;
    
    /// Check if the protocol can be safely terminated
    fn can_terminate(&self) -> bool;
    
    /// Get the protocol's unique identifier
    fn protocol_id(&self) -> Uuid;
    
    /// Get the device ID for this protocol instance
    fn device_id(&self) -> aura_journal::DeviceId;
}

/// Marker trait for valid session states
///
/// This trait is implemented by all valid protocol states to enable
/// compile-time verification of state transitions.
pub trait SessionState: Send + Sync + Clone + fmt::Debug + 'static {
    /// The state name for debugging and serialization
    const NAME: &'static str;
    
    /// Whether this is a terminal state
    const IS_FINAL: bool = false;
    
    /// Whether this state allows termination
    const CAN_TERMINATE: bool = false;
}

/// Trait for compile-time safe state transitions
///
/// This trait enables type-safe transitions between protocol states while
/// maintaining choreographic correctness guarantees.
pub trait WitnessedTransition<FromState, ToState>: SessionProtocol 
where 
    FromState: SessionState,
    ToState: SessionState,
{
    /// The witness type that proves the transition is valid
    type Witness: RuntimeWitness;
    
    /// The target protocol type after transition
    type Target: SessionProtocol<State = ToState>;
    
    /// Perform a witnessed transition with explicit witness
    fn transition_with_witness(self, witness: Self::Witness) -> Self::Target;
}

/// Core implementation of a choreographic protocol
///
/// This struct wraps protocol-specific data with session type state information.
#[derive(Debug, Clone)]
pub struct ChoreographicProtocol<Core, State> {
    /// The protocol-specific core data
    pub inner: Core,
    /// The current state (phantom type for compile-time safety)
    _state: std::marker::PhantomData<State>,
}

impl<Core, State> ChoreographicProtocol<Core, State>
where
    State: SessionState,
{
    /// Create a new choreographic protocol instance
    pub fn new(inner: Core) -> Self {
        Self {
            inner,
            _state: std::marker::PhantomData,
        }
    }
    
    /// Get the current state name
    pub fn state_name(&self) -> &'static str {
        State::NAME
    }
    
    /// Get the current state name (alias for compatibility)
    pub fn current_state_name(&self) -> &'static str {
        State::NAME
    }
    
    /// Check if this is a final state
    pub fn is_final(&self) -> bool {
        State::IS_FINAL
    }
    
    /// Transition to a new state (consuming self)
    pub fn transition_to<NewState: SessionState>(self) -> ChoreographicProtocol<Core, NewState> {
        ChoreographicProtocol {
            inner: self.inner,
            _state: std::marker::PhantomData,
        }
    }
}

/// Protocol rehydration from crash recovery
///
/// Protocols can be reconstructed from journal evidence after crashes
/// or restarts, maintaining choreographic consistency.
pub trait ProtocolRehydration: SessionProtocol {
    /// The type of evidence needed for rehydration
    type Evidence: Clone + fmt::Debug + Send + Sync;
    
    /// Rehydrate protocol state from journal evidence
    fn rehydrate_from_evidence(
        device_id: aura_journal::DeviceId,
        evidence: Self::Evidence,
    ) -> Result<Self, SessionError>;
    
    /// Validate that evidence is sufficient for rehydration
    fn validate_evidence(evidence: &Self::Evidence) -> bool;
}

/// State analysis for protocol introspection
///
/// Enables runtime analysis of protocol state for debugging,
/// monitoring, and choreographic verification.
pub trait StateAnalysis: SessionProtocol {
    /// Get detailed state information
    fn state_info(&self) -> StateInfo;
    
    /// Get valid next states from current state
    fn valid_transitions(&self) -> Vec<&'static str>;
    
    /// Check if a specific transition is valid
    fn can_transition_to(&self, target_state: &str) -> bool {
        self.valid_transitions().contains(&target_state)
    }
}

/// Detailed information about protocol state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateInfo {
    /// Current state name
    pub state_name: String,
    /// Whether state is terminal
    pub is_terminal: bool,
    /// Whether protocol can be terminated
    pub can_terminate: bool,
    /// Protocol unique identifier
    pub protocol_id: Uuid,
    /// Device identifier
    pub device_id: aura_journal::DeviceId,
    /// Optional state-specific metadata
    pub metadata: Option<serde_json::Value>,
}

/// Session type errors
#[derive(Error, Debug, Clone)]
pub enum SessionError {
    /// Invalid state transition attempted
    #[error("Invalid transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },
    
    /// Protocol rehydration failed
    #[error("Failed to rehydrate protocol: {reason}")]
    RehydrationFailed { reason: String },
    
    /// Insufficient evidence for rehydration
    #[error("Insufficient evidence for protocol rehydration")]
    InsufficientEvidence,
    
    /// Protocol invariant violation
    #[error("Protocol invariant violation: {invariant}")]
    InvariantViolation { invariant: String },
    
    /// Choreographic consistency error
    #[error("Choreographic consistency error: {details}")]
    ChoreographicError { details: String },
    
    /// Protocol timeout
    #[error("Protocol timeout after {duration_ms}ms")]
    Timeout { duration_ms: u64 },
    
    /// Serialization/deserialization error
    #[error("Serialization error: {message}")]
    SerializationError { message: String },
}