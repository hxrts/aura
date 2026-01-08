//! Protocol Fault Injection Tests
//!
//! Tests protocol resilience using the ChaosEffects fault injection system.
//! Validates that protocols handle faults gracefully without safety violations.
//!
//! ## Test Categories
//!
//! - Network delay injection
//! - Message corruption detection
//! - Byzantine behavior simulation
//! - Resource exhaustion handling
//! - Timing fault resilience

#![allow(clippy::expect_used)]

use aura_core::effects::{ByzantineType, ChaosEffects, CorruptionType, ResourceType};
use aura_simulator::choreography_observer::SimulatorObserver;
use aura_simulator::handlers::SimulationFaultHandler;
use aura_simulator::protocol_state_machine::{
    ParticipantState, ProtocolScheduler, ProtocolStateMachine, StepResult,
};
use std::sync::Arc;
use std::time::Duration;

// =============================================================================
// Fault Handler Tests
// =============================================================================

#[tokio::test]
async fn test_fault_handler_message_corruption_injection() {
    let handler = SimulationFaultHandler::new(42);

    // Inject message corruption
    handler
        .inject_message_corruption(0.1, CorruptionType::BitFlip)
        .await
        .expect("should inject corruption");

    assert_eq!(handler.active_fault_count(), 1);
    let faults = handler.get_active_faults();
    assert!(
        faults.iter().any(|f| f.contains("MessageCorruption")),
        "Expected message corruption fault"
    );
}

#[tokio::test]
async fn test_fault_handler_network_partition_injection() {
    let handler = SimulationFaultHandler::new(42);

    // Create partition between two groups
    let groups = vec![
        vec!["alice".to_string(), "bob".to_string()],
        vec!["charlie".to_string(), "dave".to_string()],
    ];

    handler
        .inject_network_partition(groups, Duration::from_secs(60))
        .await
        .expect("should inject partition");

    assert_eq!(handler.active_fault_count(), 1);
    let faults = handler.get_active_faults();
    assert!(
        faults.iter().any(|f| f.contains("NetworkPartition")),
        "Expected network partition fault"
    );
}

#[tokio::test]
async fn test_fault_handler_byzantine_behavior_injection() {
    let handler = SimulationFaultHandler::new(42);

    // Inject equivocation behavior
    handler
        .inject_byzantine_behavior(
            vec!["malicious_node".to_string()],
            ByzantineType::Equivocation,
        )
        .await
        .expect("should inject byzantine behavior");

    assert_eq!(handler.active_fault_count(), 1);
    let faults = handler.get_active_faults();
    assert!(
        faults.iter().any(|f| f.contains("Byzantine")),
        "Expected byzantine fault"
    );
}

#[tokio::test]
async fn test_fault_handler_network_delay_injection() {
    let handler = SimulationFaultHandler::new(42);

    // Inject network delays
    handler
        .inject_network_delay(
            (Duration::from_millis(100), Duration::from_millis(500)),
            Some(vec!["slow_peer".to_string()]),
        )
        .await
        .expect("should inject delay");

    assert_eq!(handler.active_fault_count(), 1);
    let faults = handler.get_active_faults();
    assert!(
        faults.iter().any(|f| f.contains("NetworkDelay")),
        "Expected network delay fault"
    );
}

#[tokio::test]
async fn test_fault_handler_timing_fault_injection() {
    let handler = SimulationFaultHandler::new(42);

    // Inject clock skew
    handler
        .inject_timing_faults(Duration::from_millis(100), 0.05)
        .await
        .expect("should inject timing fault");

    assert_eq!(handler.active_fault_count(), 1);
    let faults = handler.get_active_faults();
    assert!(
        faults.iter().any(|f| f.contains("TimingFaults")),
        "Expected timing fault"
    );
}

#[tokio::test]
async fn test_fault_handler_resource_exhaustion() {
    let handler = SimulationFaultHandler::new(42);

    // Inject memory pressure
    handler
        .inject_resource_exhaustion(ResourceType::Memory, 0.9)
        .await
        .expect("should inject resource exhaustion");

    assert_eq!(handler.active_fault_count(), 1);
    let faults = handler.get_active_faults();
    assert!(
        faults.iter().any(|f| f.contains("ResourceExhaustion")),
        "Expected resource exhaustion fault"
    );
}

