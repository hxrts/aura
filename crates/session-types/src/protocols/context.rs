//! Session Type States for Protocol Context
//!
//! This module defines session types for the ProtocolContext execution environment,
//! providing compile-time safety for protocol execution phases and instruction flows.

use crate::{SessionState, ChoreographicProtocol, SessionProtocol, WitnessedTransition, RuntimeWitness};
use aura_journal::{Event, ProtocolType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// Protocol execution types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction {
    WriteToLedger(aura_journal::Event),
    AwaitEvent { event_type: String, timeout: u64 },
    AwaitThreshold { threshold: u16, timeout: u64 },
    RunSubProtocol { protocol_type: String, parameters: BTreeMap<String, String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstructionResult {
    EventWritten { event_id: Uuid },
    EventReceived { event: aura_journal::Event },
    ThresholdMet { count: u16 },
    SubProtocolComplete { result: String },
    Error { message: String },
}

#[derive(Debug, Clone)]
pub struct ProtocolContext {
    pub context_id: Uuid,
    pub session_id: Uuid,
    pub device_id: aura_journal::DeviceId,
    pub protocol_type: ProtocolType,
}

impl ProtocolContext {
    pub async fn execute(&mut self, instruction: Instruction) -> Result<InstructionResult, ProtocolError> {
        // Placeholder implementation - this will be properly implemented
        // when the session types are integrated with the actual execution runtime
        match instruction {
            Instruction::WriteToLedger(_event) => {
                Ok(InstructionResult::EventWritten { event_id: Uuid::new_v4() })
            },
            Instruction::AwaitEvent { .. } => {
                Ok(InstructionResult::Error { message: "Not implemented".to_string() })
            },
            Instruction::AwaitThreshold { .. } => {
                Ok(InstructionResult::ThresholdMet { count: 1 })
            },
            Instruction::RunSubProtocol { .. } => {
                Ok(InstructionResult::SubProtocolComplete { result: "Success".to_string() })
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Invalid instruction: {0}")]
    InvalidInstruction(String),
}

// ========== Protocol Context Session States ==========

/// Initial state when protocol context is created
#[derive(Debug, Clone)]
pub struct ContextInitialized;

impl SessionState for ContextInitialized {
    const NAME: &'static str = "ContextInitialized";
}

/// State when context is actively executing instructions
#[derive(Debug, Clone)]
pub struct ExecutingInstructions;

impl SessionState for ExecutingInstructions {
    const NAME: &'static str = "ExecutingInstructions";
}

/// State when context is waiting for events or conditions
#[derive(Debug, Clone)]
pub struct AwaitingCondition;

impl SessionState for AwaitingCondition {
    const NAME: &'static str = "AwaitingCondition";
}

/// State when context is writing to ledger
#[derive(Debug, Clone)]
pub struct WritingToLedger;

impl SessionState for WritingToLedger {
    const NAME: &'static str = "WritingToLedger";
}

/// State when context is executing a sub-protocol
#[derive(Debug, Clone)]
pub struct ExecutingSubProtocol;

impl SessionState for ExecutingSubProtocol {
    const NAME: &'static str = "ExecutingSubProtocol";
}

/// State when protocol execution is complete
#[derive(Debug, Clone)]
pub struct ExecutionComplete;

impl SessionState for ExecutionComplete {
    const NAME: &'static str = "ExecutionComplete";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

/// State when protocol execution has failed
#[derive(Debug, Clone)]
pub struct ExecutionFailed;

impl SessionState for ExecutionFailed {
    const NAME: &'static str = "ExecutionFailed";
    const CAN_TERMINATE: bool = true;
    const IS_FINAL: bool = true;
}

// ========== Context Protocol Wrapper ==========

/// Session-typed protocol context wrapper
pub type SessionTypedContext<S> = ChoreographicProtocol<ProtocolContext, S>;

// ========== Runtime Witnesses for Context Operations ==========

/// Witness that sufficient events have been collected for threshold operations
#[derive(Debug, Clone)]
pub struct ThresholdEventsMet {
    pub collected_count: usize,
    pub required_count: usize,
    pub matching_events: Vec<Event>,
}

impl RuntimeWitness for ThresholdEventsMet {
    type Evidence = (Vec<Event>, usize);
    type Config = ();
    
    fn verify(evidence: (Vec<Event>, usize), _config: ()) -> Option<Self> {
        let (events, required_count) = evidence;
        if events.len() >= required_count {
            Some(ThresholdEventsMet {
                collected_count: events.len(),
                required_count,
                matching_events: events,
            })
        } else {
            None
        }
    }
    
    fn description(&self) -> &'static str {
        "Threshold events collected"
    }
}

/// Witness that ledger write has been successful
#[derive(Debug, Clone)]
pub struct LedgerWriteComplete {
    pub event_written: bool,
    pub new_epoch: u64,
}

impl RuntimeWitness for LedgerWriteComplete {
    type Evidence = InstructionResult;
    type Config = ();
    
    fn verify(evidence: InstructionResult, _config: ()) -> Option<Self> {
        match evidence {
            InstructionResult::EventWritten { .. } => Some(LedgerWriteComplete {
                event_written: true,
                new_epoch: 0, // Would be extracted from actual result
            }),
            _ => None,
        }
    }
    
    fn description(&self) -> &'static str {
        "Ledger write completed"
    }
}

/// Witness that sub-protocol execution has completed
#[derive(Debug, Clone)]
pub struct SubProtocolComplete {
    pub protocol_result: String,
    pub success: bool,
}

impl RuntimeWitness for SubProtocolComplete {
    type Evidence = InstructionResult;
    type Config = ();
    
    fn verify(evidence: InstructionResult, _config: ()) -> Option<Self> {
        match evidence {
            InstructionResult::SubProtocolComplete { result } => Some(SubProtocolComplete {
                protocol_result: result,
                success: true,
            }),
            _ => None,
        }
    }
    
    fn description(&self) -> &'static str {
        "Sub-protocol completed"
    }
}

// ========== Concrete Error Type ==========

#[derive(Debug, thiserror::Error)]
pub enum ContextSessionError {
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid instruction for current state")]
    InvalidInstruction,
    #[error("Context execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Timeout occurred")]
    Timeout,
}

impl From<ProtocolError> for ContextSessionError {
    fn from(err: ProtocolError) -> Self {
        ContextSessionError::ProtocolError(err.to_string())
    }
}

// ========== SessionProtocol Implementations ==========

impl SessionProtocol for ChoreographicProtocol<ProtocolContext, ContextInitialized> {
    type State = ContextInitialized;
    type Output = InstructionResult;
    type Error = ContextSessionError;
    
    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.context_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<ProtocolContext, ExecutingInstructions> {
    type State = ExecutingInstructions;
    type Output = InstructionResult;
    type Error = ContextSessionError;
    
    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.context_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<ProtocolContext, AwaitingCondition> {
    type State = AwaitingCondition;
    type Output = InstructionResult;
    type Error = ContextSessionError;
    
    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.context_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<ProtocolContext, WritingToLedger> {
    type State = WritingToLedger;
    type Output = InstructionResult;
    type Error = ContextSessionError;
    
    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.context_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<ProtocolContext, ExecutingSubProtocol> {
    type State = ExecutingSubProtocol;
    type Output = InstructionResult;
    type Error = ContextSessionError;
    
    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.context_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<ProtocolContext, ExecutionComplete> {
    type State = ExecutionComplete;
    type Output = InstructionResult;
    type Error = ContextSessionError;
    
    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.context_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

impl SessionProtocol for ChoreographicProtocol<ProtocolContext, ExecutionFailed> {
    type State = ExecutionFailed;
    type Output = InstructionResult;
    type Error = ContextSessionError;
    
    fn session_id(&self) -> Uuid {
        self.inner.session_id
    }

    fn state_name(&self) -> &'static str {
        Self::State::NAME
    }

    fn can_terminate(&self) -> bool {
        Self::State::CAN_TERMINATE
    }

    fn protocol_id(&self) -> Uuid {
        self.inner.context_id
    }

    fn device_id(&self) -> aura_journal::DeviceId {
        self.inner.device_id
    }
}

// ========== State Transitions ==========

/// Transition from ContextInitialized to ExecutingInstructions (no witness needed)
impl ChoreographicProtocol<ProtocolContext, ContextInitialized> {
    /// Begin executing instructions
    pub fn begin_execution(self) -> ChoreographicProtocol<ProtocolContext, ExecutingInstructions> {
        self.transition_to()
    }
}

/// Transition from ExecutingInstructions to various states based on instruction type
impl ChoreographicProtocol<ProtocolContext, ExecutingInstructions> {
    /// Transition to awaiting condition state
    pub fn await_condition(self) -> ChoreographicProtocol<ProtocolContext, AwaitingCondition> {
        self.transition_to()
    }
    
    /// Transition to writing to ledger state
    pub fn write_to_ledger(self) -> ChoreographicProtocol<ProtocolContext, WritingToLedger> {
        self.transition_to()
    }
    
    /// Transition to executing sub-protocol state
    pub fn execute_sub_protocol(self) -> ChoreographicProtocol<ProtocolContext, ExecutingSubProtocol> {
        self.transition_to()
    }
    
    /// Complete execution successfully
    pub fn complete_execution(self) -> ChoreographicProtocol<ProtocolContext, ExecutionComplete> {
        self.transition_to()
    }
}

/// Transition from AwaitingCondition back to ExecutingInstructions (requires ThresholdEventsMet witness for threshold operations)
impl WitnessedTransition<AwaitingCondition, ExecutingInstructions> 
    for ChoreographicProtocol<ProtocolContext, AwaitingCondition> 
{
    type Witness = ThresholdEventsMet;
    type Target = ChoreographicProtocol<ProtocolContext, ExecutingInstructions>;
    
    /// Resume execution after threshold condition is met
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Simple transition from AwaitingCondition to ExecutingInstructions (for non-threshold operations)
impl ChoreographicProtocol<ProtocolContext, AwaitingCondition> {
    /// Resume execution after simple condition is met
    pub fn resume_execution(self) -> ChoreographicProtocol<ProtocolContext, ExecutingInstructions> {
        self.transition_to()
    }
}

/// Transition from WritingToLedger back to ExecutingInstructions (requires LedgerWriteComplete witness)
impl WitnessedTransition<WritingToLedger, ExecutingInstructions>
    for ChoreographicProtocol<ProtocolContext, WritingToLedger>
{
    type Witness = LedgerWriteComplete;
    type Target = ChoreographicProtocol<ProtocolContext, ExecutingInstructions>;
    
    /// Resume execution after ledger write is complete
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from ExecutingSubProtocol back to ExecutingInstructions (requires SubProtocolComplete witness)
impl WitnessedTransition<ExecutingSubProtocol, ExecutingInstructions> 
    for ChoreographicProtocol<ProtocolContext, ExecutingSubProtocol> 
{
    type Witness = SubProtocolComplete;
    type Target = ChoreographicProtocol<ProtocolContext, ExecutingInstructions>;
    
    /// Resume execution after sub-protocol completes
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        self.transition_to()
    }
}

/// Transition to ExecutionFailed from any state (no witness needed)
impl<S: SessionState> ChoreographicProtocol<ProtocolContext, S> {
    /// Fail the execution (can be called from any state)
    pub fn fail_execution(self) -> ChoreographicProtocol<ProtocolContext, ExecutionFailed> {
        self.transition_to()
    }
}

// ========== Context-Specific Operations ==========

/// Operations only available in ExecutingInstructions state
impl ChoreographicProtocol<ProtocolContext, ExecutingInstructions> {
    /// Execute an instruction that doesn't require state transition tracking
    pub async fn execute_simple_instruction(&mut self, instruction: Instruction) -> Result<InstructionResult, ContextSessionError> {
        self.inner.execute(instruction).await.map_err(Into::into)
    }
    
    /// Check if the next instruction requires a state transition
    pub fn requires_state_transition(&self, instruction: &Instruction) -> bool {
        matches!(instruction, 
            Instruction::WriteToLedger(_) |
            Instruction::AwaitEvent { .. } |
            Instruction::AwaitThreshold { .. } |
            Instruction::RunSubProtocol { .. }
        )
    }
}

/// Operations only available in AwaitingCondition state
impl ChoreographicProtocol<ProtocolContext, AwaitingCondition> {
    /// Check if threshold condition has been met
    pub async fn check_threshold_condition(
        &mut self, 
        required_count: usize
    ) -> Option<ThresholdEventsMet> {
        // This would check the context's pending events
        // For now, return a placeholder
        let events = vec![]; // Would get from context
        ThresholdEventsMet::verify((events, required_count), ())
    }
}

/// Operations only available in WritingToLedger state
impl ChoreographicProtocol<ProtocolContext, WritingToLedger> {
    /// Execute the ledger write and check completion
    pub async fn execute_ledger_write(
        &mut self, 
        event: aura_journal::Event
    ) -> Result<LedgerWriteComplete, ContextSessionError> {
        let result = self.inner.execute(Instruction::WriteToLedger(event)).await?;
        LedgerWriteComplete::verify(result, ()).ok_or(
            ContextSessionError::ExecutionFailed("Ledger write failed".to_string())
        )
    }
}

/// Operations only available in ExecutingSubProtocol state
impl ChoreographicProtocol<ProtocolContext, ExecutingSubProtocol> {
    /// Execute sub-protocol and check completion
    pub async fn execute_sub_protocol_instruction(
        &mut self, 
        protocol_type: ProtocolType,
        context_id: Uuid
    ) -> Result<SubProtocolComplete, ContextSessionError> {
        let mut params = BTreeMap::new();
        params.insert("protocol_type".to_string(), format!("{:?}", protocol_type));
        params.insert("context_id".to_string(), context_id.to_string());
        let instruction = Instruction::RunSubProtocol {
            protocol_type: format!("{:?}", protocol_type),
            parameters: params,
        };
        let result = self.inner.execute(instruction).await?;
        SubProtocolComplete::verify(result, ()).ok_or(
            ContextSessionError::ExecutionFailed("Sub-protocol execution failed".to_string())
        )
    }
}

/// Operations available in final states
impl ChoreographicProtocol<ProtocolContext, ExecutionComplete> {
    /// Get the final execution result
    pub fn get_execution_result(&self) -> Option<String> {
        Some("Execution completed successfully".to_string())
    }
}

impl ChoreographicProtocol<ProtocolContext, ExecutionFailed> {
    /// Get the failure reason
    pub fn get_failure_reason(&self) -> Option<String> {
        Some("Execution failed".to_string())
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed protocol context
pub fn new_session_typed_context(
    context: ProtocolContext
) -> ChoreographicProtocol<ProtocolContext, ContextInitialized> {
    ChoreographicProtocol::new(context)
}

/// Rehydrate a protocol context session from execution state
pub fn rehydrate_context_session(
    context: ProtocolContext,
    last_instruction: Option<&Instruction>
) -> ContextSessionState {
    match last_instruction {
        Some(Instruction::WriteToLedger(_)) => ContextSessionState::WritingToLedger(
            ChoreographicProtocol::new(context)
        ),
        Some(Instruction::AwaitEvent { .. }) | Some(Instruction::AwaitThreshold { .. }) => {
            ContextSessionState::AwaitingCondition(ChoreographicProtocol::new(context))
        },
        Some(Instruction::RunSubProtocol { .. }) => ContextSessionState::ExecutingSubProtocol(
            ChoreographicProtocol::new(context)
        ),
        _ => ContextSessionState::ExecutingInstructions(ChoreographicProtocol::new(context)),
    }
}

/// Enum representing the possible states of a protocol context session
pub enum ContextSessionState {
    ContextInitialized(ChoreographicProtocol<ProtocolContext, ContextInitialized>),
    ExecutingInstructions(ChoreographicProtocol<ProtocolContext, ExecutingInstructions>),
    AwaitingCondition(ChoreographicProtocol<ProtocolContext, AwaitingCondition>),
    WritingToLedger(ChoreographicProtocol<ProtocolContext, WritingToLedger>),
    ExecutingSubProtocol(ChoreographicProtocol<ProtocolContext, ExecutingSubProtocol>),
    ExecutionComplete(ChoreographicProtocol<ProtocolContext, ExecutionComplete>),
    ExecutionFailed(ChoreographicProtocol<ProtocolContext, ExecutionFailed>),
}

impl ContextSessionState {
    /// Get the current state name
    pub fn state_name(&self) -> &'static str {
        match self {
            ContextSessionState::ContextInitialized(p) => p.current_state_name(),
            ContextSessionState::ExecutingInstructions(p) => p.current_state_name(),
            ContextSessionState::AwaitingCondition(p) => p.current_state_name(),
            ContextSessionState::WritingToLedger(p) => p.current_state_name(),
            ContextSessionState::ExecutingSubProtocol(p) => p.current_state_name(),
            ContextSessionState::ExecutionComplete(p) => p.current_state_name(),
            ContextSessionState::ExecutionFailed(p) => p.current_state_name(),
        }
    }
    
    /// Check if the context can be terminated
    pub fn can_terminate(&self) -> bool {
        match self {
            ContextSessionState::ContextInitialized(p) => p.can_terminate(),
            ContextSessionState::ExecutingInstructions(p) => p.can_terminate(),
            ContextSessionState::AwaitingCondition(p) => p.can_terminate(),
            ContextSessionState::WritingToLedger(p) => p.can_terminate(),
            ContextSessionState::ExecutingSubProtocol(p) => p.can_terminate(),
            ContextSessionState::ExecutionComplete(p) => p.can_terminate(),
            ContextSessionState::ExecutionFailed(p) => p.can_terminate(),
        }
    }
    
    /// Check if the context is in a final state
    pub fn is_final(&self) -> bool {
        match self {
            ContextSessionState::ContextInitialized(p) => p.is_final(),
            ContextSessionState::ExecutingInstructions(p) => p.is_final(),
            ContextSessionState::AwaitingCondition(p) => p.is_final(),
            ContextSessionState::WritingToLedger(p) => p.is_final(),
            ContextSessionState::ExecutingSubProtocol(p) => p.is_final(),
            ContextSessionState::ExecutionComplete(p) => p.is_final(),
            ContextSessionState::ExecutionFailed(p) => p.is_final(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_journal::{AccountLedger, AccountState, DeviceId};
    use aura_transport::StubTransport;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    
    #[test]
    fn test_context_state_transitions() {
        // Create a minimal context for testing
        let effects = Effects::test();
        let session_id = effects.gen_uuid();
        let device_id = effects.gen_uuid();
        
        let device_metadata = aura_journal::DeviceMetadata {
            device_id: DeviceId(device_id),
            device_name: "test-device".to_string(),
            device_type: aura_journal::DeviceType::Native,
            public_key: ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };

        let state = AccountState::new(
            aura_journal::AccountId(effects.gen_uuid()),
            ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2,
            3,
        );

        let ledger = Arc::new(RwLock::new(AccountLedger::new(state).unwrap()));
        let device_key = ed25519_dalek::SigningKey::from_bytes(&[0u8; 32]);

        let context = ProtocolContext::new(
            session_id,
            device_id,
            vec![],
            Some(2),
            ledger,
            Arc::new(StubTransport::default()),
            Effects::test(),
            device_key,
            Box::new(crate::ProductionTimeSource::new()),
        );
        
        // Test basic state transitions
        let session_context = new_session_typed_context(context);
        
        // Should start in ContextInitialized state
        assert_eq!(session_context.current_state_name(), "ContextInitialized");
        assert!(!session_context.can_terminate());
        assert!(!session_context.is_final());
        
        // Transition to ExecutingInstructions
        let executing_context = session_context.begin_execution();
        assert_eq!(executing_context.current_state_name(), "ExecutingInstructions");
        
        // Can fail from any state
        let failed_context = executing_context.fail_execution();
        assert_eq!(failed_context.current_state_name(), "ExecutionFailed");
        assert!(failed_context.can_terminate());
        assert!(failed_context.is_final());
    }
    
    #[test]
    fn test_context_witness_verification() {
        // Test ThresholdEventsMet witness
        let events = vec![];
        let witness = ThresholdEventsMet::verify((events, 2), ());
        assert!(witness.is_none()); // Not enough events
        
        // Test with sufficient events
        let events = vec![
            // Would create real events here, using placeholder for test
        ];
        let witness = ThresholdEventsMet::verify((events, 0), ());
        assert!(witness.is_some());
    }
    
    #[test]
    fn test_context_rehydration() {
        let effects = Effects::test();
        let session_id = effects.gen_uuid();
        let device_id = effects.gen_uuid();
        
        let device_metadata = aura_journal::DeviceMetadata {
            device_id: DeviceId(device_id),
            device_name: "test-device".to_string(),
            device_type: aura_journal::DeviceType::Native,
            public_key: ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
        };

        let state = AccountState::new(
            aura_journal::AccountId(effects.gen_uuid()),
            ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2,
            3,
        );

        let ledger = Arc::new(RwLock::new(AccountLedger::new(state).unwrap()));
        let device_key = ed25519_dalek::SigningKey::from_bytes(&[0u8; 32]);

        let context = ProtocolContext::new(
            session_id,
            device_id,
            vec![],
            Some(2),
            ledger,
            Arc::new(StubTransport::default()),
            Effects::test(),
            device_key,
            Box::new(crate::ProductionTimeSource::new()),
        );
        
        // Test rehydration without last instruction
        let state = rehydrate_context_session(context, None);
        assert_eq!(state.state_name(), "ExecutingInstructions");
        assert!(!state.can_terminate());
        assert!(!state.is_final());
    }
}