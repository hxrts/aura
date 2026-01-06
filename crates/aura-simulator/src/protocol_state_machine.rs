//! Protocol State Machine for Stepped Simulation
//!
//! This module provides a state machine abstraction for choreographic protocols,
//! enabling stepped execution where each protocol step is controlled explicitly.
//!
//! ## Features
//!
//! - Step-by-step protocol execution
//! - Explicit message input/output control
//! - State inspection at any point
//! - Integration with scheduler for interleaving
//! - Support for deterministic replay
//!
//! ## Example
//!
//! ```ignore
//! use aura_simulator::protocol_state_machine::{ProtocolStateMachine, StepResult};
//!
//! let mut coordinator = ProtocolStateMachine::new("Coordinator");
//! let mut witness = ProtocolStateMachine::new("Witness");
//!
//! // Step coordinator - produces output message
//! let output = coordinator.step(None)?;
//!
//! // Feed output to witness
//! let response = witness.step(Some(output.message))?;
//!
//! // Continue stepping until all complete
//! while !coordinator.is_complete() || !witness.is_complete() {
//!     // Scheduler decides which role steps next
//! }
//! ```

use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;

use crate::choreography_observer::SimulatorObserver;

/// State of a protocol participant
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParticipantState {
    /// Waiting for initial input or ready to start
    Ready,
    /// Waiting to receive a message from another role
    WaitingForMessage { from: String },
    /// Waiting to send a message to another role
    ReadyToSend { to: String },
    /// Making a choice between branches
    AtChoice { options: Vec<String> },
    /// Protocol execution complete
    Complete,
    /// Protocol failed with error
    Failed { error: String },
}

/// Result of a protocol step
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Step produced an outgoing message
    Send {
        /// Target role for the message
        to: String,
        /// Serialized message bytes
        message: Vec<u8>,
        /// Message type name
        message_type: String,
    },
    /// Step is waiting for an incoming message
    NeedInput {
        /// Role expected to send the message
        from: String,
    },
    /// Step made a choice, protocol continues
    Chose {
        /// Branch that was selected
        branch: String,
    },
    /// Protocol completed successfully
    Complete,
    /// Step resulted in an error
    Error {
        /// Error description
        message: String,
    },
}

/// A steppable protocol state machine
pub struct ProtocolStateMachine {
    /// Role name for this participant
    role: String,
    /// Current state
    state: RwLock<ParticipantState>,
    /// Queued incoming messages
    incoming: RwLock<VecDeque<(String, Vec<u8>)>>,
    /// Queued outgoing messages
    outgoing: RwLock<VecDeque<(String, Vec<u8>, String)>>,
    /// Step counter
    step_count: RwLock<usize>,
    /// Optional observer for event tracing
    observer: Option<Arc<SimulatorObserver>>,
}

