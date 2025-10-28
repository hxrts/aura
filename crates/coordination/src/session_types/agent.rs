//! Session Type States for Agent State Management
//!
//! This module defines session types for DeviceAgent, providing compile-time safety
//! for protocol coordination and state management at the device level.

use crate::session_types::wrapper::SessionTypedProtocol;
use crate::session_types::session_errors::AgentSessionError;
use aura_journal::{OperationType, ProtocolType, SessionId, SessionOutcome, SessionStatus};
use aura_types::DeviceId;
use crate::session_types::witnesses::RuntimeWitness;
use crate::session_types::SessionState;
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
    #[allow(clippy::disallowed_methods)]
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            session_id: Uuid::new_v4(),
            current_protocol: None,
            active_sessions: Vec::new(),
        }
    }
}

// ========== Session States ==========

/// Define session states for the agent
macro_rules! define_agent_states {
    ($($state:ident $(@ $prop:ident)*),*) => {
        $(
            #[derive(Debug, Clone)]
            pub struct $state;

            impl SessionState for $state {
                const NAME: &'static str = stringify!($state);
                $(const $prop: bool = true;)*
            }
        )*
    };
}

define_agent_states! {
    AgentIdle @ CAN_TERMINATE,
    DkdInProgress,
    RecoveryInProgress,
    ResharingInProgress,
    LockingInProgress,
    AgentOperationLocked,
    AgentFailed @ IS_FINAL @ CAN_TERMINATE
}

// ========== Protocol Type Aliases ==========

/// Session-typed device agent wrapper
pub type SessionTypedAgent<S> = SessionTypedProtocol<DeviceAgentCore, S>;

// ========== Union Type ==========

/// Agent session state enum
#[derive(Debug, Clone)]
pub enum AgentSessionState {
    AgentIdle(SessionTypedAgent<AgentIdle>),
    DkdInProgress(SessionTypedAgent<DkdInProgress>),
    RecoveryInProgress(SessionTypedAgent<RecoveryInProgress>),
    ResharingInProgress(SessionTypedAgent<ResharingInProgress>),
    LockingInProgress(SessionTypedAgent<LockingInProgress>),
    AgentOperationLocked(SessionTypedAgent<AgentOperationLocked>),
    AgentFailed(SessionTypedAgent<AgentFailed>),
}

