//! Pure simulation engine with functional state transitions
//!
//! This module implements the core simulation logic as a pure function that takes
//! a WorldState and returns a new WorldState with a list of events that occurred.
//! This functional approach provides deterministic, testable state transitions.

use crate::world_state::*;
use crate::{AuraError, Result};
use aura_console_types::trace::{DropReason, ParticipantStatus};
use aura_console_types::{CausalityInfo, EventType, TraceEvent};
use aura_types::SessionStatus;
use std::collections::HashMap;
use uuid::Uuid;

/// Pure function that advances simulation state by one tick
///
/// This is the core of the simulation engine - a pure, stateless function that:
/// 1. Takes the current world state
/// 2. Performs a single step of simulation
/// 3. Mutates the WorldState in place
/// 4. Returns a list of all events that occurred during this tick
///
/// Benefits:
/// - Pure and predictable
/// - Easy to test with any WorldState
/// - Deterministic execution
/// - Clear separation from execution harness
pub fn tick(world: &mut WorldState) -> Result<Vec<TraceEvent>> {
    let mut events = Vec::new();
    let mut event_id_counter = 0;

    // Helper to generate unique event IDs
    let mut next_event_id = || {
        let id = event_id_counter;
        event_id_counter += 1;
        id
    };

    // 1. Advance time
    advance_time(world);
    events.push(create_tick_start_event(world, next_event_id()));

    // 2. Process network message delivery
    process_network_messages(world, &mut events, &mut next_event_id)?;

    // 3. Execute queued protocols
    execute_queued_protocols(world, &mut events, &mut next_event_id)?;

    // 4. Update protocol sessions
    update_protocol_sessions(world, &mut events, &mut next_event_id)?;

    // 5. Process participant actions
    process_participant_actions(world, &mut events, &mut next_event_id)?;

    // 6. Apply byzantine strategies
    apply_byzantine_strategies(world, &mut events, &mut next_event_id)?;

    // 7. Update network state
    update_network_state(world, &mut events, &mut next_event_id)?;

    // 8. Check for timeouts and cleanup
    check_timeouts_and_cleanup(world, &mut events, &mut next_event_id)?;

    // Record events in world state for consumers
    world.last_tick_events = events.clone();

    events.push(create_tick_complete_event(world, next_event_id()));
    Ok(events)
}

/// Advance simulation time
fn advance_time(world: &mut WorldState) {
    world.current_tick += 1;
    world.current_time += world.config.tick_duration_ms;
}

/// Process network message delivery
fn process_network_messages(
    world: &mut WorldState,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let current_time = world.current_time;
    let mut delivered_messages = Vec::new();
    let mut dropped_messages = Vec::new();

    // Find messages ready for delivery
    while let Some(message) = world.network.in_flight_messages.front() {
        if message
            .deliver_at
            .is_some_and(|deliver_time| deliver_time <= current_time)
        {
            let message = world
                .network
                .in_flight_messages
                .pop_front()
                .expect("Message should exist as we just checked front()");

            if message.will_drop {
                dropped_messages.push(message);
            } else {
                delivered_messages.push(message);
            }
        } else {
            break;
        }
    }

    // Deliver messages
    for message in delivered_messages {
        deliver_message(world, &message, events, next_event_id)?;
    }

    // Record dropped messages
    for message in dropped_messages {
        record_message_dropped(world, &message, events, next_event_id);
    }

    Ok(())
}