impl ProtocolStateMachine {
    /// Create a new state machine for the given role
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            state: RwLock::new(ParticipantState::Ready),
            incoming: RwLock::new(VecDeque::new()),
            outgoing: RwLock::new(VecDeque::new()),
            step_count: RwLock::new(0),
            observer: None,
        }
    }

    /// Create a state machine with an observer attached
    pub fn with_observer(role: impl Into<String>, observer: Arc<SimulatorObserver>) -> Self {
        Self {
            role: role.into(),
            state: RwLock::new(ParticipantState::Ready),
            incoming: RwLock::new(VecDeque::new()),
            outgoing: RwLock::new(VecDeque::new()),
            step_count: RwLock::new(0),
            observer: Some(observer),
        }
    }

    /// Get the role name
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Get the current state
    pub fn state(&self) -> ParticipantState {
        self.state.read().clone()
    }

    /// Check if the protocol is complete
    pub fn is_complete(&self) -> bool {
        matches!(*self.state.read(), ParticipantState::Complete)
    }

    /// Check if the protocol has failed
    pub fn is_failed(&self) -> bool {
        matches!(*self.state.read(), ParticipantState::Failed { .. })
    }

    /// Get the current step count
    pub fn step_count(&self) -> usize {
        *self.step_count.read()
    }

    /// Check if there's a pending outgoing message
    pub fn has_pending_output(&self) -> bool {
        !self.outgoing.read().is_empty()
    }

    /// Queue an incoming message from another role
    pub fn queue_message(&self, from: impl Into<String>, message: Vec<u8>) {
        self.incoming.write().push_back((from.into(), message));
    }

    /// Take the next pending output message, if any
    pub fn take_output(&self) -> Option<(String, Vec<u8>, String)> {
        self.outgoing.write().pop_front()
    }

    /// Queue an outgoing message (for use during step execution)
    pub fn queue_output(&self, to: impl Into<String>, message: Vec<u8>, message_type: impl Into<String>) {
        let to = to.into();
        let message_type = message_type.into();

        if let Some(obs) = &self.observer {
            obs.on_send(&self.role, &to, &message_type, message.len());
        }

        self.outgoing.write().push_back((to, message, message_type));
    }

    /// Execute a single step of the protocol
    ///
    /// Returns what happened during the step. The caller should:
    /// - For `Send`: Route the message to the target role
    /// - For `NeedInput`: Queue the message when available
    /// - For `Chose`: Continue stepping
    /// - For `Complete`/`Error`: Stop stepping this participant
    pub fn step(&self, input: Option<(String, Vec<u8>)>) -> StepResult {
        // Increment step counter
        *self.step_count.write() += 1;

        // If input was provided, queue it
        if let Some((from, msg)) = input {
            self.queue_message(from, msg);
        }

        // Check current state and determine next action
        let current_state = self.state.read().clone();

        match current_state {
            ParticipantState::Ready => {
                // Ready to start - check if we need input or can produce output
                if let Some((from, msg)) = self.incoming.write().pop_front() {
                    if let Some(obs) = &self.observer {
                        obs.on_receive(&from, &self.role, "message", msg.len());
                    }
                    // Process the message and determine next state
                    self.process_incoming(&from, &msg)
                } else if let Some((to, msg, msg_type)) = self.outgoing.write().pop_front() {
                    StepResult::Send {
                        to,
                        message: msg,
                        message_type: msg_type,
                    }
                } else {
                    // Nothing to do, stay ready
                    StepResult::NeedInput {
                        from: "any".to_string(),
                    }
                }
            }
            ParticipantState::WaitingForMessage { from } => {
                // Try to receive from the expected sender
                let mut incoming = self.incoming.write();
                if let Some(pos) = incoming.iter().position(|(f, _)| f == &from) {
                    let (from, msg) = incoming.remove(pos).unwrap();
                    drop(incoming);

                    if let Some(obs) = &self.observer {
                        obs.on_receive(&from, &self.role, "message", msg.len());
                    }

                    self.process_incoming(&from, &msg)
                } else {
                    StepResult::NeedInput { from }
                }
            }
            ParticipantState::ReadyToSend { to: _ } => {
                // Send the next queued message
                if let Some((target, msg, msg_type)) = self.outgoing.write().pop_front() {
                    StepResult::Send {
                        to: target,
                        message: msg,
                        message_type: msg_type,
                    }
                } else {
                    // No message queued - this is an error state
                    *self.state.write() = ParticipantState::Failed {
                        error: "No message to send".to_string(),
                    };
                    StepResult::Error {
                        message: "No message to send".to_string(),
                    }
                }
            }
            ParticipantState::AtChoice { options } => {
                // For simulation, we just pick the first option
                // Real implementations would use decision sourcing
                let choice = options.first().cloned().unwrap_or_default();

                if let Some(obs) = &self.observer {
                    obs.on_choice(&self.role, &choice);
                }

                *self.state.write() = ParticipantState::Ready;
                StepResult::Chose { branch: choice }
            }
            ParticipantState::Complete => {
                StepResult::Complete
            }
            ParticipantState::Failed { error } => {
                StepResult::Error { message: error }
            }
        }
    }

    /// Mark the protocol as complete
    pub fn mark_complete(&self) {
        *self.state.write() = ParticipantState::Complete;
        if let Some(obs) = &self.observer {
            obs.on_phase_end(&self.role, "protocol");
        }
    }

    /// Mark the protocol as failed
    pub fn mark_failed(&self, error: impl Into<String>) {
        let error = error.into();
        if let Some(obs) = &self.observer {
            obs.on_error(&self.role, &error);
        }
        *self.state.write() = ParticipantState::Failed { error };
    }

    /// Set the state to waiting for a message from a specific role
    pub fn wait_for(&self, from: impl Into<String>) {
        *self.state.write() = ParticipantState::WaitingForMessage { from: from.into() };
    }

    /// Set the state to ready to send to a specific role
    pub fn ready_to_send(&self, to: impl Into<String>) {
        *self.state.write() = ParticipantState::ReadyToSend { to: to.into() };
    }

    /// Set the state to a choice point
    pub fn at_choice(&self, options: Vec<String>) {
        *self.state.write() = ParticipantState::AtChoice { options };
    }

    /// Process an incoming message (stub for actual protocol logic)
    fn process_incoming(&self, from: &str, msg: &[u8]) -> StepResult {
        // In a real implementation, this would parse the message and
        // update protocol state accordingly. For the simulation framework,
        // we provide hooks for the protocol adapter to call.
        let _ = (from, msg);
        *self.state.write() = ParticipantState::Ready;
        StepResult::NeedInput {
            from: "any".to_string(),
        }
    }
}

impl std::fmt::Debug for ProtocolStateMachine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolStateMachine")
            .field("role", &self.role)
            .field("state", &*self.state.read())
            .field("step_count", &*self.step_count.read())
            .field("pending_incoming", &self.incoming.read().len())
            .field("pending_outgoing", &self.outgoing.read().len())
            .finish()
    }
}

/// A scheduler for coordinating multiple state machines
pub struct ProtocolScheduler {
    /// Participants in the protocol
    participants: Vec<ProtocolStateMachine>,
    /// Message queue between participants (from_idx, to_idx, message)
    message_queue: RwLock<VecDeque<(usize, usize, Vec<u8>, String)>>,
    /// Total steps executed across all participants
    total_steps: RwLock<usize>,
}