impl AgentSessionState {
    /// Get the current state name
    pub fn state_name(&self) -> &'static str {
        match self {
            AgentSessionState::AgentIdle(_) => AgentIdle::NAME,
            AgentSessionState::DkdInProgress(_) => DkdInProgress::NAME,
            AgentSessionState::RecoveryInProgress(_) => RecoveryInProgress::NAME,
            AgentSessionState::ResharingInProgress(_) => ResharingInProgress::NAME,
            AgentSessionState::LockingInProgress(_) => LockingInProgress::NAME,
            AgentSessionState::AgentOperationLocked(_) => AgentOperationLocked::NAME,
            AgentSessionState::AgentFailed(_) => AgentFailed::NAME,
        }
    }

    /// Check if the session can terminate
    pub fn can_terminate(&self) -> bool {
        match self {
            AgentSessionState::AgentIdle(_) => AgentIdle::CAN_TERMINATE,
            AgentSessionState::DkdInProgress(_) => DkdInProgress::CAN_TERMINATE,
            AgentSessionState::RecoveryInProgress(_) => RecoveryInProgress::CAN_TERMINATE,
            AgentSessionState::ResharingInProgress(_) => ResharingInProgress::CAN_TERMINATE,
            AgentSessionState::LockingInProgress(_) => LockingInProgress::CAN_TERMINATE,
            AgentSessionState::AgentOperationLocked(_) => AgentOperationLocked::CAN_TERMINATE,
            AgentSessionState::AgentFailed(_) => AgentFailed::CAN_TERMINATE,
        }
    }

    /// Get the session ID
    pub fn session_id(&self) -> Uuid {
        match self {
            AgentSessionState::AgentIdle(p) => p.core().session_id,
            AgentSessionState::DkdInProgress(p) => p.core().session_id,
            AgentSessionState::RecoveryInProgress(p) => p.core().session_id,
            AgentSessionState::ResharingInProgress(p) => p.core().session_id,
            AgentSessionState::LockingInProgress(p) => p.core().session_id,
            AgentSessionState::AgentOperationLocked(p) => p.core().session_id,
            AgentSessionState::AgentFailed(p) => p.core().session_id,
        }
    }

    /// Get the device ID
    pub fn device_id(&self) -> Uuid {
        match self {
            AgentSessionState::AgentIdle(p) => p.core().device_id.0,
            AgentSessionState::DkdInProgress(p) => p.core().device_id.0,
            AgentSessionState::RecoveryInProgress(p) => p.core().device_id.0,
            AgentSessionState::ResharingInProgress(p) => p.core().device_id.0,
            AgentSessionState::LockingInProgress(p) => p.core().device_id.0,
            AgentSessionState::AgentOperationLocked(p) => p.core().device_id.0,
            AgentSessionState::AgentFailed(p) => p.core().device_id.0,
        }
    }

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

    /// Check if completely idle
    pub fn is_completely_idle(&self) -> bool {
        match self {
            AgentSessionState::AgentIdle(agent) => agent.is_completely_idle(),
            _ => false,
        }
    }

    /// Release lock for locked operations
    pub fn release_lock(self, session_id: SessionId) -> AgentSessionState {
        match self {
            AgentSessionState::AgentOperationLocked(mut locked) => {
                locked.inner.current_protocol = None;
                locked.inner.active_sessions.retain(|&id| id != session_id);
                AgentSessionState::AgentIdle(locked.transition_to())
            }
            _ => self, // No-op for other states
        }
    }
}

// ========== Runtime Witnesses ==========

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
                derived_key: vec![],
                participants: vec![],
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
    pub new_configuration: Option<(usize, usize)>,
}

impl RuntimeWitness for RecoveryCompleted {
    type Evidence = (SessionId, SessionOutcome);
    type Config = ProtocolType;

