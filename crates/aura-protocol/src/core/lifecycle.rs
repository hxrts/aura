//! Unified protocol lifecycle trait and supporting types.

use crate::core::capabilities::ProtocolEffects;
use crate::core::metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType};
use crate::core::typestate::{SessionState, SessionStateTransition};
use aura_types::{AccountId, DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::time::Duration;
use uuid::Uuid;

/// Descriptor describing a protocol instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolDescriptor {
    /// Unique identifier for protocol instance.
    pub protocol_id: Uuid,
    /// Session identifier associated with the protocol.
    pub session_id: SessionId,
    /// Local device identifier executing the protocol.
    pub device_id: DeviceId,
    /// Protocol type.
    pub protocol_type: ProtocolType,
    /// Operation type semantics.
    pub operation_type: OperationType,
    /// Execution priority.
    pub priority: ProtocolPriority,
    /// Execution mode.
    pub mode: ProtocolMode,
}

impl ProtocolDescriptor {
    /// Utility constructor.
    pub fn new(
        protocol_id: Uuid,
        session_id: SessionId,
        device_id: DeviceId,
        protocol_type: ProtocolType,
    ) -> Self {
        Self {
            protocol_id,
            session_id,
            device_id,
            protocol_type,
            operation_type: protocol_type.into(),
            priority: ProtocolPriority::Normal,
            mode: ProtocolMode::Asynchronous,
        }
    }

    /// With explicit operation type.
    pub fn with_operation_type(mut self, op: OperationType) -> Self {
        self.operation_type = op;
        self
    }

    /// With priority.
    pub fn with_priority(mut self, priority: ProtocolPriority) -> Self {
        self.priority = priority;
        self
    }

    /// With execution mode.
    pub fn with_mode(mut self, mode: ProtocolMode) -> Self {
        self.mode = mode;
        self
    }
}

/// Input stimulus delivered to a protocol step.
#[derive(Debug, Clone)]
pub enum ProtocolInput<'a> {
    /// Transport message received from peer.
    Message(&'a crate::core::capabilities::ProtocolMessage),
    /// Journal event delivered from ledger.
    Journal {
        /// Event type string.
        event_type: &'a str,
        /// Raw payload.
        payload: &'a serde_json::Value,
    },
    /// Timer tick previously scheduled by protocol.
    Timer {
        /// Timer identifier.
        timer_id: Uuid,
        /// Duration originally requested.
        timeout: Duration,
    },
    /// Local API invocation.
    LocalSignal {
        /// Signal identifier.
        signal: &'a str,
        /// Arbitrary parameters.
        data: Option<&'a serde_json::Value>,
    },
}

/// Result of processing a single input.
#[derive(Debug)]
pub struct ProtocolStep<O, E> {
    /// Side effects requested by the protocol.
    pub effects: Vec<ProtocolEffects>,
    /// Optional typestate transition metadata.
    pub transition: Option<SessionStateTransition>,
    /// Optional output completion.
    pub outcome: Option<Result<O, E>>,
}

impl<O, E> ProtocolStep<O, E> {
    /// Convenience constructor for progress without completion.
    pub fn progress(
        effects: Vec<ProtocolEffects>,
        transition: Option<SessionStateTransition>,
    ) -> Self {
        Self {
            effects,
            transition,
            outcome: None,
        }
    }

    /// Completed step with outcome.
    pub fn completed(
        effects: Vec<ProtocolEffects>,
        transition: Option<SessionStateTransition>,
        outcome: Result<O, E>,
    ) -> Self {
        Self {
            effects,
            transition,
            outcome: Some(outcome),
        }
    }
}

/// Unified trait implemented by all protocols.
pub trait ProtocolLifecycle: Send + Sync {
    /// Typestate marker for the protocol.
    type State: SessionState;
    /// Successful output type.
    type Output: Send + Sync;
    /// Error type.
    type Error: Debug + Send + Sync;

    /// Fetch descriptor for orchestration and metrics.
    fn descriptor(&self) -> &ProtocolDescriptor;

    /// Execute the protocol for a single input using supplied capabilities.
    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        caps: &mut crate::core::capabilities::ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error>;

    /// Whether protocol reached terminal state.
    fn is_final(&self) -> bool;
}

/// Crash rehydration support.
pub trait ProtocolRehydration: ProtocolLifecycle {
    /// Evidence payload captured in journal.
    type Evidence: Clone + Debug + Send + Sync + 'static;
    /// Validate evidence sufficiency.
    fn validate_evidence(evidence: &Self::Evidence) -> bool;
    /// Rehydrate instance from evidence.
    fn rehydrate(
        device_id: DeviceId,
        account_id: AccountId,
        evidence: Self::Evidence,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

/// Helper to produce state transition metadata.
pub fn transition_from_witness(
    desc: &ProtocolDescriptor,
    from: &str,
    to: &str,
    witness_desc: Option<&str>,
) -> SessionStateTransition {
    SessionStateTransition {
        protocol_id: desc.protocol_id,
        from_state: from.to_string(),
        to_state: to.to_string(),
        witness: witness_desc.map(|s| s.to_string()),
    }
}
