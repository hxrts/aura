//! Session Type States for Protocol Context (Refactored with Macros)
//!
//! This module defines session types for the ProtocolContext execution environment,
//! providing compile-time safety for protocol execution phases and instruction flows.

use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState, WitnessedTransition};
use crate::define_protocol;
use crate::witnesses::RuntimeWitness;
use aura_journal::{DeviceId, Event, ProtocolType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

// ========== Protocol execution types ==========

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

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Invalid instruction: {0}")]
    InvalidInstruction(String),
}

// ========== Core Protocol Structure ==========

/// Core data structure for protocol context execution
#[derive(Debug, Clone)]
pub struct ProtocolContextCore {
    pub context_id: Uuid,
    pub session_id: Uuid,
    pub device_id: DeviceId,
    pub protocol_type: ProtocolType,
}

impl ProtocolContextCore {
    pub fn new(context_id: Uuid, session_id: Uuid, device_id: DeviceId, protocol_type: ProtocolType) -> Self {
        Self {
            context_id,
            session_id,
            device_id,
            protocol_type,
        }
    }

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

// ========== Error Type ==========

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

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: ContextProtocol,
    Core: ProtocolContextCore,
    Error: ContextSessionError,
    Union: ContextSessionState,

    States {
        ContextInitialized => (),
        ExecutingInstructions => InstructionResult,
        AwaitingCondition => (),
        WritingToLedger => (),
        ExecutingSubProtocol => (),
        ExecutionComplete @ final => InstructionResult,
        ExecutionFailed @ final => (),
    }

    Extract {
        session_id: |core| core.session_id,
        device_id: |core| core.device_id,
    }
}

// ========== Protocol Type Alias ==========

/// Session-typed protocol context wrapper
pub type ContextProtocol<S> = ChoreographicProtocol<ProtocolContextCore, S>;

// ========== Protocol Methods ==========

impl<S: SessionState> ChoreographicProtocol<ProtocolContextCore, S> {
    /// Get reference to the protocol core
    pub fn core(&self) -> &ProtocolContextCore {
        &self.inner
    }

    /// Get the context ID
    pub fn context_id(&self) -> Uuid {
        self.core().context_id
    }

    /// Get the protocol type
    pub fn protocol_type(&self) -> ProtocolType {
        self.core().protocol_type
    }
    
    /// Get the protocol ID (alias for context_id for compatibility)
    pub fn protocol_id(&self) -> Uuid {
        self.context_id()
    }
    
    /// Get the device ID
    pub fn device_id(&self) -> aura_journal::DeviceId {
        self.core().device_id
    }
    
    /// Check if the protocol can terminate (final states only)
    pub fn can_terminate(&self) -> bool {
        S::IS_FINAL
    }
}

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

// ========== State Transitions ==========

/// Transition from ContextInitialized to ExecutingInstructions (no witness needed)
impl ChoreographicProtocol<ProtocolContextCore, ContextInitialized> {
    /// Begin executing instructions
    pub fn begin_execution(self) -> ChoreographicProtocol<ProtocolContextCore, ExecutingInstructions> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from ExecutingInstructions to various states based on instruction type
impl ChoreographicProtocol<ProtocolContextCore, ExecutingInstructions> {
    /// Transition to awaiting condition state
    pub fn await_condition(self) -> ChoreographicProtocol<ProtocolContextCore, AwaitingCondition> {
        ChoreographicProtocol::transition_to(self)
    }
    
    /// Transition to writing to ledger state
    pub fn write_to_ledger(self) -> ChoreographicProtocol<ProtocolContextCore, WritingToLedger> {
        ChoreographicProtocol::transition_to(self)
    }
    
    /// Transition to executing sub-protocol state
    pub fn execute_sub_protocol(self) -> ChoreographicProtocol<ProtocolContextCore, ExecutingSubProtocol> {
        ChoreographicProtocol::transition_to(self)
    }
    