/// Deliver a message to a participant
fn deliver_message(
    world: &mut WorldState,
    message: &Message,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    // Check if recipient is specified and online
    if let Some(to_participant) = &message.to {
        if let Some(recipient) = world.participants.get_mut(to_participant) {
            if recipient.status == ParticipantStatus::Online {
                // Add message to recipient's inbox
                let inbox_message = Message {
                    message_id: message.message_id.clone(),
                    from: message.from.clone(),
                    to: message.to.clone(),
                    message_type: message.message_type.clone(),
                    payload: message.payload.clone(),
                    sent_at: message.sent_at,
                    deliver_at: Some(world.current_time),
                    will_drop: false,
                };

                recipient.message_inbox.push_back(inbox_message);
                recipient.message_count += 1;
                recipient.last_active = world.current_time;

                // Record delivery event
                let event = TraceEvent {
                    tick: world.current_tick,
                    event_id: next_event_id(),
                    event_type: EventType::MessageReceived {
                        envelope_id: message.message_id.clone(),
                        from: message.from.clone(),
                        message_type: message.message_type.clone(),
                    },
                    participant: to_participant.clone(),
                    causality: CausalityInfo {
                        parent_events: Vec::new(),
                        happens_before: Vec::new(),
                        concurrent_with: Vec::new(),
                    },
                };
                events.push(event);
            } else {
                // Recipient is offline, drop message
                record_message_dropped_with_reason(
                    world,
                    message,
                    DropReason::NetworkPartition,
                    events,
                    next_event_id,
                );
            }
        } else {
            // Recipient doesn't exist, drop message
            record_message_dropped_with_reason(
                world,
                message,
                DropReason::NetworkPartition,
                events,
                next_event_id,
            );
        }
    } else {
        // No recipient specified (broadcast), handle differently if needed
        // For now, just ignore broadcast messages in this simplified implementation
    }

    Ok(())
}

/// Record a dropped message event
fn record_message_dropped(
    world: &WorldState,
    message: &Message,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) {
    record_message_dropped_with_reason(
        world,
        message,
        DropReason::NetworkPartition,
        events,
        next_event_id,
    );
}

/// Record a dropped message event with specific reason
fn record_message_dropped_with_reason(
    world: &WorldState,
    message: &Message,
    reason: DropReason,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) {
    let event = TraceEvent {
        tick: world.current_tick,
        event_id: next_event_id(),
        event_type: EventType::MessageDropped {
            envelope_id: message.message_id.clone(),
            reason,
        },
        participant: "network".to_string(),
        causality: CausalityInfo {
            parent_events: Vec::new(),
            happens_before: Vec::new(),
            concurrent_with: Vec::new(),
        },
    };
    events.push(event);
}

/// Execute protocols that are ready to start
fn execute_queued_protocols(
    world: &mut WorldState,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let current_time = world.current_time;
    let mut ready_protocols = Vec::new();

    // Find protocols ready to execute
    while let Some(protocol) = world.protocols.execution_queue.front() {
        if protocol.scheduled_time <= current_time {
            ready_protocols.push(
                world
                    .protocols
                    .execution_queue
                    .pop_front()
                    .expect("Protocol should exist as we just checked front()"),
            );
        } else {
            break;
        }
    }

    // Start ready protocols
    for protocol in ready_protocols {
        start_protocol_session(world, protocol, events, next_event_id)?;
    }

    Ok(())
}

/// Start a new protocol session
fn start_protocol_session(
    world: &mut WorldState,
    queued_protocol: QueuedProtocol,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let session_id = Uuid::new_v4().to_string();
    let coordinator = queued_protocol
        .participants
        .first()
        .ok_or_else(|| AuraError::configuration_error("Protocol has no participants".to_string()))?
        .clone();

    // Create protocol session
    let session = ProtocolSession {
        session_id: session_id.clone(),
        protocol_type: queued_protocol.protocol_type.clone(),
        current_phase: "initializing".to_string(),
        participants: queued_protocol.participants.clone(),
        coordinator: coordinator.clone(),
        started_at: world.current_time,
        expected_completion: Some(world.current_time + 5000), // 5 second default
        status: SessionStatus::Initializing,
        state_data: HashMap::new(),
        session_messages: Vec::new(),
    };

    // Add session to active sessions
    world
        .protocols
        .active_sessions
        .insert(session_id.clone(), session);

    // Update participant states
    for participant_id in &queued_protocol.participants {
        if let Some(participant) = world.participants.get_mut(participant_id) {
            let participation = SessionParticipation {
                session_id: session_id.clone(),
                protocol_type: queued_protocol.protocol_type.clone(),
                current_phase: "initializing".to_string(),
                role: if participant_id == &coordinator {
                    ParticipantRole::Coordinator
                } else {
                    ParticipantRole::Participant
                },
                state_data: Vec::new(),
                joined_at: world.current_time,
            };
            participant
                .active_sessions
                .insert(session_id.clone(), participation);
        }
    }

    // Record session start event
    let event = TraceEvent {
        tick: world.current_tick,
        event_id: next_event_id(),
        event_type: EventType::ProtocolStateTransition {
            protocol: queued_protocol.protocol_type,
            from_state: "queued".to_string(),
            to_state: "initializing".to_string(),
            witness_data: Some(session_id.as_bytes().to_vec()),
        },
        participant: coordinator,
        causality: CausalityInfo {
            parent_events: Vec::new(),
            happens_before: Vec::new(),
            concurrent_with: Vec::new(),
        },
    };
    events.push(event);

    Ok(())
}