impl ProtocolScheduler {
    /// Create a scheduler from a list of participant state machines
    pub fn new(participants: Vec<ProtocolStateMachine>) -> Self {
        Self {
            participants,
            message_queue: RwLock::new(VecDeque::new()),
            total_steps: RwLock::new(0),
        }
    }

    /// Get the number of participants
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }

    /// Get a reference to a participant by index
    pub fn participant(&self, index: usize) -> Option<&ProtocolStateMachine> {
        self.participants.get(index)
    }

    /// Get a participant by role name
    pub fn participant_by_role(&self, role: &str) -> Option<&ProtocolStateMachine> {
        self.participants.iter().find(|p| p.role() == role)
    }

    /// Check if all participants have completed
    pub fn all_complete(&self) -> bool {
        self.participants.iter().all(|p| p.is_complete())
    }

    /// Check if any participant has failed
    pub fn any_failed(&self) -> bool {
        self.participants.iter().any(|p| p.is_failed())
    }

    /// Get total steps executed
    pub fn total_steps(&self) -> usize {
        *self.total_steps.read()
    }

    /// Find the index of a role by name
    fn role_index(&self, role: &str) -> Option<usize> {
        self.participants.iter().position(|p| p.role() == role)
    }

    /// Step a specific participant by index
    pub fn step_participant(&self, index: usize) -> Option<StepResult> {
        let participant = self.participants.get(index)?;

        // Check for pending messages for this participant
        let input = {
            let mut queue = self.message_queue.write();
            if let Some(pos) = queue.iter().position(|(_, to, _, _)| *to == index) {
                let (from_idx, _, msg, _msg_type) = queue.remove(pos).unwrap();
                let from_role = self.participants[from_idx].role().to_string();
                Some((from_role, msg))
            } else {
                None
            }
        };

        let result = participant.step(input);
        *self.total_steps.write() += 1;

        // Handle the result
        if let StepResult::Send { to, message, message_type } = &result {
            if let Some(to_idx) = self.role_index(to) {
                self.message_queue
                    .write()
                    .push_back((index, to_idx, message.clone(), message_type.clone()));
            }
        }

        Some(result)
    }

    /// Run all participants to completion using round-robin scheduling
    pub fn run_to_completion(&self) -> Result<(), String> {
        let max_steps = 10000; // Safety limit
        let mut steps = 0;

        while !self.all_complete() && steps < max_steps {
            if self.any_failed() {
                // Find the failed participant
                for p in &self.participants {
                    if let ParticipantState::Failed { error } = p.state() {
                        return Err(format!("Participant {} failed: {}", p.role(), error));
                    }
                }
            }

            // Round-robin step
            for i in 0..self.participants.len() {
                if !self.participants[i].is_complete() {
                    let _ = self.step_participant(i);
                    steps += 1;
                }
            }
        }

        if steps >= max_steps {
            Err("Protocol did not complete within step limit".to_string())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_machine_creation() {
        let sm = ProtocolStateMachine::new("Alice");
        assert_eq!(sm.role(), "Alice");
        assert_eq!(sm.state(), ParticipantState::Ready);
        assert!(!sm.is_complete());
        assert!(!sm.is_failed());
    }

    #[test]
    fn test_message_queueing() {
        let sm = ProtocolStateMachine::new("Alice");

        sm.queue_message("Bob", vec![1, 2, 3]);
        sm.queue_output("Bob", vec![4, 5, 6], "Ping");

        assert!(sm.has_pending_output());

        let output = sm.take_output();
        assert!(output.is_some());
        let (to, msg, msg_type) = output.unwrap();
        assert_eq!(to, "Bob");
        assert_eq!(msg, vec![4, 5, 6]);
        assert_eq!(msg_type, "Ping");
    }

    #[test]
    fn test_state_transitions() {
        let sm = ProtocolStateMachine::new("Alice");

        sm.wait_for("Bob");
        assert!(matches!(sm.state(), ParticipantState::WaitingForMessage { .. }));

        sm.ready_to_send("Bob");
        assert!(matches!(sm.state(), ParticipantState::ReadyToSend { .. }));

        sm.at_choice(vec!["Continue".to_string(), "Stop".to_string()]);
        assert!(matches!(sm.state(), ParticipantState::AtChoice { .. }));

        sm.mark_complete();
        assert!(sm.is_complete());
    }

    #[test]
    fn test_scheduler_creation() {
        let alice = ProtocolStateMachine::new("Alice");
        let bob = ProtocolStateMachine::new("Bob");

        let scheduler = ProtocolScheduler::new(vec![alice, bob]);

        assert_eq!(scheduler.participant_count(), 2);
        assert!(scheduler.participant_by_role("Alice").is_some());
        assert!(scheduler.participant_by_role("Bob").is_some());
        assert!(scheduler.participant_by_role("Charlie").is_none());
    }
}
