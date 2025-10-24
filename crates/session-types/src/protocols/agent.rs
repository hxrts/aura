//! Session Type States for Agent State Management (Refactored with Macros)
//!
//! This module defines session types for DeviceAgent, providing compile-time safety
//! for protocol coordination and state management at the device level.

use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState, WitnessedTransition};
use crate::witnesses::RuntimeWitness;
use aura_journal::{DeviceId, ProtocolType, SessionId, SessionOutcome, SessionStatus};
use uuid::Uuid;

// ========== Agent Core ==========

/// Core DeviceAgent data without session state
#[derive(Debug, Clone)]
pub struct DeviceAgentCore {
    pub device_id: DeviceId,
    pub session_id: Uuid,
    pub current_protocol: Option<ProtocolType>,
    pub active_sessions: Vec<SessionId>,
}

impl DeviceAgentCore {
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            session_id: Uuid::new_v4(),
            current_protocol: None,
            active_sessions: Vec::new(),
        }
    }
}

// ========== Error Type ==========

#[derive(Debug, thiserror::Error)]
pub enum AgentSessionError {
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid operation for current agent state")]
    InvalidOperation,
    #[error("Agent operation failed: {0}")]
    OperationFailed(String),
    #[error("Session management error: {0}")]
    SessionError(String),
    #[error("Concurrent protocol already active: {0:?}")]
    ConcurrentProtocol(ProtocolType),
}

// ========== Protocol Definition using Macros ==========

// ========== Session States ==========

// Define the states manually since AgentIdle needs special properties
define_session_states! {
    DkdInProgress,
    RecoveryInProgress,
    ResharingInProgress,
    LockingInProgress,
    AgentOperationLocked,
    AgentFailed @ final,
}

// AgentIdle is special - it can terminate but is not final
#[derive(Debug, Clone)]
pub struct AgentIdle;

impl SessionState for AgentIdle {
    const NAME: &'static str = "AgentIdle";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = false;
}

// ========== Protocol Implementation ==========

impl_session_protocol! {
    for AgentProtocol<Core = DeviceAgentCore, Error = AgentSessionError> {
        AgentIdle => (),
        DkdInProgress => Vec<u8>,
        RecoveryInProgress => (),
        ResharingInProgress => (),
        LockingInProgress => (),
        AgentOperationLocked => (),
        AgentFailed => (),
    }

    session_id: |core| core.session_id,
    device_id: |core| core.device_id,
}

// ========== Union Type ==========

define_session_union! {
    pub enum AgentSessionState for DeviceAgentCore {
        AgentIdle,
        DkdInProgress,
        RecoveryInProgress,
        ResharingInProgress,
        LockingInProgress,
        AgentOperationLocked,
        AgentFailed,
    }

    delegate: [state_name, can_terminate, protocol_id, device_id]
}

// ========== Protocol Type Alias ==========

/// Session-typed device agent wrapper
pub type SessionTypedAgent<S> = ChoreographicProtocol<DeviceAgentCore, S>;

// ========== Runtime Witnesses for Agent Operations ==========

/// Witness that DKD protocol has completed successfully
#[derive(Debug, Clone)]
pub struct DkdCompleted {
    pub session_id: SessionId,
    pub derived_key: Vec<u8>,
    pub participants: Vec<DeviceId>,
}

impl RuntimeWitness for DkdCompleted {
    type Evidence = (SessionId, SessionOutcome);
    type Config = ProtocolType;

