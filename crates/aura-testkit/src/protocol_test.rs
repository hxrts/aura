//! Fluent Protocol Test API
//!
//! This module provides a fluent test API for choreographic protocols,
//! simplifying the setup and execution of multi-party protocol tests.
//!
//! ## Example: Basic Setup
//!
//! ```ignore
//! use aura_testkit::ProtocolTest;
//!
//! let test = ProtocolTest::new("AuraConsensus")
//!     .bind_role("Coordinator", coord_device)
//!     .bind_roles("Witness", &[w1, w2, w3])
//!     .expect_success();
//!
//! let harness = test.build_harness()?;
//! ```
//!
//! ## Example: Using execute_as with Protocol Runners
//!
//! ```ignore
//! use aura_consensus::protocol::runners::{execute_as, AuraConsensusRole};
//! use aura_testkit::ProtocolTest;
//!
//! // Setup test with role bindings
//! let test = ProtocolTest::new("AuraConsensus")
//!     .bind_role("Coordinator", coord_device)
//!     .bind_roles("Witness", &[w1, w2, w3])
//!     .with_observer();
//!
//! // Execute using the generated runners
//! let output = execute_as(
//!     AuraConsensusRole::Coordinator,
//!     transport,
//!     |channel| async move {
//!         // Protocol logic here
//!         Ok(())
//!     }
//! ).await?;
//! ```
//!
//! ## Example: Stepped Simulation
//!
//! ```ignore
//! use aura_testkit::protocol_test::{ProtocolTestRunner, SteppedExecution};
//!
//! let mut runner = ProtocolTestRunner::new("TestProtocol")
//!     .add_participant("Alice")
//!     .add_participant("Bob");
//!
//! // Step through execution manually
//! while !runner.all_complete() {
//!     let step = runner.step_next()?;
//!     println!("Stepped: {:?}", step);
//! }
//! ```

use crate::simulation::choreography::{ChoreographyTestHarness, TestError};
use aura_core::DeviceId;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Fluent protocol test builder
///
/// Provides a builder pattern for setting up and running protocol tests.
pub struct ProtocolTest {
    /// Protocol name for logging/debugging
    protocol_name: String,
    /// Role bindings: role name -> device ID
    role_bindings: HashMap<String, DeviceId>,
    /// Role family bindings: family name -> list of device IDs
    role_family_bindings: HashMap<String, Vec<DeviceId>>,
    /// Whether to expect success
    expect_success: bool,
    /// Test execution mode
    execution_mode: ExecutionMode,
}

/// Test execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Sequential execution (default, safer for debugging)
    Sequential,
    /// Parallel execution (faster, more realistic)
    Parallel,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Sequential
    }
}

impl ProtocolTest {
    /// Create a new protocol test for the given protocol name
    pub fn new(protocol_name: impl Into<String>) -> Self {
        Self {
            protocol_name: protocol_name.into(),
            role_bindings: HashMap::new(),
            role_family_bindings: HashMap::new(),
            expect_success: true,
            execution_mode: ExecutionMode::default(),
        }
    }

    /// Bind a single role to a device
    pub fn bind_role(mut self, role_name: impl Into<String>, device_id: DeviceId) -> Self {
        self.role_bindings.insert(role_name.into(), device_id);
        self
    }

    /// Bind a role family (e.g., Witness[N]) to multiple devices
    pub fn bind_roles(mut self, family_name: impl Into<String>, device_ids: &[DeviceId]) -> Self {
        self.role_family_bindings
            .insert(family_name.into(), device_ids.to_vec());
        self
    }

    /// Expect the protocol to succeed
    pub fn expect_success(mut self) -> Self {
        self.expect_success = true;
        self
    }

    /// Expect the protocol to fail
    pub fn expect_failure(mut self) -> Self {
        self.expect_success = false;
        self
    }