    /// Complete execution successfully
    pub fn complete_execution(self) -> ChoreographicProtocol<ProtocolContextCore, ExecutionComplete> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from AwaitingCondition back to ExecutingInstructions (requires ThresholdEventsMet witness for threshold operations)
impl WitnessedTransition<AwaitingCondition, ExecutingInstructions> 
    for ChoreographicProtocol<ProtocolContextCore, AwaitingCondition> 
{
    type Witness = ThresholdEventsMet;
    type Target = ChoreographicProtocol<ProtocolContextCore, ExecutingInstructions>;
    
    /// Resume execution after threshold condition is met
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Simple transition from AwaitingCondition to ExecutingInstructions (for non-threshold operations)
impl ChoreographicProtocol<ProtocolContextCore, AwaitingCondition> {
    /// Resume execution after simple condition is met
    pub fn resume_execution(self) -> ChoreographicProtocol<ProtocolContextCore, ExecutingInstructions> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from WritingToLedger back to ExecutingInstructions (requires LedgerWriteComplete witness)
impl WitnessedTransition<WritingToLedger, ExecutingInstructions>
    for ChoreographicProtocol<ProtocolContextCore, WritingToLedger>
{
    type Witness = LedgerWriteComplete;
    type Target = ChoreographicProtocol<ProtocolContextCore, ExecutingInstructions>;
    
    /// Resume execution after ledger write is complete
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from ExecutingSubProtocol back to ExecutingInstructions (requires SubProtocolComplete witness)
impl WitnessedTransition<ExecutingSubProtocol, ExecutingInstructions> 
    for ChoreographicProtocol<ProtocolContextCore, ExecutingSubProtocol> 
{
    type Witness = SubProtocolComplete;
    type Target = ChoreographicProtocol<ProtocolContextCore, ExecutingInstructions>;
    
    /// Resume execution after sub-protocol completes
    fn transition_with_witness(
        self, 
        _witness: Self::Witness
    ) -> Self::Target {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition to ExecutionFailed from any state (no witness needed)
impl<S: SessionState> ChoreographicProtocol<ProtocolContextCore, S> {
    /// Fail the execution (can be called from any state)
    pub fn fail_execution(self) -> ChoreographicProtocol<ProtocolContextCore, ExecutionFailed> {
        ChoreographicProtocol::transition_to(self)
    }
}

// ========== Context-Specific Operations ==========

/// Operations only available in ExecutingInstructions state
impl ChoreographicProtocol<ProtocolContextCore, ExecutingInstructions> {
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
impl ChoreographicProtocol<ProtocolContextCore, AwaitingCondition> {
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
impl ChoreographicProtocol<ProtocolContextCore, WritingToLedger> {
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
impl ChoreographicProtocol<ProtocolContextCore, ExecutingSubProtocol> {
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
impl ChoreographicProtocol<ProtocolContextCore, ExecutionComplete> {
    /// Get the final execution result
    pub fn get_execution_result(&self) -> Option<String> {
        Some("Execution completed successfully".to_string())
    }
}

impl ChoreographicProtocol<ProtocolContextCore, ExecutionFailed> {
    /// Get the failure reason
    pub fn get_failure_reason(&self) -> Option<String> {
        Some("Execution failed".to_string())
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed protocol context
pub fn new_session_typed_context(
    context_id: Uuid,
    session_id: Uuid,
    device_id: DeviceId,
    protocol_type: ProtocolType,
) -> ChoreographicProtocol<ProtocolContextCore, ContextInitialized> {
    let core = ProtocolContextCore::new(context_id, session_id, device_id, protocol_type);
    ChoreographicProtocol::new(core)
}

/// Rehydrate a protocol context session from execution state
pub fn rehydrate_context_session(
    context_id: Uuid,
    session_id: Uuid,
    device_id: DeviceId,
    protocol_type: ProtocolType,
    last_instruction: Option<&Instruction>
) -> ContextSessionState {
    let core = ProtocolContextCore::new(context_id, session_id, device_id, protocol_type);
    
    match last_instruction {
        Some(Instruction::WriteToLedger(_)) => ContextSessionState::WritingToLedger(
            ChoreographicProtocol::new(core)
        ),
        Some(Instruction::AwaitEvent { .. }) | Some(Instruction::AwaitThreshold { .. }) => {
            ContextSessionState::AwaitingCondition(ChoreographicProtocol::new(core))
        },
        Some(Instruction::RunSubProtocol { .. }) => ContextSessionState::ExecutingSubProtocol(
            ChoreographicProtocol::new(core)
        ),
        _ => ContextSessionState::ExecutingInstructions(ChoreographicProtocol::new(core)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_journal::DeviceId;
    
    #[test]
    fn test_context_state_transitions() {
        let effects = Effects::test();
        let context_id = effects.gen_uuid();
        let session_id = effects.gen_uuid();
        let device_id = DeviceId::new_with_effects(&effects);
        let protocol_type = ProtocolType::Dkd;
        
        // Test basic state transitions
        let session_context = new_session_typed_context(context_id, session_id, device_id, protocol_type);
        
        // Should start in ContextInitialized state
        assert_eq!(session_context.state_name(), "ContextInitialized");
        assert!(!session_context.can_terminate());
        assert!(!session_context.is_final());
        
        // Transition to ExecutingInstructions
        let executing_context = session_context.begin_execution();
        assert_eq!(executing_context.state_name(), "ExecutingInstructions");
        
        // Can fail from any state
        let failed_context = executing_context.fail_execution();
        assert_eq!(failed_context.state_name(), "ExecutionFailed");
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
        let events = vec![];
        let witness = ThresholdEventsMet::verify((events, 0), ());
        assert!(witness.is_some());
    }
    
    #[test]
    fn test_context_rehydration() {
        let effects = Effects::test();
        let context_id = effects.gen_uuid();
        let session_id = effects.gen_uuid();
        let device_id = DeviceId::new_with_effects(&effects);
        let protocol_type = ProtocolType::Dkd;
        
        // Test rehydration without last instruction
        let state = rehydrate_context_session(context_id, session_id, device_id, protocol_type, None);
        assert_eq!(state.state_name(), "ExecutingInstructions");
        assert!(!state.can_terminate());
        assert!(!state.is_final());
    }

    #[test]
    fn test_context_specific_operations() {
        let effects = Effects::test();
        let context_id = effects.gen_uuid();
        let session_id = effects.gen_uuid();
        let device_id = DeviceId::new_with_effects(&effects);
        let protocol_type = ProtocolType::Dkd;
        
        let session_context = new_session_typed_context(context_id, session_id, device_id, protocol_type);
        let executing_context = session_context.begin_execution();
        
        // Create a test event for instruction testing
        let dummy_signature = ed25519_dalek::Signature::from_bytes(&[0u8; 64]);
        let test_event = aura_journal::Event::new(
            aura_journal::AccountId(effects.gen_uuid()),
            1,
            None,
            0,
            aura_journal::EventType::EpochTick(aura_journal::events::EpochTickEvent { 
                new_epoch: 1,
                evidence_hash: [0u8; 32],
            }),
            aura_journal::EventAuthorization::DeviceCertificate {
                device_id,
                signature: dummy_signature,
            },
            &effects,
        ).unwrap();
        
        // Test instruction type checking
        let write_instruction = Instruction::WriteToLedger(test_event);
        assert!(executing_context.requires_state_transition(&write_instruction));
        
        // Transition to different states
        let awaiting_context = executing_context.await_condition();
        assert_eq!(awaiting_context.state_name(), "AwaitingCondition");
        
        // Resume execution
        let resumed_context = awaiting_context.resume_execution();
        assert_eq!(resumed_context.state_name(), "ExecutingInstructions");
    }

    #[test]
    fn test_union_type_functionality() {
        let effects = Effects::test();
        let context_id = effects.gen_uuid();
        let session_id = effects.gen_uuid();
        let device_id = DeviceId::new_with_effects(&effects);
        let protocol_type = ProtocolType::Dkd;
        
        let state = rehydrate_context_session(context_id, session_id, device_id, protocol_type, None);
        
        // Test union type methods
        assert_eq!(state.state_name(), "ExecutingInstructions");
        assert!(!state.can_terminate());
        assert!(!state.is_final());
        
        // Test protocol_id and device_id delegation
        assert_eq!(state.protocol_id(), context_id);
        assert_eq!(state.device_id(), device_id);
    }
}