    fn verify(evidence: (SessionId, SessionOutcome), config: ProtocolType) -> Option<Self> {
        let (session_id, outcome) = evidence;
        if config == ProtocolType::Recovery && outcome == SessionOutcome::Success {
            Some(RecoveryCompleted {
                session_id,
                recovered_shares: 0,
                new_configuration: None,
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
                new_participants: vec![],
                new_threshold: 0,
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
    pub operation_type: OperationType,
    pub lock_holder: DeviceId,
}

impl RuntimeWitness for AgentLockAcquired {
    type Evidence = (SessionId, SessionOutcome);
    type Config = OperationType;

    #[allow(clippy::disallowed_methods)]
    fn verify(evidence: (SessionId, SessionOutcome), config: OperationType) -> Option<Self> {
        let (session_id, outcome) = evidence;
        if outcome == SessionOutcome::Success {
            Some(AgentLockAcquired {
                session_id,
                operation_type: config,
                lock_holder: DeviceId(uuid::Uuid::new_v4()),
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

/// Trait for AgentIdle state operations
pub trait AgentIdleOperations {
    /// Start DKD protocol
    fn begin_dkd(self, protocol_session_id: SessionId) -> SessionTypedAgent<DkdInProgress>;
    /// Start recovery protocol
    fn begin_recovery(
        self,
        protocol_session_id: SessionId,
    ) -> SessionTypedAgent<RecoveryInProgress>;
    /// Start resharing protocol
    fn begin_resharing(
        self,
        protocol_session_id: SessionId,
    ) -> SessionTypedAgent<ResharingInProgress>;
    /// Start locking protocol
    fn begin_locking(self, protocol_session_id: SessionId) -> SessionTypedAgent<LockingInProgress>;
    /// Check if agent can start a new protocol
    fn can_start_protocol(&self, protocol_type: ProtocolType) -> Result<(), AgentSessionError>;
    /// Get current active sessions
    fn active_sessions(&self) -> &[SessionId];
    /// Check if agent is truly idle
    fn is_completely_idle(&self) -> bool;
}

impl AgentIdleOperations for SessionTypedAgent<AgentIdle> {
    fn begin_dkd(mut self, protocol_session_id: SessionId) -> SessionTypedAgent<DkdInProgress> {
        self.inner.current_protocol = Some(ProtocolType::Dkd);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }

    fn begin_recovery(
        mut self,
        protocol_session_id: SessionId,
    ) -> SessionTypedAgent<RecoveryInProgress> {
        self.inner.current_protocol = Some(ProtocolType::Recovery);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }

    fn begin_resharing(
        mut self,
        protocol_session_id: SessionId,
    ) -> SessionTypedAgent<ResharingInProgress> {
        self.inner.current_protocol = Some(ProtocolType::Resharing);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }

    fn begin_locking(
        mut self,
        protocol_session_id: SessionId,
    ) -> SessionTypedAgent<LockingInProgress> {
        self.inner.current_protocol = Some(ProtocolType::LockAcquisition);
        self.inner.active_sessions.push(protocol_session_id);
        self.transition_to()
    }

    fn can_start_protocol(&self, protocol_type: ProtocolType) -> Result<(), AgentSessionError> {
        if self.inner.current_protocol.is_some() {
            return Err(AgentSessionError::ConcurrentProtocol(protocol_type));
        }
        Ok(())
    }

    fn active_sessions(&self) -> &[SessionId] {
        &self.inner.active_sessions
    }

    fn is_completely_idle(&self) -> bool {
        self.inner.active_sessions.is_empty() && self.inner.current_protocol.is_none()
    }
}

// ========== Protocol-specific state transitions ==========

impl SessionTypedAgent<DkdInProgress> {
    /// Complete DKD protocol successfully
    pub fn complete_dkd(mut self, witness: DkdCompleted) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner
            .active_sessions
            .retain(|&id| id != witness.session_id);
        self.transition_to()
    }

    /// Abort DKD protocol
    pub fn abort_dkd(mut self, _reason: String) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.clear();
        self.transition_to()
    }
}

impl SessionTypedAgent<RecoveryInProgress> {
    /// Complete recovery protocol successfully
    pub fn complete_recovery(mut self, witness: RecoveryCompleted) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner
            .active_sessions
            .retain(|&id| id != witness.session_id);
        self.transition_to()
    }

    /// Abort recovery protocol
    pub fn abort_recovery(mut self, _reason: String) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.clear();
        self.transition_to()
    }
}

impl SessionTypedAgent<ResharingInProgress> {
    /// Complete resharing protocol successfully
    pub fn complete_resharing(
        mut self,
        witness: AgentResharingCompleted,
    ) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner
            .active_sessions
            .retain(|&id| id != witness.session_id);
        self.transition_to()
    }

    /// Abort resharing protocol
    pub fn abort_resharing(mut self, _reason: String) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.clear();
        self.transition_to()
    }
}

impl SessionTypedAgent<LockingInProgress> {
    /// Successfully acquire operation lock
    pub fn acquire_lock(
        mut self,
        _witness: AgentLockAcquired,
    ) -> SessionTypedAgent<AgentOperationLocked> {
        self.inner.current_protocol = Some(ProtocolType::Locking);
        self.transition_to()
    }

    /// Fail to acquire lock
    pub fn fail_lock(mut self, _reason: String) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.clear();
        self.transition_to()
    }
}

impl SessionTypedAgent<AgentOperationLocked> {
    /// Release the operation lock
    pub fn release_lock(mut self, session_id: SessionId) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.retain(|&id| id != session_id);
        self.transition_to()
    }
}

impl SessionTypedAgent<AgentFailed> {
    /// Attempt to recover from failure
    pub fn attempt_recovery(mut self) -> SessionTypedAgent<AgentIdle> {
        self.inner.current_protocol = None;
        self.inner.active_sessions.clear();
        self.transition_to()
    }
}

// ========== Agent-Specific Operations ==========

/// Trait for DKD operations
pub trait AgentDkdOperations {
    /// Get the current DKD session
    fn current_dkd_session(&self) -> Option<SessionId>;
    /// Check DKD progress
    fn check_dkd_progress(
        &self,
        session_status: SessionStatus,
    ) -> impl std::future::Future<Output = Option<DkdCompleted>> + Send;
}

impl AgentDkdOperations for SessionTypedAgent<DkdInProgress> {
    fn current_dkd_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    async fn check_dkd_progress(&self, session_status: SessionStatus) -> Option<DkdCompleted> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_dkd_session() {
                return RuntimeWitness::verify(
                    (session_id, SessionOutcome::Success),
                    ProtocolType::Dkd,
                );
            }
        }
        None
    }
}