    /// Set execution mode
    pub fn with_execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.execution_mode = mode;
        self
    }

    /// Build a ChoreographyTestHarness from the configured bindings
    pub fn build_harness(&self) -> Result<ChoreographyTestHarness, TestError> {
        // Collect all unique device IDs
        let mut all_devices: Vec<DeviceId> = self.role_bindings.values().copied().collect();
        for family_devices in self.role_family_bindings.values() {
            for &device_id in family_devices {
                if !all_devices.contains(&device_id) {
                    all_devices.push(device_id);
                }
            }
        }

        // Create harness with device count
        let mut harness = ChoreographyTestHarness::with_devices(all_devices.len());

        // Map roles to device indices
        for (i, &device_id) in all_devices.iter().enumerate() {
            // Find role names for this device
            for (role_name, &role_device) in &self.role_bindings {
                if role_device == device_id {
                    harness.map_role(role_name, i)?;
                }
            }
        }

        Ok(harness)
    }

    /// Get all device IDs participating in the test
    pub fn all_devices(&self) -> Vec<DeviceId> {
        let mut all_devices: Vec<DeviceId> = self.role_bindings.values().copied().collect();
        for family_devices in self.role_family_bindings.values() {
            for &device_id in family_devices {
                if !all_devices.contains(&device_id) {
                    all_devices.push(device_id);
                }
            }
        }
        all_devices
    }

    /// Get role bindings
    pub fn role_bindings(&self) -> &HashMap<String, DeviceId> {
        &self.role_bindings
    }

    /// Get role family bindings
    pub fn role_family_bindings(&self) -> &HashMap<String, Vec<DeviceId>> {
        &self.role_family_bindings
    }

    /// Get protocol name
    pub fn protocol_name(&self) -> &str {
        &self.protocol_name
    }

    /// Check if success is expected
    pub fn expects_success(&self) -> bool {
        self.expect_success
    }
}

/// Result of a protocol test execution
#[derive(Debug)]
pub struct ProtocolTestResult {
    /// Protocol name
    pub protocol_name: String,
    /// Whether the test succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Collected outputs from each role
    pub role_outputs: HashMap<String, Vec<u8>>,
}

impl ProtocolTestResult {
    /// Check if the test succeeded
    pub fn is_success(&self) -> bool {
        self.success
    }

    /// Get error message if failed
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Assert that the test succeeded
    pub fn assert_success(&self) {
        assert!(
            self.success,
            "Protocol '{}' failed: {}",
            self.protocol_name,
            self.error.as_deref().unwrap_or("unknown error")
        );
    }

    /// Assert that the test failed
    pub fn assert_failure(&self) {
        assert!(
            !self.success,
            "Protocol '{}' should have failed but succeeded",
            self.protocol_name
        );
    }
}

// =============================================================================
// Protocol Event Tracing
// =============================================================================

/// Events that occur during protocol test execution
#[derive(Debug, Clone)]
pub enum ProtocolEvent {
    /// A protocol phase started
    PhaseStart {
        role: String,
        phase: String,
        timestamp: Duration,
    },
    /// A protocol phase ended
    PhaseEnd {
        role: String,
        phase: String,
        timestamp: Duration,
        duration: Duration,
    },
    /// A message was sent between roles
    MessageSent {
        from: String,
        to: String,
        message_type: String,
        size: usize,
        timestamp: Duration,
    },
    /// A message was received
    MessageReceived {
        from: String,
        to: String,
        message_type: String,
        size: usize,
        timestamp: Duration,
    },
    /// A choice was made at a branch point
    Choice {
        role: String,
        branch: String,
        timestamp: Duration,
    },
    /// An error occurred
    Error {
        role: String,
        message: String,
        timestamp: Duration,
    },
    /// Protocol completed
    Complete { timestamp: Duration },
}

/// Observer for tracking protocol test events
#[derive(Debug)]
pub struct ProtocolTestObserver {
    protocol_name: String,
    start_time: Instant,
    events: Vec<ProtocolEvent>,
    phase_starts: HashMap<(String, String), Instant>,
}

impl ProtocolTestObserver {
    /// Create a new observer for the given protocol
    pub fn new(protocol_name: impl Into<String>) -> Self {
        Self {
            protocol_name: protocol_name.into(),
            start_time: Instant::now(),
            events: Vec::new(),
            phase_starts: HashMap::new(),
        }
    }

    /// Record the start of a phase
    pub fn on_phase_start(&mut self, role: &str, phase: &str) {
        let timestamp = self.start_time.elapsed();
        self.phase_starts
            .insert((role.to_string(), phase.to_string()), Instant::now());
        self.events.push(ProtocolEvent::PhaseStart {
            role: role.to_string(),
            phase: phase.to_string(),
            timestamp,
        });
    }

