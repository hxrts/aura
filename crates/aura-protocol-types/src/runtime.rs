//! Runtime types for session commands and responses

// Remove unused serde imports for now
use crate::{DkdResult, SessionStatus};
use aura_types::DeviceId;
use uuid::Uuid;

/// Command that can be sent to the session runtime
#[derive(Debug, Clone)]
pub enum SessionCommand {
    /// Start a new DKD session
    StartDkd {
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
    },
    /// Start a new DKD session with full context
    StartDkdWithContext {
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: usize,
        context_bytes: Vec<u8>,
        with_binding_proof: bool,
    },
    /// Start a new recovery session
    StartRecovery {
        guardian_threshold: usize,
        cooldown_seconds: u64,
    },
    /// Start a new resharing session
    StartResharing {
        new_participants: Vec<DeviceId>,
        new_threshold: usize,
    },
    /// Start FROST DKG (Distributed Key Generation)
    StartFrostDkg {
        participants: Vec<DeviceId>,
        threshold: u16,
    },
    /// Start FROST threshold signing
    StartFrostSigning {
        message: Vec<u8>,
        participants: Vec<DeviceId>,
        threshold: u16,
    },
    /// Start a new locking session
    StartLocking {
        operation_type: String, // Simplified for now to avoid aura_journal dependency
    },
    /// Start a new agent session
    StartAgent,
    /// Terminate a session
    TerminateSession { session_id: Uuid },
    /// Send event to a specific session
    SendEvent {
        session_id: Uuid,
        event: Vec<u8>, // Simplified to avoid circular dependency
    },
    /// Update session status
    UpdateStatus {
        session_id: Uuid,
        status: SessionStatus,
    },
    /// Handle transport event
    TransportEvent { event: crate::TransportEvent },
}

/// Response from session runtime operations
#[derive(Debug, Clone)]
pub enum SessionResponse {
    /// Session started successfully
    SessionStarted {
        session_id: Uuid,
        session_type: crate::SessionProtocolType,
    },
    /// Session completed with result
    SessionCompleted {
        session_id: Uuid,
        result: SessionResult,
    },
    /// Session failed with error
    SessionFailed { session_id: Uuid, error: String },
    /// Runtime error occurred
    RuntimeError(String),
}

/// Result of a completed session
#[derive(Debug, Clone)]
pub enum SessionResult {
    /// DKD session completed
    Dkd(DkdResult),
    /// Recovery session completed
    Recovery,
    /// Resharing session completed
    Resharing,
    /// FROST DKG completed
    FrostDkg,
    /// FROST signing completed
    FrostSigning(Vec<u8>), // Signature bytes
    /// Locking session completed
    Locking,
    /// Agent session result
    Agent,
}

/// Trait for transport session state to avoid circular dependencies
pub trait TransportSession: std::fmt::Debug + Send + Sync {
    /// Get the current state name
    fn state_name(&self) -> &'static str;
    /// Check if the session can terminate
    fn can_terminate(&self) -> bool;
    /// Get the device ID from the underlying protocol
    fn device_id(&self) -> DeviceId;
    /// Check if this is a final state
    fn is_final(&self) -> bool;
}