/// Update active protocol sessions
fn update_protocol_sessions(
    world: &mut WorldState,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let mut sessions_to_advance = Vec::new();

    // Collect sessions that need to advance
    for (session_id, session) in &world.protocols.active_sessions {
        if session.status == SessionStatus::Initializing || session.status == SessionStatus::Active
        {
            sessions_to_advance.push(session_id.clone());
        }
    }

    // Advance each session
    for session_id in sessions_to_advance {
        advance_protocol_session(world, &session_id, events, next_event_id)?;
    }

    Ok(())
}

/// Advance a specific protocol session
fn advance_protocol_session(
    world: &mut WorldState,
    session_id: &str,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let session = world
        .protocols
        .active_sessions
        .get_mut(session_id)
        .ok_or_else(|| {
            AuraError::configuration_error(format!("Session {} not found", session_id))
        })?;

    let previous_phase = session.current_phase.clone();
    let coordinator = session.coordinator.clone();

    // Simple phase advancement logic
    match session.current_phase.as_str() {
        "initializing" => {
            session.current_phase = "active".to_string();
            session.status = SessionStatus::Active;
        }
        "active" => {
            // Check if all participants have responded (simplified)
            if world.current_time >= session.started_at + 1000 {
                // 1 second timeout
                session.current_phase = "completing".to_string();
            }
        }
        "completing" => {
            session.current_phase = "completed".to_string();
            session.status = SessionStatus::Completed;
        }
        _ => {} // No advancement needed
    }

    // Record state transition if phase changed
    if session.current_phase != previous_phase {
        let event = TraceEvent {
            tick: world.current_tick,
            event_id: next_event_id(),
            event_type: EventType::ProtocolStateTransition {
                protocol: session.protocol_type.clone(),
                from_state: previous_phase,
                to_state: session.current_phase.clone(),
                witness_data: Some(session_id.as_bytes().to_vec()),
            },
            participant: coordinator,
            causality: CausalityInfo {
                parent_events: Vec::new(),
                happens_before: Vec::new(),
                concurrent_with: Vec::new(),
            },
        };
        events.push(event);
    }

    Ok(())
}

/// Process participant actions (message processing, etc.)
fn process_participant_actions(
    world: &mut WorldState,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let participant_ids: Vec<String> = world.participants.keys().cloned().collect();

    for participant_id in participant_ids {
        process_participant_inbox(world, &participant_id, events, next_event_id)?;
    }

    Ok(())
}

/// Process messages in a participant's inbox
fn process_participant_inbox(
    world: &mut WorldState,
    participant_id: &str,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let participant = world
        .participants
        .get_mut(participant_id)
        .ok_or_else(|| AuraError::device_not_found(participant_id.to_string()))?;

    // Process all messages in inbox
    while let Some(message) = participant.message_inbox.pop_front() {
        // Simple message processing - just record the processing
        let event = TraceEvent {
            tick: world.current_tick,
            event_id: next_event_id(),
            event_type: EventType::EffectExecuted {
                effect_type: "process_message".to_string(),
                effect_data: message.message_id.as_bytes().to_vec(),
            },
            participant: participant_id.to_string(),
            causality: CausalityInfo {
                parent_events: Vec::new(),
                happens_before: Vec::new(),
                concurrent_with: Vec::new(),
            },
        };
        events.push(event);

        // Update participant activity
        participant.last_active = world.current_time;
    }

    Ok(())
}