    /// Record the end of a phase
    pub fn on_phase_end(&mut self, role: &str, phase: &str) {
        let timestamp = self.start_time.elapsed();
        let duration = self
            .phase_starts
            .remove(&(role.to_string(), phase.to_string()))
            .map(|start| start.elapsed())
            .unwrap_or(Duration::ZERO);

        self.events.push(ProtocolEvent::PhaseEnd {
            role: role.to_string(),
            phase: phase.to_string(),
            timestamp,
            duration,
        });
    }

    /// Record a message send
    pub fn on_send(&mut self, from: &str, to: &str, message_type: &str, size: usize) {
        self.events.push(ProtocolEvent::MessageSent {
            from: from.to_string(),
            to: to.to_string(),
            message_type: message_type.to_string(),
            size,
            timestamp: self.start_time.elapsed(),
        });
    }

    /// Record a message receive
    pub fn on_receive(&mut self, from: &str, to: &str, message_type: &str, size: usize) {
        self.events.push(ProtocolEvent::MessageReceived {
            from: from.to_string(),
            to: to.to_string(),
            message_type: message_type.to_string(),
            size,
            timestamp: self.start_time.elapsed(),
        });
    }

    /// Record a choice
    pub fn on_choice(&mut self, role: &str, branch: &str) {
        self.events.push(ProtocolEvent::Choice {
            role: role.to_string(),
            branch: branch.to_string(),
            timestamp: self.start_time.elapsed(),
        });
    }

    /// Record an error
    pub fn on_error(&mut self, role: &str, message: &str) {
        self.events.push(ProtocolEvent::Error {
            role: role.to_string(),
            message: message.to_string(),
            timestamp: self.start_time.elapsed(),
        });
    }

    /// Record protocol completion
    pub fn on_complete(&mut self) {
        self.events.push(ProtocolEvent::Complete {
            timestamp: self.start_time.elapsed(),
        });
    }

    /// Get all recorded events
    pub fn events(&self) -> &[ProtocolEvent] {
        &self.events
    }

    /// Take all events, leaving the observer empty
    pub fn take_events(&mut self) -> Vec<ProtocolEvent> {
        std::mem::take(&mut self.events)
    }

    /// Get the protocol name
    pub fn protocol_name(&self) -> &str {
        &self.protocol_name
    }

    /// Get statistics about the protocol execution
    pub fn statistics(&self) -> ProtocolTestStatistics {
        let mut stats = ProtocolTestStatistics::default();

        for event in &self.events {
            match event {
                ProtocolEvent::MessageSent { size, .. } => {
                    stats.messages_sent += 1;
                    stats.bytes_sent += size;
                }
                ProtocolEvent::MessageReceived { size, .. } => {
                    stats.messages_received += 1;
                    stats.bytes_received += size;
                }
                ProtocolEvent::PhaseStart { .. } => {
                    stats.phases_started += 1;
                }
                ProtocolEvent::PhaseEnd { duration, .. } => {
                    stats.phases_completed += 1;
                    stats.total_phase_time += *duration;
                }
                ProtocolEvent::Choice { .. } => {
                    stats.choices_made += 1;
                }
                ProtocolEvent::Error { .. } => {
                    stats.errors += 1;
                }
                ProtocolEvent::Complete { timestamp } => {
                    stats.total_duration = Some(*timestamp);
                }
            }
        }

        stats
    }
}

/// Statistics collected during protocol test execution
#[derive(Debug, Clone, Default)]
pub struct ProtocolTestStatistics {
    /// Number of messages sent
    pub messages_sent: usize,
    /// Number of messages received
    pub messages_received: usize,
    /// Total bytes sent
    pub bytes_sent: usize,
    /// Total bytes received
    pub bytes_received: usize,
    /// Number of phases started
    pub phases_started: usize,
    /// Number of phases completed
    pub phases_completed: usize,
    /// Total time in phases
    pub total_phase_time: Duration,
    /// Number of choices made
    pub choices_made: usize,
    /// Number of errors
    pub errors: usize,
    /// Total duration (if completed)
    pub total_duration: Option<Duration>,
}

// =============================================================================
// Stepped Execution Support
// =============================================================================

/// State of a participant in stepped execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParticipantState {
    /// Ready to start or continue
    Ready,
    /// Waiting for a message from a specific role
    WaitingForMessage { from: String },
    /// Ready to send a message to a specific role
    ReadyToSend { to: String },
    /// At a choice point with options
    AtChoice { options: Vec<String> },
    /// Completed successfully
    Complete,
    /// Failed with error
    Failed { error: String },
}

