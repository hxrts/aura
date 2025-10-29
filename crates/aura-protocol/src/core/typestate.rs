//! Typestate primitives for protocol state machines.

use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use uuid::Uuid;

/// Marker trait for valid protocol states.
pub trait SessionState: Send + Sync + Debug + 'static {
    /// Human-readable state name.
    const NAME: &'static str;

    /// Whether this state is terminal.
    const IS_FINAL: bool = false;

    /// Whether protocol execution may terminate in this state.
    const CAN_TERMINATE: bool = false;
}

/// Opaque state marker used for type-erased envelopes.
#[derive(Debug)]
pub struct AnyProtocolState;

impl SessionState for AnyProtocolState {
    const NAME: &'static str = "erased";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Witness type for validated state transitions.
pub trait StateWitness: Send + Sync + Debug {
    /// Evidence payload used to verify the witness.
    type Evidence: Clone + Debug + Send + Sync + 'static;
    /// Configuration input required to validate evidence.
    type Config: Clone + Debug + Send + Sync + 'static;

    /// Verify evidence and construct a witness.
    fn verify(evidence: Self::Evidence, config: Self::Config) -> Option<Self>
    where
        Self: Sized;

    /// Human-friendly description of the witness.
    fn description(&self) -> &'static str;
}

/// Transition event emitted by protocols when mutating their typestate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStateTransition {
    /// Protocol instance identifier.
    pub protocol_id: Uuid,
    /// Human-readable state name prior to transition.
    pub from_state: String,
    /// Human-readable state name after transition.
    pub to_state: String,
    /// Optional witness description.
    pub witness: Option<String>,
}