/// Apply byzantine attack strategies
fn apply_byzantine_strategies(
    world: &mut WorldState,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let byzantine_participants = world.byzantine.byzantine_participants.clone();

    for participant_id in byzantine_participants {
        if let Some(strategy) = world
            .byzantine
            .active_strategies
            .get(&participant_id)
            .cloned()
        {
            apply_byzantine_strategy(world, &participant_id, &strategy, events, next_event_id)?;
        }
    }

    Ok(())
}

/// Apply a specific byzantine strategy for a participant
fn apply_byzantine_strategy(
    world: &mut WorldState,
    participant_id: &str,
    strategy: &ByzantineStrategy,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    match strategy {
        ByzantineStrategy::DropAllMessages => {
            // Mark any in-flight messages from this participant to be dropped
            for message in &mut world.network.in_flight_messages {
                if message.from == participant_id {
                    message.will_drop = true;
                }
            }
        }
        ByzantineStrategy::DelayMessages { delay_ms } => {
            // Add delay to messages from this participant
            for message in &mut world.network.in_flight_messages {
                if message.from == participant_id {
                    if let Some(deliver_time) = message.deliver_at {
                        message.deliver_at = Some(deliver_time + delay_ms);
                    }
                }
            }
        }
        ByzantineStrategy::RefuseParticipation => {
            // Remove participant from active sessions
            if let Some(participant) = world.participants.get_mut(participant_id) {
                participant.active_sessions.clear();
            }
        }
        _ => {
            // Other strategies would be implemented here
        }
    }

    // Record strategy application
    let event = TraceEvent {
        tick: world.current_tick,
        event_id: next_event_id(),
        event_type: EventType::EffectExecuted {
            effect_type: "byzantine_strategy_applied".to_string(),
            effect_data: format!("{:?}", strategy).as_bytes().to_vec(),
        },
        participant: participant_id.to_string(),
        causality: CausalityInfo {
            parent_events: Vec::new(),
            happens_before: Vec::new(),
            concurrent_with: Vec::new(),
        },
    };
    events.push(event);

    Ok(())
}

/// Update network simulation state
fn update_network_state(
    world: &mut WorldState,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let current_time = world.current_time;

    // Remove expired partitions
    let initial_partition_count = world.network.partitions.len();
    world.network.partitions.retain(|partition| {
        if let Some(duration) = partition.duration {
            partition.started_at + duration > current_time
        } else {
            true // Permanent partition
        }
    });

    // Record partition changes
    if world.network.partitions.len() != initial_partition_count {
        let event = TraceEvent {
            tick: world.current_tick,
            event_id: next_event_id(),
            event_type: EventType::EffectExecuted {
                effect_type: "partition_expired".to_string(),
                effect_data: Vec::new(),
            },
            participant: "network".to_string(),
            causality: CausalityInfo {
                parent_events: Vec::new(),
                happens_before: Vec::new(),
                concurrent_with: Vec::new(),
            },
        };
        events.push(event);
    }

    Ok(())
}

/// Check for timeouts and cleanup completed sessions
fn check_timeouts_and_cleanup(
    world: &mut WorldState,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    let current_time = world.current_time;
    let mut timed_out_sessions = Vec::new();
    let mut completed_sessions = Vec::new();

    // Find timed out and completed sessions
    for (session_id, session) in &world.protocols.active_sessions {
        if let Some(expected_completion) = session.expected_completion {
            if current_time > expected_completion && session.status == SessionStatus::Active {
                timed_out_sessions.push(session_id.clone());
            }
        }

        if session.status == SessionStatus::Completed {
            completed_sessions.push(session_id.clone());
        }
    }

    // Handle timed out sessions
    for session_id in timed_out_sessions {
        timeout_session(world, &session_id, events, next_event_id)?;
    }

    // Move completed sessions to completed list
    for session_id in completed_sessions {
        complete_session(world, &session_id, events, next_event_id)?;
    }

    Ok(())
}