/// Result of a single step in protocol execution
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Produced an outgoing message
    Send {
        to: String,
        message: Vec<u8>,
        message_type: String,
    },
    /// Waiting for input
    NeedInput { from: String },
    /// Made a choice
    Chose { branch: String },
    /// Completed
    Complete,
    /// Error occurred
    Error { message: String },
}

/// A participant in stepped protocol execution
#[derive(Debug)]
pub struct SteppedParticipant {
    role: String,
    state: ParticipantState,
    incoming: VecDeque<(String, Vec<u8>)>,
    outgoing: VecDeque<(String, Vec<u8>, String)>,
    step_count: usize,
}

impl SteppedParticipant {
    /// Create a new participant
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            state: ParticipantState::Ready,
            incoming: VecDeque::new(),
            outgoing: VecDeque::new(),
            step_count: 0,
        }
    }

    /// Get the role name
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Get current state
    pub fn state(&self) -> &ParticipantState {
        &self.state
    }

    /// Check if complete
    pub fn is_complete(&self) -> bool {
        matches!(self.state, ParticipantState::Complete)
    }

    /// Check if failed
    pub fn is_failed(&self) -> bool {
        matches!(self.state, ParticipantState::Failed { .. })
    }

    /// Get step count
    pub fn step_count(&self) -> usize {
        self.step_count
    }

    /// Queue an incoming message
    pub fn queue_message(&mut self, from: impl Into<String>, message: Vec<u8>) {
        self.incoming.push_back((from.into(), message));
    }

    /// Queue an outgoing message
    pub fn queue_output(
        &mut self,
        to: impl Into<String>,
        message: Vec<u8>,
        message_type: impl Into<String>,
    ) {
        self.outgoing
            .push_back((to.into(), message, message_type.into()));
    }

    /// Take the next pending output
    pub fn take_output(&mut self) -> Option<(String, Vec<u8>, String)> {
        self.outgoing.pop_front()
    }

    /// Set state to waiting for message
    pub fn wait_for(&mut self, from: impl Into<String>) {
        self.state = ParticipantState::WaitingForMessage { from: from.into() };
    }

    /// Set state to ready to send
    pub fn ready_to_send(&mut self, to: impl Into<String>) {
        self.state = ParticipantState::ReadyToSend { to: to.into() };
    }

    /// Set state to complete
    pub fn mark_complete(&mut self) {
        self.state = ParticipantState::Complete;
    }

    /// Set state to failed
    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.state = ParticipantState::Failed {
            error: error.into(),
        };
    }

    /// Execute a single step
    pub fn step(&mut self, input: Option<(String, Vec<u8>)>) -> StepResult {
        self.step_count += 1;

        // Queue input if provided
        if let Some((from, msg)) = input {
            self.queue_message(from, msg);
        }

        match &self.state {
            ParticipantState::Ready => {
                // Check for pending output first
                if let Some((to, msg, msg_type)) = self.outgoing.pop_front() {
                    return StepResult::Send {
                        to,
                        message: msg,
                        message_type: msg_type,
                    };
                }
                // Then check for pending input
                if let Some((from, _msg)) = self.incoming.pop_front() {
                    self.state = ParticipantState::Ready;
                    return StepResult::NeedInput { from };
                }
                StepResult::NeedInput {
                    from: "any".to_string(),
                }
            }
            ParticipantState::WaitingForMessage { from } => {
                // Check if we have the expected message
                if let Some(pos) = self.incoming.iter().position(|(f, _)| f == from) {
                    let (from, _msg) = self.incoming.remove(pos).unwrap();
                    self.state = ParticipantState::Ready;
                    StepResult::NeedInput { from }
                } else {
                    StepResult::NeedInput { from: from.clone() }
                }
            }
            ParticipantState::ReadyToSend { to: _ } => {
                if let Some((to, msg, msg_type)) = self.outgoing.pop_front() {
                    StepResult::Send {
                        to,
                        message: msg,
                        message_type: msg_type,
                    }
                } else {
                    self.state = ParticipantState::Failed {
                        error: "No message to send".to_string(),
                    };
                    StepResult::Error {
                        message: "No message to send".to_string(),
                    }
                }
            }
            ParticipantState::AtChoice { options } => {
                let choice = options.first().cloned().unwrap_or_default();
                self.state = ParticipantState::Ready;
                StepResult::Chose { branch: choice }
            }
            ParticipantState::Complete => StepResult::Complete,
            ParticipantState::Failed { error } => StepResult::Error {
                message: error.clone(),
            },
        }
    }
}