    fn verify(evidence: (SessionId, SessionOutcome), config: ProtocolType) -> Option<Self> {
        let (session_id, outcome) = evidence;
        if config == ProtocolType::Dkd && outcome == SessionOutcome::Success {
            Some(DkdCompleted {
                session_id,
                derived_key: vec![], // Would be populated from actual protocol result
                participants: vec![], // Would be populated from session data
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "DKD protocol completed successfully"
    }
}

/// Witness that recovery protocol has completed successfully
#[derive(Debug, Clone)]
pub struct RecoveryCompleted {
    pub session_id: SessionId,
    pub recovered_shares: usize,
    pub new_configuration: Option<(usize, usize)>, // (threshold, total)
}

impl RuntimeWitness for RecoveryCompleted {
    type Evidence = (SessionId, SessionOutcome);
    type Config = ProtocolType;

    fn verify(evidence: (SessionId, SessionOutcome), config: ProtocolType) -> Option<Self> {
        let (session_id, outcome) = evidence;
        if config == ProtocolType::Recovery && outcome == SessionOutcome::Success {
            Some(RecoveryCompleted {
                session_id,
                recovered_shares: 0, // Would be populated from actual protocol result
                new_configuration: None, // Would be populated from protocol result
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery protocol completed successfully"
    }
}

/// Witness that resharing protocol has completed successfully
#[derive(Debug, Clone)]
pub struct AgentResharingCompleted {
    pub session_id: SessionId,
    pub new_participants: Vec<DeviceId>,
    pub new_threshold: usize,
}

impl RuntimeWitness for AgentResharingCompleted {
    type Evidence = (SessionId, SessionOutcome);
    type Config = ProtocolType;

    fn verify(evidence: (SessionId, SessionOutcome), config: ProtocolType) -> Option<Self> {
        let (session_id, outcome) = evidence;
        if config == ProtocolType::Resharing && outcome == SessionOutcome::Success {
            Some(AgentResharingCompleted {
                session_id,
                new_participants: vec![], // Would be populated from actual protocol result
                new_threshold: 0,         // Would be populated from protocol result
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Resharing protocol completed successfully"
    }
}

/// Witness that agent operation lock has been acquired
#[derive(Debug, Clone)]
pub struct AgentLockAcquired {
    pub session_id: SessionId,
    pub operation_type: aura_journal::OperationType,
    pub lock_holder: DeviceId,
}

impl RuntimeWitness for AgentLockAcquired {
    type Evidence = (SessionId, SessionOutcome);
    type Config = aura_journal::OperationType;

    fn verify(
        evidence: (SessionId, SessionOutcome),
        config: aura_journal::OperationType,
    ) -> Option<Self> {
        let (session_id, outcome) = evidence;
        if outcome == SessionOutcome::Success {
            Some(AgentLockAcquired {
                session_id,
                operation_type: config,
                lock_holder: DeviceId(uuid::Uuid::new_v4()), // Would be populated from actual protocol result
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Operation lock acquired successfully"
    }
}

/// Witness that protocol has failed
#[derive(Debug, Clone)]
pub struct ProtocolFailed {
    pub session_id: SessionId,
    pub protocol_type: ProtocolType,
    pub failure_reason: String,
    pub blamed_party: Option<DeviceId>,
}

impl RuntimeWitness for ProtocolFailed {
    type Evidence = (SessionId, ProtocolType, String, Option<DeviceId>);
    type Config = ();

    fn verify(
        evidence: (SessionId, ProtocolType, String, Option<DeviceId>),
        _config: (),
    ) -> Option<Self> {
        let (session_id, protocol_type, failure_reason, blamed_party) = evidence;
        Some(ProtocolFailed {
            session_id,
            protocol_type,
            failure_reason,
            blamed_party,
        })
    }

    fn description(&self) -> &'static str {
        "Protocol execution failed"
    }
}

// ========== State Transitions ==========

/// Transitions from AgentIdle to protocol-specific states
impl ChoreographicProtocol<DeviceAgentCore, AgentIdle> {
    /// Start DKD protocol (no witness needed)
    pub fn begin_dkd(
        mut self,
        protocol_session_id: SessionId,
    ) -> ChoreographicProtocol<DeviceAgentCore, DkdInProgress> {
        self.inner.current_protocol = Some(ProtocolType::Dkd);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }

    /// Start recovery protocol (no witness needed)
    pub fn begin_recovery(
        mut self,
        protocol_session_id: SessionId,
    ) -> ChoreographicProtocol<DeviceAgentCore, RecoveryInProgress> {
        self.inner.current_protocol = Some(ProtocolType::Recovery);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }

    /// Start resharing protocol (no witness needed)
    pub fn begin_resharing(
        mut self,
        protocol_session_id: SessionId,
    ) -> ChoreographicProtocol<DeviceAgentCore, ResharingInProgress> {
        self.inner.current_protocol = Some(ProtocolType::Resharing);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }

    /// Start locking protocol (no witness needed)
    pub fn begin_locking(
        mut self,
        protocol_session_id: SessionId,
    ) -> ChoreographicProtocol<DeviceAgentCore, LockingInProgress> {
        self.inner.current_protocol = Some(ProtocolType::LockAcquisition);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }
}

/// Transition from DkdInProgress back to AgentIdle (requires DkdCompleted witness)
impl WitnessedTransition<DkdInProgress, AgentIdle>
    for ChoreographicProtocol<DeviceAgentCore, DkdInProgress>
{
    type Witness = DkdCompleted;
    type Target = ChoreographicProtocol<DeviceAgentCore, AgentIdle>;

    /// Complete DKD protocol successfully
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_protocol = None;
        self.inner
            .active_sessions
            .retain(|&id| id != witness.session_id);
        self.transition_to()
    }
}

/// Transition from RecoveryInProgress back to AgentIdle (requires RecoveryCompleted witness)
impl WitnessedTransition<RecoveryInProgress, AgentIdle>
    for ChoreographicProtocol<DeviceAgentCore, RecoveryInProgress>
{
    type Witness = RecoveryCompleted;
    type Target = ChoreographicProtocol<DeviceAgentCore, AgentIdle>;

    /// Complete recovery protocol successfully
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_protocol = None;
        self.inner
            .active_sessions
            .retain(|&id| id != witness.session_id);
        self.transition_to()
    }
}

/// Transition from ResharingInProgress back to AgentIdle (requires AgentResharingCompleted witness)
impl WitnessedTransition<ResharingInProgress, AgentIdle>
    for ChoreographicProtocol<DeviceAgentCore, ResharingInProgress>
{
    type Witness = AgentResharingCompleted;
    type Target = ChoreographicProtocol<DeviceAgentCore, AgentIdle>;

    /// Complete resharing protocol successfully
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_protocol = None;
        self.inner
            .active_sessions
            .retain(|&id| id != witness.session_id);
        self.transition_to()
    }
}

/// Transition from LockingInProgress to AgentOperationLocked (requires AgentLockAcquired witness)
impl WitnessedTransition<LockingInProgress, AgentOperationLocked>
    for ChoreographicProtocol<DeviceAgentCore, LockingInProgress>
{
    type Witness = AgentLockAcquired;
    type Target = ChoreographicProtocol<DeviceAgentCore, AgentOperationLocked>;

    /// Successfully acquire operation lock
    fn transition_with_witness(mut self, _witness: Self::Witness) -> Self::Target {
        self.inner.current_protocol = Some(ProtocolType::Locking);
        self.transition_to()
    }
}

/// Transition from AgentOperationLocked back to AgentIdle (no witness needed)
impl ChoreographicProtocol<DeviceAgentCore, AgentOperationLocked> {
    /// Release the operation lock
    pub fn release_lock(
        mut self,
        session_id: SessionId,
    ) -> ChoreographicProtocol<DeviceAgentCore, AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.retain(|&id| id != session_id);
        self.transition_to()
    }
}

/// Transition to AgentFailed from any state (requires ProtocolFailed witness)
impl<S: SessionState> WitnessedTransition<S, AgentFailed>
    for ChoreographicProtocol<DeviceAgentCore, S>
where
    Self: SessionProtocol<State = S, Output = (), Error = AgentSessionError>,
{
    type Witness = ProtocolFailed;
    type Target = ChoreographicProtocol<DeviceAgentCore, AgentFailed>;

    /// Fail the agent due to protocol error
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        self.inner.current_protocol = None;
        self.inner
            .active_sessions
            .retain(|&id| id != witness.session_id);
        self.transition_to()
    }
}

// ========== Agent-Specific Operations ==========

/// Operations only available in AgentIdle state
impl ChoreographicProtocol<DeviceAgentCore, AgentIdle> {
    /// Check if agent can start a new protocol
    pub fn can_start_protocol(&self, protocol_type: ProtocolType) -> Result<(), AgentSessionError> {
        if self.inner.current_protocol.is_some() {
            return Err(AgentSessionError::ConcurrentProtocol(protocol_type));
        }
        Ok(())
    }

    /// Get current active sessions
    pub fn active_sessions(&self) -> &[SessionId] {
        &self.inner.active_sessions
    }

    /// Check if agent is truly idle (no active sessions)
    pub fn is_completely_idle(&self) -> bool {
        self.inner.active_sessions.is_empty() && self.inner.current_protocol.is_none()
    }
}

/// Operations only available in DkdInProgress state
impl ChoreographicProtocol<DeviceAgentCore, DkdInProgress> {
    /// Get the current DKD session
    pub fn current_dkd_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    /// Check DKD progress
    pub async fn check_dkd_progress(&self, session_status: SessionStatus) -> Option<DkdCompleted> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_dkd_session() {
                return DkdCompleted::verify(
                    (session_id, SessionOutcome::Success),
                    ProtocolType::Dkd,
                );
            }
        }
        None
    }
}

/// Operations only available in RecoveryInProgress state
impl ChoreographicProtocol<DeviceAgentCore, RecoveryInProgress> {
    /// Get the current recovery session
    pub fn current_recovery_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    /// Check recovery progress
    pub async fn check_recovery_progress(
        &self,
        session_status: SessionStatus,
    ) -> Option<RecoveryCompleted> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_recovery_session() {
                return RecoveryCompleted::verify(
                    (session_id, SessionOutcome::Success),
                    ProtocolType::Recovery,
                );
            }
        }
        None
    }
}

/// Operations only available in ResharingInProgress state
impl ChoreographicProtocol<DeviceAgentCore, ResharingInProgress> {
    /// Get the current resharing session
    pub fn current_resharing_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    /// Check resharing progress
    pub async fn check_resharing_progress(
        &self,
        session_status: SessionStatus,
    ) -> Option<AgentResharingCompleted> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_resharing_session() {
                return AgentResharingCompleted::verify(
                    (session_id, SessionOutcome::Success),
                    ProtocolType::Resharing,
                );
            }
        }
        None
    }
}