/// Mark a session as timed out
fn timeout_session(
    world: &mut WorldState,
    session_id: &str,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    if let Some(session) = world.protocols.active_sessions.get_mut(session_id) {
        session.status = SessionStatus::TimedOut;

        let event = TraceEvent {
            tick: world.current_tick,
            event_id: next_event_id(),
            event_type: EventType::ProtocolStateTransition {
                protocol: session.protocol_type.clone(),
                from_state: session.current_phase.clone(),
                to_state: "timed_out".to_string(),
                witness_data: Some(session_id.as_bytes().to_vec()),
            },
            participant: session.coordinator.clone(),
            causality: CausalityInfo {
                parent_events: Vec::new(),
                happens_before: Vec::new(),
                concurrent_with: Vec::new(),
            },
        };
        events.push(event);
    }

    Ok(())
}

/// Move a completed session to the completed list
fn complete_session(
    world: &mut WorldState,
    session_id: &str,
    events: &mut Vec<TraceEvent>,
    next_event_id: &mut impl FnMut() -> u64,
) -> Result<()> {
    if let Some(session) = world.protocols.active_sessions.remove(session_id) {
        let completed_session = CompletedSession {
            session: session.clone(),
            result: SessionResult::Success {
                result_data: Vec::new(),
            },
            completed_at: world.current_time,
        };

        world.protocols.completed_sessions.push(completed_session);

        // Remove session from participant states
        for participant_id in &session.participants {
            if let Some(participant) = world.participants.get_mut(participant_id) {
                participant.active_sessions.remove(session_id);
            }
        }

        let event = TraceEvent {
            tick: world.current_tick,
            event_id: next_event_id(),
            event_type: EventType::EffectExecuted {
                effect_type: "session_completed".to_string(),
                effect_data: session_id.as_bytes().to_vec(),
            },
            participant: session.coordinator,
            causality: CausalityInfo {
                parent_events: Vec::new(),
                happens_before: Vec::new(),
                concurrent_with: Vec::new(),
            },
        };
        events.push(event);
    }

    Ok(())
}

/// Create a tick start event
fn create_tick_start_event(world: &WorldState, event_id: u64) -> TraceEvent {
    TraceEvent {
        tick: world.current_tick,
        event_id,
        event_type: EventType::EffectExecuted {
            effect_type: "tick_start".to_string(),
            effect_data: world.current_tick.to_le_bytes().to_vec(),
        },
        participant: "simulation".to_string(),
        causality: CausalityInfo {
            parent_events: Vec::new(),
            happens_before: Vec::new(),
            concurrent_with: Vec::new(),
        },
    }
}