/// Runner for stepped protocol execution
#[derive(Debug)]
pub struct ProtocolTestRunner {
    protocol_name: String,
    participants: Vec<SteppedParticipant>,
    message_queue: VecDeque<(usize, usize, Vec<u8>, String)>,
    total_steps: usize,
    observer: Option<ProtocolTestObserver>,
}

impl ProtocolTestRunner {
    /// Create a new runner for the given protocol
    pub fn new(protocol_name: impl Into<String>) -> Self {
        Self {
            protocol_name: protocol_name.into(),
            participants: Vec::new(),
            message_queue: VecDeque::new(),
            total_steps: 0,
            observer: None,
        }
    }

    /// Add a participant with the given role name
    pub fn add_participant(mut self, role: impl Into<String>) -> Self {
        self.participants.push(SteppedParticipant::new(role));
        self
    }

    /// Enable event observation
    pub fn with_observer(mut self) -> Self {
        self.observer = Some(ProtocolTestObserver::new(&self.protocol_name));
        self
    }

    /// Get the number of participants
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }

    /// Get a participant by index
    pub fn participant(&self, index: usize) -> Option<&SteppedParticipant> {
        self.participants.get(index)
    }

    /// Get a mutable participant by index
    pub fn participant_mut(&mut self, index: usize) -> Option<&mut SteppedParticipant> {
        self.participants.get_mut(index)
    }

    /// Get a participant by role name
    pub fn participant_by_role(&self, role: &str) -> Option<&SteppedParticipant> {
        self.participants.iter().find(|p| p.role() == role)
    }

    /// Check if all participants are complete
    pub fn all_complete(&self) -> bool {
        self.participants.iter().all(|p| p.is_complete())
    }

    /// Check if any participant has failed
    pub fn any_failed(&self) -> bool {
        self.participants.iter().any(|p| p.is_failed())
    }

    /// Get total steps executed
    pub fn total_steps(&self) -> usize {
        self.total_steps
    }

    /// Find participant index by role
    fn role_index(&self, role: &str) -> Option<usize> {
        self.participants.iter().position(|p| p.role() == role)
    }

    /// Step a specific participant
    pub fn step_participant(&mut self, index: usize) -> Option<StepResult> {
        if index >= self.participants.len() {
            return None;
        }

        // Check for pending messages - get from_role before borrowing participant
        let input = {
            if let Some(pos) = self
                .message_queue
                .iter()
                .position(|(_, to, _, _)| *to == index)
            {
                let (from_idx, _, msg, _) = self.message_queue.remove(pos).unwrap();
                let from_role = self.participants[from_idx].role().to_string();
                Some((from_role, msg))
            } else {
                None
            }
        };

        // Now borrow participant mutably and step
        let result = self.participants[index].step(input);
        self.total_steps += 1;

        // Handle message routing
        if let StepResult::Send {
            to,
            message,
            message_type,
        } = &result
        {
            if let Some(to_idx) = self.role_index(to) {
                // Get role name for observer before mutable borrow
                let from_role = self.participants[index].role().to_string();

                // Record in observer
                if let Some(obs) = &mut self.observer {
                    obs.on_send(&from_role, to, message_type, message.len());
                }
                self.message_queue.push_back((
                    index,
                    to_idx,
                    message.clone(),
                    message_type.clone(),
                ));
            }
        }

        Some(result)
    }

    /// Step the next ready participant (round-robin)
    pub fn step_next(&mut self) -> Option<StepResult> {
        for i in 0..self.participants.len() {
            if !self.participants[i].is_complete() && !self.participants[i].is_failed() {
                return self.step_participant(i);
            }
        }
        None
    }

    /// Run to completion with a step limit
    pub fn run_to_completion(&mut self, max_steps: usize) -> Result<(), String> {
        while !self.all_complete() && self.total_steps < max_steps {
            if self.any_failed() {
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
                }
            }
        }

        if self.total_steps >= max_steps {
            Err("Protocol did not complete within step limit".to_string())
        } else {
            if let Some(obs) = &mut self.observer {
                obs.on_complete();
            }
            Ok(())
        }
    }

    /// Get the observer (if enabled)
    pub fn observer(&self) -> Option<&ProtocolTestObserver> {
        self.observer.as_ref()
    }

    /// Take the observer
    pub fn take_observer(&mut self) -> Option<ProtocolTestObserver> {
        self.observer.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_device_id(index: u8) -> DeviceId {
        let mut bytes = [0u8; 32];
        // Use byte 0 since new_from_entropy only uses first 16 bytes
        bytes[0] = index;
        DeviceId::new_from_entropy(bytes)
    }

    #[test]
    fn test_protocol_test_builder() {
        let device1 = make_device_id(1);
        let device2 = make_device_id(2);
        let device3 = make_device_id(3);

        let test = ProtocolTest::new("TestProtocol")
            .bind_role("Coordinator", device1)
            .bind_roles("Witness", &[device2, device3])
            .expect_success();

        assert_eq!(test.protocol_name(), "TestProtocol");
        assert!(test.expects_success());
        assert_eq!(test.all_devices().len(), 3);
    }

    #[test]
    fn test_build_harness() {
        let device1 = make_device_id(1);
        let device2 = make_device_id(2);

        let test = ProtocolTest::new("TestProtocol")
            .bind_role("Alice", device1)
            .bind_role("Bob", device2);

        let harness = test.build_harness().expect("Failed to build harness");
        assert_eq!(harness.device_count(), 2);
    }

    // =========================================================================
    // Observer Tests
    // =========================================================================

    #[test]
    fn test_observer_event_tracking() {
        let mut observer = ProtocolTestObserver::new("TestProtocol");

        observer.on_phase_start("Alice", "handshake");
        observer.on_send("Alice", "Bob", "Hello", 64);
        observer.on_receive("Alice", "Bob", "Hello", 64);
        observer.on_choice("Alice", "Continue");
        observer.on_phase_end("Alice", "handshake");
        observer.on_complete();

        let events = observer.events();
        assert_eq!(events.len(), 6);

        // Verify event types
        assert!(matches!(events[0], ProtocolEvent::PhaseStart { .. }));
        assert!(matches!(events[1], ProtocolEvent::MessageSent { .. }));
        assert!(matches!(events[2], ProtocolEvent::MessageReceived { .. }));
        assert!(matches!(events[3], ProtocolEvent::Choice { .. }));
        assert!(matches!(events[4], ProtocolEvent::PhaseEnd { .. }));
        assert!(matches!(events[5], ProtocolEvent::Complete { .. }));
    }

    #[test]
    fn test_observer_statistics() {
        let mut observer = ProtocolTestObserver::new("TestProtocol");

        observer.on_phase_start("Alice", "init");
        observer.on_send("Alice", "Bob", "Request", 128);
        observer.on_receive("Bob", "Alice", "Request", 128);
        observer.on_send("Bob", "Alice", "Response", 256);
        observer.on_receive("Alice", "Bob", "Response", 256);
        observer.on_choice("Alice", "Continue");
        observer.on_phase_end("Alice", "init");
        observer.on_complete();

        let stats = observer.statistics();
        assert_eq!(stats.messages_sent, 2);
        assert_eq!(stats.messages_received, 2);
        assert_eq!(stats.bytes_sent, 384);
        assert_eq!(stats.bytes_received, 384);
        assert_eq!(stats.phases_started, 1);
        assert_eq!(stats.phases_completed, 1);
        assert_eq!(stats.choices_made, 1);
        assert!(stats.total_duration.is_some());
    }

    #[test]
    fn test_observer_take_events() {
        let mut observer = ProtocolTestObserver::new("TestProtocol");

        observer.on_send("Alice", "Bob", "Ping", 32);
        observer.on_complete();

        assert_eq!(observer.events().len(), 2);

        let taken = observer.take_events();
        assert_eq!(taken.len(), 2);
        assert!(observer.events().is_empty());
    }

    // =========================================================================
    // Stepped Participant Tests
    // =========================================================================

    #[test]
    fn test_stepped_participant_creation() {
        let participant = SteppedParticipant::new("Alice");

        assert_eq!(participant.role(), "Alice");
        assert!(matches!(participant.state(), ParticipantState::Ready));
        assert!(!participant.is_complete());
        assert!(!participant.is_failed());
        assert_eq!(participant.step_count(), 0);
    }

    #[test]
    fn test_stepped_participant_message_queue() {
        let mut participant = SteppedParticipant::new("Alice");

        participant.queue_message("Bob", vec![1, 2, 3]);
        participant.queue_output("Bob", vec![4, 5, 6], "Ping");

        let output = participant.take_output();
        assert!(output.is_some());
        let (to, msg, msg_type) = output.unwrap();
        assert_eq!(to, "Bob");
        assert_eq!(msg, vec![4, 5, 6]);
        assert_eq!(msg_type, "Ping");
    }

    #[test]
    fn test_stepped_participant_state_transitions() {
        let mut participant = SteppedParticipant::new("Alice");

        participant.wait_for("Bob");
        assert!(matches!(
            participant.state(),
            ParticipantState::WaitingForMessage { .. }
        ));

        participant.ready_to_send("Bob");
        assert!(matches!(
            participant.state(),
            ParticipantState::ReadyToSend { .. }
        ));

        participant.mark_complete();
        assert!(participant.is_complete());
    }

    #[test]
    fn test_stepped_participant_failure() {
        let mut participant = SteppedParticipant::new("Alice");

        participant.mark_failed("Connection timeout");
        assert!(participant.is_failed());

        if let ParticipantState::Failed { error } = participant.state() {
            assert_eq!(error, "Connection timeout");
        } else {
            panic!("Expected Failed state");
        }
    }

    // =========================================================================
    // Protocol Test Runner Tests
    // =========================================================================

    #[test]
    fn test_runner_creation() {
        let runner = ProtocolTestRunner::new("TestProtocol")
            .add_participant("Alice")
            .add_participant("Bob")
            .with_observer();

        assert_eq!(runner.participant_count(), 2);
        assert!(runner.participant_by_role("Alice").is_some());
        assert!(runner.participant_by_role("Bob").is_some());
        assert!(runner.participant_by_role("Carol").is_none());
        assert!(runner.observer().is_some());
    }

    #[test]
    fn test_runner_step_participant() {
        let mut runner = ProtocolTestRunner::new("TestProtocol")
            .add_participant("Alice")
            .add_participant("Bob");

        // Queue a message from Alice to Bob
        if let Some(alice) = runner.participant_mut(0) {
            alice.queue_output("Bob", vec![1, 2, 3], "Ping");
        }

        // Step Alice - should send
        let result = runner.step_participant(0);
        assert!(result.is_some());

        if let Some(StepResult::Send {
            to, message_type, ..
        }) = result
        {
            assert_eq!(to, "Bob");
            assert_eq!(message_type, "Ping");
        } else {
            panic!("Expected Send result");
        }
    }

    #[test]
    fn test_runner_all_complete() {
        let mut runner = ProtocolTestRunner::new("TestProtocol")
            .add_participant("Alice")
            .add_participant("Bob");

        assert!(!runner.all_complete());

        if let Some(alice) = runner.participant_mut(0) {
            alice.mark_complete();
        }

        assert!(!runner.all_complete());

        if let Some(bob) = runner.participant_mut(1) {
            bob.mark_complete();
        }

        assert!(runner.all_complete());
    }

    #[test]
    fn test_runner_any_failed() {
        let mut runner = ProtocolTestRunner::new("TestProtocol")
            .add_participant("Alice")
            .add_participant("Bob");

        assert!(!runner.any_failed());

        if let Some(alice) = runner.participant_mut(0) {
            alice.mark_failed("Test error");
        }

        assert!(runner.any_failed());
    }

    #[test]
    fn test_runner_with_observer_records_sends() {
        let mut runner = ProtocolTestRunner::new("TestProtocol")
            .add_participant("Alice")
            .add_participant("Bob")
            .with_observer();

        // Queue and send a message
        if let Some(alice) = runner.participant_mut(0) {
            alice.queue_output("Bob", vec![1, 2, 3], "Ping");
        }
        runner.step_participant(0);

        // Check observer recorded the send
        let stats = runner.observer().unwrap().statistics();
        assert_eq!(stats.messages_sent, 1);
        assert_eq!(stats.bytes_sent, 3);
    }

    #[test]
    fn test_runner_total_steps() {
        let mut runner = ProtocolTestRunner::new("TestProtocol")
            .add_participant("Alice")
            .add_participant("Bob");

        assert_eq!(runner.total_steps(), 0);

        runner.step_participant(0);
        assert_eq!(runner.total_steps(), 1);

        runner.step_participant(1);
        assert_eq!(runner.total_steps(), 2);
    }
}