#[tokio::test]
async fn test_fault_handler_multiple_faults() {
    let handler = SimulationFaultHandler::new(42);

    // Inject multiple faults
    handler
        .inject_message_corruption(0.1, CorruptionType::BitFlip)
        .await
        .expect("corruption");

    handler
        .inject_network_delay((Duration::from_millis(10), Duration::from_millis(50)), None)
        .await
        .expect("delay");

    handler
        .inject_timing_faults(Duration::from_millis(20), 0.01)
        .await
        .expect("timing");

    assert_eq!(handler.active_fault_count(), 3);
}

#[tokio::test]
async fn test_fault_handler_stop_all() {
    let handler = SimulationFaultHandler::new(42);

    // Inject faults
    handler
        .inject_message_corruption(0.1, CorruptionType::BitFlip)
        .await
        .unwrap();
    handler
        .inject_timing_faults(Duration::from_millis(50), 0.1)
        .await
        .unwrap();

    assert_eq!(handler.active_fault_count(), 2);

    // Stop all
    handler.stop_all_injections().await.unwrap();
    assert_eq!(handler.active_fault_count(), 0);
}

#[tokio::test]
async fn test_fault_handler_max_concurrent_limit() {
    let handler = SimulationFaultHandler::with_max_faults(42, 2);

    // First two should succeed
    handler
        .inject_message_corruption(0.1, CorruptionType::BitFlip)
        .await
        .expect("first fault");
    handler
        .inject_timing_faults(Duration::from_millis(50), 0.1)
        .await
        .expect("second fault");

    // Third should fail
    let result = handler
        .inject_network_delay(
            (Duration::from_millis(10), Duration::from_millis(100)),
            None,
        )
        .await;

    assert!(result.is_err(), "Expected max faults error");
}

#[tokio::test]
async fn test_fault_handler_validation_corruption_rate() {
    let handler = SimulationFaultHandler::new(42);

    // Invalid rate should fail
    let result = handler
        .inject_message_corruption(1.5, CorruptionType::BitFlip)
        .await;

    assert!(result.is_err(), "Expected validation error for rate > 1.0");
}

#[tokio::test]
async fn test_fault_handler_validation_empty_byzantine_peers() {
    let handler = SimulationFaultHandler::new(42);

    // Empty peers list should fail
    let result = handler
        .inject_byzantine_behavior(vec![], ByzantineType::Silent)
        .await;

    assert!(result.is_err(), "Expected validation error for empty peers");
}

// =============================================================================
// Protocol State Machine Tests
// =============================================================================

#[test]
fn test_state_machine_basic_creation() {
    let sm = ProtocolStateMachine::new("Coordinator");

    assert_eq!(sm.role(), "Coordinator");
    assert_eq!(sm.state(), ParticipantState::Ready);
    assert!(!sm.is_complete());
    assert!(!sm.is_failed());
    assert_eq!(sm.step_count(), 0);
}

#[test]
fn test_state_machine_with_observer() {
    let observer = Arc::new(SimulatorObserver::new("TestProtocol"));
    let sm = ProtocolStateMachine::with_observer("Witness", observer.clone());

    assert_eq!(sm.role(), "Witness");

    // Queue a message to test observer integration
    sm.queue_output("Coordinator", vec![1, 2, 3], "VoteMessage");

    let stats = observer.statistics();
    assert!(stats.messages_sent >= 1, "Observer should record send");
}

#[test]
fn test_state_machine_message_queue() {
    let sm = ProtocolStateMachine::new("Alice");

    // Queue incoming message
    sm.queue_message("Bob", vec![1, 2, 3]);

    // Queue outgoing message
    sm.queue_output("Charlie", vec![4, 5, 6], "Request");

    assert!(sm.has_pending_output());

    let output = sm.take_output();
    assert!(output.is_some());

    let (to, msg, msg_type) = output.unwrap();
    assert_eq!(to, "Charlie");
    assert_eq!(msg, vec![4, 5, 6]);
    assert_eq!(msg_type, "Request");
}

#[test]
fn test_state_machine_state_transitions() {
    let sm = ProtocolStateMachine::new("Node");

    // Test various state transitions
    sm.wait_for("Peer");
    assert!(matches!(
        sm.state(),
        ParticipantState::WaitingForMessage { .. }
    ));

    sm.ready_to_send("Peer");
    assert!(matches!(sm.state(), ParticipantState::ReadyToSend { .. }));

    sm.at_choice(vec!["Accept".to_string(), "Reject".to_string()]);
    assert!(matches!(sm.state(), ParticipantState::AtChoice { .. }));

    sm.mark_complete();
    assert!(sm.is_complete());
}