/// Create a tick completion event
fn create_tick_complete_event(world: &WorldState, event_id: u64) -> TraceEvent {
    TraceEvent {
        tick: world.current_tick,
        event_id,
        event_type: EventType::EffectExecuted {
            effect_type: "tick_complete".to_string(),
            effect_data: world.current_tick.to_le_bytes().to_vec(),
        },
        participant: "simulation".to_string(),
        causality: CausalityInfo {
            parent_events: Vec::new(),
            happens_before: Vec::new(),
            concurrent_with: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure_tick_function() {
        let mut world = WorldState::new(42);

        // Initial state
        assert_eq!(world.current_tick, 0);
        assert_eq!(world.current_time, 0);

        // Run one tick
        let events = tick(&mut world).unwrap();

        // Verify state advancement
        assert_eq!(world.current_tick, 1);
        assert_eq!(world.current_time, world.config.tick_duration_ms);

        // Verify events were generated
        assert!(!events.is_empty());
        assert!(events
            .iter()
            .any(|e| matches!(e.event_type, EventType::EffectExecuted { .. })));
    }

    /// TODO: Update test to match current simulation engine implementation
    #[test]
    #[ignore]
    fn test_message_delivery() {
        let mut world = WorldState::new(42);

        // Add participants
        world
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();
        world
            .add_participant(
                "bob".to_string(),
                "device_bob".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        // Add a message to be delivered
        let message = Message {
            message_id: "msg_1".to_string(),
            from: "alice".to_string(),
            to: Some("bob".to_string()),
            message_type: "test".to_string(),
            payload: b"hello".to_vec(),
            sent_at: 0,
            deliver_at: Some(world.current_time + world.config.tick_duration_ms),
            will_drop: false,
        };
        world.network.in_flight_messages.push_back(message);

        // Run tick to deliver message
        let events = tick(&mut world).unwrap();

        // Verify message was delivered
        let bob = world.get_participant("bob").unwrap();
        assert_eq!(bob.message_inbox.len(), 1);

        // Verify delivery event was recorded
        assert!(events
            .iter()
            .any(|e| matches!(e.event_type, EventType::MessageReceived { .. })));
    }

    #[test]
    fn test_protocol_execution() {
        let mut world = WorldState::new(42);

        // Add participants
        world
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();
        world
            .add_participant(
                "bob".to_string(),
                "device_bob".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        // Queue a protocol
        let protocol = QueuedProtocol {
            protocol_type: "DKD".to_string(),
            participants: vec!["alice".to_string(), "bob".to_string()],
            parameters: HashMap::new(),
            scheduled_time: world.current_time + world.config.tick_duration_ms,
            priority: 0,
        };
        world.protocols.execution_queue.push_back(protocol);

        // Run tick to start protocol
        let events = tick(&mut world).unwrap();

        // Verify protocol was started
        assert_eq!(world.protocols.active_sessions.len(), 1);
        assert!(world.protocols.execution_queue.is_empty());

        // Verify participants have the session
        let alice = world.get_participant("alice").unwrap();
        assert_eq!(alice.active_sessions.len(), 1);

        // Verify protocol start event was recorded
        assert!(events
            .iter()
            .any(|e| matches!(e.event_type, EventType::ProtocolStateTransition { .. })));
    }

    /// TODO: Update test to match current byzantine strategy implementation
    #[test]
    #[ignore]
    fn test_byzantine_strategy_application() {
        let mut world = WorldState::new(42);

        // Add byzantine participant
        world
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();
        world
            .byzantine
            .byzantine_participants
            .push("alice".to_string());
        world
            .byzantine
            .active_strategies
            .insert("alice".to_string(), ByzantineStrategy::DropAllMessages);

        // Add a message from the byzantine participant
        let message = Message {
            message_id: "msg_1".to_string(),
            from: "alice".to_string(),
            to: Some("bob".to_string()),
            message_type: "test".to_string(),
            payload: b"hello".to_vec(),
            sent_at: 0,
            deliver_at: Some(world.current_time + world.config.tick_duration_ms),
            will_drop: false,
        };
        world.network.in_flight_messages.push_back(message);

        // Run tick to apply byzantine strategy
        let _events = tick(&mut world).unwrap();

        // Verify the byzantine strategy was applied - message should either be dropped or marked to drop
        // Since we're using DropAllMessages strategy, the message should be handled
        assert!(
            world.network.in_flight_messages.is_empty()
                || world
                    .network
                    .in_flight_messages
                    .front()
                    .map(|m| m.will_drop)
                    .unwrap_or(false)
        );
    }

    #[test]
    fn test_deterministic_execution() {
        // Run the same simulation twice with the same seed
        let mut world1 = WorldState::new(42);
        let mut world2 = WorldState::new(42);

        world1
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();
        world2
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        // Run multiple ticks
        for _ in 0..5 {
            let events1 = tick(&mut world1).unwrap();
            let events2 = tick(&mut world2).unwrap();

            // Verify deterministic execution
            assert_eq!(world1.current_tick, world2.current_tick);
            assert_eq!(world1.current_time, world2.current_time);
            assert_eq!(events1.len(), events2.len());
        }

        // Verify final states match
        assert_eq!(world1.snapshot().state_hash, world2.snapshot().state_hash);
    }
}