/// Operations only available in LockingInProgress state
impl ChoreographicProtocol<DeviceAgentCore, LockingInProgress> {
    /// Get the current locking session
    pub fn current_locking_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    /// Check lock acquisition progress
    pub async fn check_lock_progress(
        &self,
        session_status: SessionStatus,
        operation_type: aura_journal::OperationType,
    ) -> Option<AgentLockAcquired> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_locking_session() {
                return AgentLockAcquired::verify(
                    (session_id, SessionOutcome::Success),
                    operation_type,
                );
            }
        }
        None
    }
}

/// Operations only available in AgentOperationLocked state
impl ChoreographicProtocol<DeviceAgentCore, AgentOperationLocked> {
    /// Execute operation while holding lock
    pub async fn execute_locked_operation<F, R>(
        &mut self,
        operation: F,
    ) -> Result<R, AgentSessionError>
    where
        F: FnOnce() -> Result<R, AgentSessionError>,
    {
        // Only execute operation if we're in the locked state
        operation().map_err(|e| AgentSessionError::OperationFailed(e.to_string()))
    }

    /// Check if lock is still valid
    pub fn is_lock_valid(&self) -> bool {
        self.inner.current_protocol == Some(ProtocolType::Locking)
    }
}

/// Operations available in failed state
impl ChoreographicProtocol<DeviceAgentCore, AgentFailed> {
    /// Get failure information
    pub fn get_failure_info(&self) -> Option<String> {
        Some("Agent operation failed".to_string())
    }