/// Trait for recovery operations
pub trait AgentRecoveryOperations {
    /// Get the current recovery session
    fn current_recovery_session(&self) -> Option<SessionId>;
    /// Check recovery progress
    fn check_recovery_progress(
        &self,
        session_status: SessionStatus,
    ) -> impl std::future::Future<Output = Option<RecoveryCompleted>> + Send;
}

impl AgentRecoveryOperations for SessionTypedAgent<RecoveryInProgress> {
    fn current_recovery_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    async fn check_recovery_progress(
        &self,
        session_status: SessionStatus,
    ) -> Option<RecoveryCompleted> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_recovery_session() {
                return RuntimeWitness::verify(
                    (session_id, SessionOutcome::Success),
                    ProtocolType::Recovery,
                );
            }
        }
        None
    }
}

/// Trait for resharing operations
pub trait AgentResharingOperations {
    /// Get the current resharing session
    fn current_resharing_session(&self) -> Option<SessionId>;
    /// Check resharing progress
    fn check_resharing_progress(
        &self,
        session_status: SessionStatus,
    ) -> impl std::future::Future<Output = Option<AgentResharingCompleted>> + Send;
}

impl AgentResharingOperations for SessionTypedAgent<ResharingInProgress> {
    fn current_resharing_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    async fn check_resharing_progress(
        &self,
        session_status: SessionStatus,
    ) -> Option<AgentResharingCompleted> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_resharing_session() {
                return RuntimeWitness::verify(
                    (session_id, SessionOutcome::Success),
                    ProtocolType::Resharing,
                );
            }
        }
        None
    }
}

/// Trait for locking operations
pub trait AgentLockingOperations {
    /// Get the current locking session
    fn current_locking_session(&self) -> Option<SessionId>;
    /// Check lock acquisition progress
    fn check_lock_progress(
        &self,
        session_status: SessionStatus,
        operation_type: OperationType,
    ) -> impl std::future::Future<Output = Option<AgentLockAcquired>> + Send;
}

impl AgentLockingOperations for SessionTypedAgent<LockingInProgress> {
    fn current_locking_session(&self) -> Option<SessionId> {
        self.inner.active_sessions.last().copied()
    }

    async fn check_lock_progress(
        &self,
        session_status: SessionStatus,
        operation_type: OperationType,
    ) -> Option<AgentLockAcquired> {
        if session_status == SessionStatus::Completed {
            if let Some(session_id) = self.current_locking_session() {
                return RuntimeWitness::verify(
                    (session_id, SessionOutcome::Success),
                    operation_type,
                );
            }
        }
        None
    }
}

/// Trait for locked operation management
pub trait AgentLockedOperations {
    /// Execute operation while holding lock
    fn execute_locked_operation<F, R>(
        &mut self,
        operation: F,
    ) -> impl std::future::Future<Output = Result<R, AgentSessionError>> + Send
    where
        F: FnOnce() -> Result<R, AgentSessionError> + Send;
    /// Check if lock is still valid
    fn is_lock_valid(&self) -> bool;
}