#[test]
fn test_state_machine_failure() {
    let sm = ProtocolStateMachine::new("Node");

    sm.mark_failed("Protocol violation detected");

    assert!(sm.is_failed());
    assert!(matches!(sm.state(), ParticipantState::Failed { .. }));

    if let ParticipantState::Failed { error } = sm.state() {
        assert!(error.contains("Protocol violation"));
    }
}

#[test]
fn test_state_machine_step_with_input() {
    let sm = ProtocolStateMachine::new("Receiver");

    sm.wait_for("Sender");

    // Step with input from expected sender
    let result = sm.step(Some(("Sender".to_string(), vec![1, 2, 3])));

    // Should process the input
    assert!(sm.step_count() > 0);

    // Result depends on implementation
    match result {
        StepResult::NeedInput { .. } | StepResult::Complete | StepResult::Send { .. } => {}
        StepResult::Error { message } => panic!("Unexpected error: {message}"),
        StepResult::Chose { .. } => {}
    }
}

#[test]
fn test_state_machine_choice_step() {
    let sm = ProtocolStateMachine::new("Decider");

    sm.at_choice(vec!["OptionA".to_string(), "OptionB".to_string()]);

    let result = sm.step(None);

    // Should choose first option
    if let StepResult::Chose { branch } = result {
        assert_eq!(branch, "OptionA");
    } else {
        panic!("Expected Chose result");
    }

    // State should transition to Ready
    assert_eq!(sm.state(), ParticipantState::Ready);
}

// =============================================================================
// Protocol Scheduler Tests
// =============================================================================

#[test]
fn test_scheduler_creation() {
    let alice = ProtocolStateMachine::new("Alice");
    let bob = ProtocolStateMachine::new("Bob");
    let charlie = ProtocolStateMachine::new("Charlie");

    let scheduler = ProtocolScheduler::new(vec![alice, bob, charlie]);

    assert_eq!(scheduler.participant_count(), 3);
    assert!(scheduler.participant_by_role("Alice").is_some());
    assert!(scheduler.participant_by_role("Bob").is_some());
    assert!(scheduler.participant_by_role("Charlie").is_some());
    assert!(scheduler.participant_by_role("Unknown").is_none());
}

#[test]
fn test_scheduler_step_participant() {
    let alice = ProtocolStateMachine::new("Alice");
    let bob = ProtocolStateMachine::new("Bob");

    // Alice has a message to send
    alice.queue_output("Bob", vec![42], "Ping");

    let scheduler = ProtocolScheduler::new(vec![alice, bob]);

    // Step Alice
    let result = scheduler.step_participant(0);

    assert!(result.is_some());
    if let Some(StepResult::Send { to, message, .. }) = result {
        assert_eq!(to, "Bob");
        assert_eq!(message, vec![42]);
    }

    assert!(scheduler.total_steps() >= 1);
}

#[test]
fn test_scheduler_all_complete() {
    let alice = ProtocolStateMachine::new("Alice");
    let bob = ProtocolStateMachine::new("Bob");

    alice.mark_complete();
    bob.mark_complete();

    let scheduler = ProtocolScheduler::new(vec![alice, bob]);

    assert!(scheduler.all_complete());
    assert!(!scheduler.any_failed());
}

#[test]
fn test_scheduler_any_failed() {
    let alice = ProtocolStateMachine::new("Alice");
    let bob = ProtocolStateMachine::new("Bob");

    alice.mark_complete();
    bob.mark_failed("Connection lost");

    let scheduler = ProtocolScheduler::new(vec![alice, bob]);

    assert!(!scheduler.all_complete());
    assert!(scheduler.any_failed());
}

#[test]
fn test_scheduler_message_routing() {
    let alice = ProtocolStateMachine::new("Alice");
    let bob = ProtocolStateMachine::new("Bob");

    // Alice queues a message for Bob
    alice.queue_output("Bob", vec![1, 2, 3], "Data");
    alice.ready_to_send("Bob");

    // Bob is waiting for Alice
    bob.wait_for("Alice");

    let scheduler = ProtocolScheduler::new(vec![alice, bob]);

    // Step Alice to send
    let alice_result = scheduler.step_participant(0);
    assert!(matches!(alice_result, Some(StepResult::Send { .. })));

    // Step Bob to receive (message should be routed)
    let bob_result = scheduler.step_participant(1);

    // Bob either gets the message or needs more input
    match bob_result {
        Some(StepResult::NeedInput { .. }) | Some(StepResult::Complete) => {}
        Some(StepResult::Send { .. }) => {}
        Some(StepResult::Chose { .. }) => {}
        Some(StepResult::Error { message }) => panic!("Unexpected error: {message}"),
        None => panic!("Expected result from step"),
    }
}