    /// Attempt to recover from failure (transition back to idle)
    pub fn attempt_recovery(mut self) -> ChoreographicProtocol<DeviceAgentCore, AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.clear();
        self.transition_to()
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed agent
pub fn new_session_typed_agent(
    device_id: DeviceId,
) -> ChoreographicProtocol<DeviceAgentCore, AgentIdle> {
    let core = DeviceAgentCore::new(device_id);
    ChoreographicProtocol::new(core)
}

/// Rehydrate an agent session from protocol state
pub fn rehydrate_agent_session(
    device_id: DeviceId,
    current_protocol: Option<ProtocolType>,
    active_sessions: Vec<SessionId>,
) -> AgentSessionState {
    let mut core = DeviceAgentCore::new(device_id);
    core.current_protocol = current_protocol;
    core.active_sessions = active_sessions;

    match current_protocol {
        Some(ProtocolType::Dkd) => {
            AgentSessionState::DkdInProgress(ChoreographicProtocol::new(core))
        }
        Some(ProtocolType::Recovery) => {
            AgentSessionState::RecoveryInProgress(ChoreographicProtocol::new(core))
        }
        Some(ProtocolType::Resharing) => {
            AgentSessionState::ResharingInProgress(ChoreographicProtocol::new(core))
        }
        Some(ProtocolType::LockAcquisition) => {
            AgentSessionState::LockingInProgress(ChoreographicProtocol::new(core))
        }
        Some(ProtocolType::Locking) => {
            AgentSessionState::AgentOperationLocked(ChoreographicProtocol::new(core))
        }
        None => AgentSessionState::AgentIdle(ChoreographicProtocol::new(core)),
    }
}

// ========== Additional Methods for Union Type ==========

impl AgentSessionState {
    /// Get the current protocol if any
    pub fn current_protocol(&self) -> Option<ProtocolType> {
        match self {
            AgentSessionState::AgentIdle(a) => a.inner.current_protocol,
            AgentSessionState::DkdInProgress(a) => a.inner.current_protocol,
            AgentSessionState::RecoveryInProgress(a) => a.inner.current_protocol,
            AgentSessionState::ResharingInProgress(a) => a.inner.current_protocol,
            AgentSessionState::LockingInProgress(a) => a.inner.current_protocol,
            AgentSessionState::AgentOperationLocked(a) => a.inner.current_protocol,
            AgentSessionState::AgentFailed(a) => a.inner.current_protocol,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::DeviceId;

    #[test]
    fn test_agent_state_transitions() {
        let device_id = DeviceId(Uuid::new_v4());
        let session_id = SessionId(Uuid::new_v4());

        // Create a new agent in idle state
        let agent = new_session_typed_agent(device_id);

        // Should start in AgentIdle state
        assert_eq!(agent.state_name(), "AgentIdle");
        assert!(agent.can_terminate());
        assert!(agent.is_completely_idle());

        // Should be able to check if we can start protocols
        assert!(agent.can_start_protocol(ProtocolType::Dkd).is_ok());

        // Transition to DkdInProgress
        let dkd_agent = agent.begin_dkd(session_id);
        assert_eq!(dkd_agent.state_name(), "DkdInProgress");
        assert_eq!(dkd_agent.current_dkd_session(), Some(session_id));

        // Can transition back with witness
        let witness = DkdCompleted {
            session_id,
            derived_key: vec![1, 2, 3, 4],
            participants: vec![device_id],
        };

        let idle_agent = dkd_agent.transition_with_witness(witness);
        assert_eq!(idle_agent.state_name(), "AgentIdle");
        assert!(idle_agent.is_completely_idle());
    }

    #[test]
    fn test_agent_witness_verification() {
        let session_id = SessionId(Uuid::new_v4());

        // Test DkdCompleted witness
        let witness =
            DkdCompleted::verify((session_id, SessionOutcome::Success), ProtocolType::Dkd);
        assert!(witness.is_some());

        // Should fail for wrong protocol type
        let witness = DkdCompleted::verify(
            (session_id, SessionOutcome::Success),
            ProtocolType::Recovery,
        );
        assert!(witness.is_none());

        // Should fail for failure outcome
        let witness = DkdCompleted::verify((session_id, SessionOutcome::Failed), ProtocolType::Dkd);
        assert!(witness.is_none());
    }

    #[test]
    fn test_agent_rehydration() {
        let device_id = DeviceId(Uuid::new_v4());
        let session_id = SessionId(Uuid::new_v4());

        // Test rehydration without active protocol
        let state = rehydrate_agent_session(device_id, None, vec![]);
        assert_eq!(state.state_name(), "AgentIdle");
        assert!(state.can_terminate());

        // Test rehydration with DKD protocol active
        let state = rehydrate_agent_session(device_id, Some(ProtocolType::Dkd), vec![session_id]);
        assert_eq!(state.state_name(), "DkdInProgress");
        assert_eq!(state.current_protocol(), Some(ProtocolType::Dkd));
        assert!(!state.can_terminate());
    }

    #[test]
    fn test_operation_lock_flow() {
        let device_id = DeviceId(Uuid::new_v4());
        let session_id = SessionId(Uuid::new_v4());

        // Start from idle
        let agent = new_session_typed_agent(device_id);

        // Begin locking
        let locking_agent = agent.begin_locking(session_id);
        assert_eq!(locking_agent.state_name(), "LockingInProgress");

        // Acquire lock
        let witness = AgentLockAcquired {
            session_id,
            operation_type: aura_journal::OperationType::Resharing,
            lock_holder: device_id,
        };

        let locked_agent = <ChoreographicProtocol<DeviceAgentCore, LockingInProgress> as WitnessedTransition<LockingInProgress, AgentOperationLocked>>::transition_with_witness(locking_agent, witness);
        assert_eq!(locked_agent.state_name(), "AgentOperationLocked");
        assert!(locked_agent.is_lock_valid());

        // Release lock
        let idle_agent = locked_agent.release_lock(session_id);
        assert_eq!(idle_agent.state_name(), "AgentIdle");
        assert!(idle_agent.is_completely_idle());
    }
}