impl AgentLockedOperations for SessionTypedAgent<AgentOperationLocked> {
    async fn execute_locked_operation<F, R>(&mut self, operation: F) -> Result<R, AgentSessionError>
    where
        F: FnOnce() -> Result<R, AgentSessionError> + Send,
    {
        operation().map_err(|e| AgentSessionError::OperationFailed(e.to_string()))
    }

    fn is_lock_valid(&self) -> bool {
        self.inner.current_protocol == Some(ProtocolType::Locking)
    }
}

/// Trait for failed state operations
pub trait AgentFailedOperations {
    /// Get failure information
    fn get_failure_info(&self) -> Option<String>;
    /// Attempt to recover from failure
    fn attempt_recovery(self) -> AgentSessionState;
}

impl AgentFailedOperations for SessionTypedAgent<AgentFailed> {
    fn get_failure_info(&self) -> Option<String> {
        Some("Agent operation failed".to_string())
    }

    fn attempt_recovery(self) -> AgentSessionState {
        AgentSessionState::AgentIdle(self.attempt_recovery())
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed agent
pub fn new_session_typed_agent(device_id: DeviceId) -> SessionTypedAgent<AgentIdle> {
    let core = DeviceAgentCore::new(device_id);
    SessionTypedProtocol::new(core)
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
            AgentSessionState::DkdInProgress(SessionTypedProtocol::new(core))
        }
        Some(ProtocolType::Recovery) => {
            AgentSessionState::RecoveryInProgress(SessionTypedProtocol::new(core))
        }
        Some(ProtocolType::Resharing) => {
            AgentSessionState::ResharingInProgress(SessionTypedProtocol::new(core))
        }
        Some(ProtocolType::LockAcquisition) => {
            AgentSessionState::LockingInProgress(SessionTypedProtocol::new(core))
        }
        Some(ProtocolType::Locking) => {
            AgentSessionState::AgentOperationLocked(SessionTypedProtocol::new(core))
        }
        Some(ProtocolType::Counter) => {
            AgentSessionState::AgentIdle(SessionTypedProtocol::new(core))
        }
        None => AgentSessionState::AgentIdle(SessionTypedProtocol::new(core)),
    }
}

#[allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use DeviceId;

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_agent_state_transitions() {
        let device_id = DeviceId(Uuid::new_v4());
        let session_id = SessionId(Uuid::new_v4());

        // Create a new agent in idle state
        let agent = new_session_typed_agent(device_id);

        // Should start in AgentIdle state
        assert!(agent.is_completely_idle());

        // Should be able to check if we can start protocols
        assert!(agent.can_start_protocol(ProtocolType::Dkd).is_ok());

        // Transition to DkdInProgress
        let dkd_agent = agent.begin_dkd(session_id);
        assert_eq!(dkd_agent.current_dkd_session(), Some(session_id));

        // Complete DKD with witness
        let witness = DkdCompleted {
            session_id,
            derived_key: vec![1, 2, 3, 4],
            participants: vec![device_id],
        };

        let idle_agent = dkd_agent.complete_dkd(witness);
        assert!(idle_agent.is_completely_idle());
    }

    #[allow(clippy::disallowed_methods)]
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

    #[allow(clippy::disallowed_methods)]
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

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_operation_lock_flow() {
        let device_id = DeviceId(Uuid::new_v4());
        let session_id = SessionId(Uuid::new_v4());

        // Start from idle
        let agent = new_session_typed_agent(device_id);

        // Begin locking
        let locking_agent = agent.begin_locking(session_id);
        assert_eq!(locking_agent.current_locking_session(), Some(session_id));

        // Acquire lock
        let witness = AgentLockAcquired {
            session_id,
            operation_type: OperationType::Resharing,
            lock_holder: device_id,
        };

        let locked_agent = locking_agent.acquire_lock(witness);
        assert!(locked_agent.is_lock_valid());

        // Release lock
        let idle_agent = locked_agent.release_lock(session_id);
        assert!(idle_agent.is_completely_idle());
    }
}