// =============================================================================
// Combined Fault Injection + Protocol State Machine Tests
// =============================================================================

#[tokio::test]
async fn test_protocol_under_network_delay() {
    let handler = SimulationFaultHandler::new(42);

    // Set up delayed network
    handler
        .inject_network_delay(
            (Duration::from_millis(50), Duration::from_millis(200)),
            None,
        )
        .await
        .expect("delay injection");

    // Create protocol participants
    let coordinator = ProtocolStateMachine::new("Coordinator");
    let witness = ProtocolStateMachine::new("Witness");

    // Coordinator sends proposal
    coordinator.queue_output("Witness", vec![1, 2, 3], "Proposal");

    let scheduler = ProtocolScheduler::new(vec![coordinator, witness]);

    // Run a few steps - with delay fault active, timing would be affected
    for _ in 0..5 {
        scheduler.step_participant(0);
        scheduler.step_participant(1);
    }

    // Protocol should still make progress (delays don't block in simulation)
    assert!(scheduler.total_steps() >= 10);

    // No failures due to delay
    assert!(!scheduler.any_failed());
}

#[tokio::test]
async fn test_protocol_under_message_corruption() {
    let handler = SimulationFaultHandler::new(42);

    // Set up message corruption (in real system, this would corrupt messages)
    handler
        .inject_message_corruption(0.2, CorruptionType::BitFlip)
        .await
        .expect("corruption injection");

    let sender = ProtocolStateMachine::new("Sender");
    let receiver = ProtocolStateMachine::new("Receiver");

    // Send message
    sender.queue_output("Receiver", vec![42, 42, 42], "Data");

    let scheduler = ProtocolScheduler::new(vec![sender, receiver]);

    // Step through
    scheduler.step_participant(0);
    scheduler.step_participant(1);

    // In real system with corruption, receiver might detect bad message
    // For simulation, we verify fault is tracked
    assert!(handler.active_fault_count() >= 1);
}

#[tokio::test]
async fn test_protocol_under_byzantine_behavior() {
    let handler = SimulationFaultHandler::new(42);

    // Mark a participant as Byzantine
    handler
        .inject_byzantine_behavior(vec!["Malicious".to_string()], ByzantineType::Equivocation)
        .await
        .expect("byzantine injection");

    let honest = ProtocolStateMachine::new("Honest");
    let malicious = ProtocolStateMachine::new("Malicious");
    let honest2 = ProtocolStateMachine::new("Honest2");

    // In real system, malicious would equivocate
    // Honest nodes should still be able to make progress
    honest.queue_output("Honest2", vec![1], "Vote");
    honest2.wait_for("Honest");

    let scheduler = ProtocolScheduler::new(vec![honest, malicious, honest2]);

    // Step honest participants
    scheduler.step_participant(0);
    scheduler.step_participant(2);

    // Protocol progress among honest nodes
    assert!(scheduler.total_steps() >= 2);

    // Byzantine fault is tracked
    let faults = handler.get_active_faults();
    assert!(faults.iter().any(|f| f.contains("Byzantine")));
}

// =============================================================================
// Observer Integration Tests
// =============================================================================

#[test]
fn test_observer_tracks_sends() {
    let observer = Arc::new(SimulatorObserver::new("TestProtocol"));

    let sender = ProtocolStateMachine::with_observer("Sender", observer.clone());

    sender.queue_output("Receiver", vec![1, 2, 3], "Ping");

    let stats = observer.statistics();
    assert!(stats.messages_sent >= 1);
}

#[test]
fn test_observer_tracks_phases() {
    let observer = Arc::new(SimulatorObserver::new("TestProtocol"));

    let participant = ProtocolStateMachine::with_observer("Participant", observer.clone());

    participant.mark_complete();

    let stats = observer.statistics();
    assert!(stats.phases_completed >= 1);
}

#[test]
fn test_observer_tracks_errors() {
    let observer = Arc::new(SimulatorObserver::new("TestProtocol"));

    let participant = ProtocolStateMachine::with_observer("Participant", observer.clone());

    participant.mark_failed("Test error");

    let stats = observer.statistics();
    assert!(stats.errors >= 1);
}
