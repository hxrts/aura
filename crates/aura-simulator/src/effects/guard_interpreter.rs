//! Simulation Effect Interpreter
//!
//! This module provides a deterministic implementation of the `EffectInterpreter` trait
//! for simulation and testing. It records all effect commands as events and maintains
//! deterministic state that can be inspected and replayed.
//!
//! Per ADR-014, this interpreter executes effect commands in a controlled environment,
//! enabling deterministic simulation, replay capabilities, and comprehensive testing
//! of distributed protocols.

use async_trait::async_trait;
use aura_core::{
    effects::{
        guard::{
            EffectCommand, EffectInterpreter, EffectResult, JournalEntry, SimulationEvent,
        },
        NetworkAddress,
    },
    identifiers::{AuthorityId, ContextId},
    time::TimeStamp,
    AuraError, AuraResult as Result,
};
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};
use tracing::{debug, info};

/// Deterministic state for simulation
#[derive(Debug, Clone)]
pub struct SimulationState {
    /// Flow budgets by authority
    pub flow_budgets: HashMap<AuthorityId, u32>,
    /// Journal entries in order
    pub journal: Vec<JournalEntry>,
    /// Metadata storage
    pub metadata: HashMap<String, String>,
    /// Total metadata leakage bits
    pub total_leakage_bits: u32,
    /// Queued network messages
    pub message_queue: Vec<QueuedMessage>,
    /// Current simulation time
    pub current_time: TimeStamp,
    /// Random number generator for deterministic nonce generation
    pub rng: ChaCha8Rng,
    /// All events in chronological order
    pub events: Vec<SimulationEvent>,
}

/// Message queued for network delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    /// Source address
    pub from: NetworkAddress,
    /// Destination address
    pub to: NetworkAddress,
    /// Message envelope
    pub envelope: Vec<u8>,
    /// Time when queued
    pub timestamp: TimeStamp,
}

impl SimulationState {
    /// Create new simulation state with seed for deterministic RNG
    pub fn new(seed: u64, initial_time: TimeStamp) -> Self {
        Self {
            flow_budgets: HashMap::new(),
            journal: Vec::new(),
            metadata: HashMap::new(),
            total_leakage_bits: 0,
            message_queue: Vec::new(),
            current_time: initial_time,
            rng: ChaCha8Rng::seed_from_u64(seed),
            events: Vec::new(),
        }
    }

    /// Set initial flow budget for an authority
    pub fn set_budget(&mut self, authority: AuthorityId, budget: u32) {
        self.flow_budgets.insert(authority, budget);
    }

    /// Get current flow budget for an authority
    pub fn get_budget(&self, authority: &AuthorityId) -> u32 {
        self.flow_budgets.get(authority).copied().unwrap_or(0)
    }

    /// Record an event
    pub fn record_event(&mut self, event: SimulationEvent) {
        self.events.push(event);
    }

    /// Advance simulation time
    pub fn advance_time(&mut self, new_time: TimeStamp) {
        self.current_time = new_time;
    }

    /// Generate deterministic nonce
    pub fn generate_nonce(&mut self, bytes: usize) -> Vec<u8> {
        let mut nonce = vec![0u8; bytes];
        self.rng.fill_bytes(&mut nonce);
        nonce
    }
}

/// Simulation effect interpreter for deterministic execution
///
/// This interpreter maintains internal state and records all effects as events,
/// enabling full replay and inspection of execution traces.
pub struct SimulationEffectInterpreter {
    /// Shared simulation state
    state: Arc<Mutex<SimulationState>>,
    /// Current authority ID for context
    authority_id: AuthorityId,
    /// Source network address for this interpreter
    source_address: NetworkAddress,
}

impl SimulationEffectInterpreter {
    /// Create a new simulation interpreter with initial state
    pub fn new(
        seed: u64,
        initial_time: TimeStamp,
        authority_id: AuthorityId,
        source_address: NetworkAddress,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(SimulationState::new(seed, initial_time))),
            authority_id,
            source_address,
        }
    }

    /// Create from existing state (for sharing state between interpreters)
    pub fn from_state(
        state: Arc<Mutex<SimulationState>>,
        authority_id: AuthorityId,
        source_address: NetworkAddress,
    ) -> Self {
        Self {
            state,
            authority_id,
            source_address,
        }
    }

    /// Get a read lock on the state for inspection
    pub fn state(&self) -> MutexGuard<'_, SimulationState> {
        self.state.lock().expect("Simulator state lock poisoned")
    }

    /// Get a clone of the current state
    pub fn snapshot_state(&self) -> SimulationState {
        self.state.lock().expect("Simulator state lock poisoned").clone()
    }

    /// Get all recorded events
    pub fn events(&self) -> Vec<SimulationEvent> {
        self.state.lock().unwrap().events.clone()
    }

    /// Get events of a specific type
    pub fn events_of_type(
        &self,
        filter: impl Fn(&SimulationEvent) -> bool,
    ) -> Vec<SimulationEvent> {
        self.state
            .lock()
            .unwrap()
            .events
            .iter()
            .filter(|e| filter(e))
            .cloned()
            .collect()
    }

    /// Replay events from a previous execution
    pub async fn replay(&self, events: Vec<SimulationEvent>) -> Result<()> {
        let mut state = self.state.lock().unwrap();

        // Clear current state
        state.events.clear();
        state.flow_budgets.clear();
        state.journal.clear();
        state.metadata.clear();
        state.total_leakage_bits = 0;
        state.message_queue.clear();

        // Replay each event
        for event in events {
            match &event {
                SimulationEvent::BudgetCharged {
                    authority,
                    remaining,
                    ..
                } => {
                    state.flow_budgets.insert(*authority, *remaining);
                }
                SimulationEvent::JournalAppended { entry, .. } => {
                    state.journal.push(entry.clone());
                }
                SimulationEvent::LeakageRecorded { bits, .. } => {
                    state.total_leakage_bits += bits;
                }
                SimulationEvent::MetadataStored { key, value, .. } => {
                    state.metadata.insert(key.clone(), value.clone());
                }
                SimulationEvent::EnvelopeQueued {
                    from,
                    to,
                    envelope,
                    time,
                } => {
                    state.message_queue.push(QueuedMessage {
                        from: from.clone(),
                        to: to.clone(),
                        envelope: envelope.clone(),
                        timestamp: time.clone(),
                    });
                }
                SimulationEvent::NonceGenerated { .. } => {
                    // Nonces are generated deterministically from RNG
                }
            }
            state.record_event(event);
        }

        Ok(())
    }

    /// Set initial flow budget for testing
    pub fn set_initial_budget(&self, authority: AuthorityId, budget: u32) {
        let mut state = self.state.lock().unwrap();
        state.set_budget(authority, budget);
    }

    /// Advance simulation time
    pub fn advance_time(&self, new_time: TimeStamp) {
        let mut state = self.state.lock().unwrap();
        state.advance_time(new_time);
    }
}

#[async_trait]
impl EffectInterpreter for SimulationEffectInterpreter {
    async fn execute(&self, cmd: EffectCommand) -> Result<EffectResult> {
        let mut state = self.state.lock().unwrap();

        match cmd {
            EffectCommand::ChargeBudget { authority, amount, .. } => {
                debug!(?authority, amount, "Simulation: Charging flow budget");

                let current_budget = state.get_budget(&authority);
                if current_budget < amount {
                    return Err(AuraError::invalid(format!(
                        "Insufficient budget: has {}, needs {}",
                        current_budget, amount
                    )));
                }

                let remaining = current_budget - amount;
                state.set_budget(authority, remaining);

                let current_time = state.current_time.clone();
                let event = SimulationEvent::BudgetCharged {
                    time: current_time,
                    authority,
                    amount,
                    remaining,
                };
                state.record_event(event);

                info!(
                    ?authority,
                    amount, remaining, "Simulation: Successfully charged flow budget"
                );

                Ok(EffectResult::RemainingBudget(remaining))
            }

            EffectCommand::AppendJournal { entry } => {
                debug!(
                    authority = ?entry.authority,
                    fact = ?entry.fact,
                    "Simulation: Appending journal entry"
                );

                state.journal.push(entry.clone());

                let event = SimulationEvent::JournalAppended {
                    time: state.current_time.clone(),
                    entry,
                };
                state.record_event(event);

                info!("Simulation: Successfully appended journal entry");

                Ok(EffectResult::Success)
            }

            EffectCommand::RecordLeakage { bits } => {
                debug!(
                    bits,
                    authority = ?self.authority_id,
                    "Simulation: Recording metadata leakage"
                );

                state.total_leakage_bits += bits;

                let event = SimulationEvent::LeakageRecorded {
                    time: state.current_time.clone(),
                    bits,
                };
                state.record_event(event);

                info!(bits, "Simulation: Successfully recorded leakage");

                Ok(EffectResult::Success)
            }

            EffectCommand::StoreMetadata { key, value } => {
                debug!(key, value_len = value.len(), "Simulation: Storing metadata");

                state.metadata.insert(key.clone(), value.clone());

                let event = SimulationEvent::MetadataStored {
                    time: state.current_time.clone(),
                    key,
                    value,
                };
                state.record_event(event);

                info!("Simulation: Successfully stored metadata");

                Ok(EffectResult::Success)
            }

            EffectCommand::SendEnvelope { to, envelope } => {
                debug!(
                    ?to,
                    envelope_len = envelope.len(),
                    "Simulation: Queuing network envelope"
                );

                let current_time = state.current_time.clone();
                state.message_queue.push(QueuedMessage {
                    from: self.source_address.clone(),
                    to: to.clone(),
                    envelope: envelope.clone(),
                    timestamp: current_time.clone(),
                });

                let event = SimulationEvent::EnvelopeQueued {
                    time: current_time,
                    from: self.source_address.clone(),
                    to,
                    envelope,
                };
                state.record_event(event);

                info!("Simulation: Successfully queued envelope");

                Ok(EffectResult::Success)
            }

            EffectCommand::GenerateNonce { bytes } => {
                debug!(bytes, "Simulation: Generating nonce");

                let nonce = state.generate_nonce(bytes);

                let event = SimulationEvent::NonceGenerated {
                    time: state.current_time.clone(),
                    nonce: nonce.clone(),
                };
                state.record_event(event);

                info!(bytes, "Simulation: Successfully generated nonce");

                Ok(EffectResult::Nonce(nonce))
            }
        }
    }

    fn interpreter_type(&self) -> &'static str {
        "simulation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        identifiers::{AuthorityId, ContextId},
        journal::Fact,
        time::{PhysicalTime, TimeStamp},
    };

    #[tokio::test]
    async fn test_deterministic_nonce_generation() {
        let authority = AuthorityId::new();
        let addr = NetworkAddress::new("test://addr1".to_string());
        let time = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });

        // Create two interpreters with same seed
        let interp1 = SimulationEffectInterpreter::new(42, time.clone(), authority, addr.clone());
        let interp2 = SimulationEffectInterpreter::new(42, time, authority, addr);

        // Generate nonces
        let cmd = EffectCommand::GenerateNonce { bytes: 32 };
        let result1 = interp1.execute(cmd.clone()).await.unwrap();
        let result2 = interp2.execute(cmd).await.unwrap();

        // Should be identical due to same seed
        match (result1, result2) {
            (EffectResult::Nonce(n1), EffectResult::Nonce(n2)) => {
                assert_eq!(n1, n2, "Nonces should be deterministic");
            }
            _ => panic!("Expected nonce results"),
        }
    }

    #[tokio::test]
    async fn test_flow_budget_tracking() {
        let authority = AuthorityId::new();
        let addr = NetworkAddress::new("test://addr1".to_string());
        let time = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 2000,
            uncertainty: None,
        });

        let interp = SimulationEffectInterpreter::new(42, time, authority, addr);

        // Set initial budget
        interp.set_initial_budget(authority, 1000);

        // Charge budget
        let cmd = EffectCommand::ChargeBudget {
            context: ContextId::new(),
            authority,
            peer: authority,
            amount: 250,
        };
        let result = interp.execute(cmd).await.unwrap();

        match result {
            EffectResult::RemainingBudget(remaining) => {
                assert_eq!(remaining, 750);
            }
            _ => panic!("Expected remaining budget"),
        }

        // Check state
        let state = interp.snapshot_state();
        assert_eq!(state.get_budget(&authority), 750);
        assert_eq!(state.events.len(), 1);
    }

    #[tokio::test]
    async fn test_event_recording() {
        let authority = AuthorityId::new();
        let addr = NetworkAddress::new("test://addr1".to_string());
        let time = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 3000,
            uncertainty: None,
        });

        let interp = SimulationEffectInterpreter::new(42, time, authority, addr);

        // Execute various commands
        let cmds = vec![
            EffectCommand::StoreMetadata {
                key: "test_key".to_string(),
                value: "test_value".to_string(),
            },
            EffectCommand::RecordLeakage { bits: 128 },
            EffectCommand::SendEnvelope {
                to: NetworkAddress::new("test://addr2".to_string()),
                envelope: vec![1, 2, 3, 4],
            },
        ];

        for cmd in cmds {
            interp.execute(cmd).await.unwrap();
        }

        // Check events
        let events = interp.events();
        assert_eq!(events.len(), 3);

        // Check event types
        let metadata_events =
            interp.events_of_type(|e| matches!(e, SimulationEvent::MetadataStored { .. }));
        assert_eq!(metadata_events.len(), 1);

        let leakage_events =
            interp.events_of_type(|e| matches!(e, SimulationEvent::LeakageRecorded { .. }));
        assert_eq!(leakage_events.len(), 1);

        let envelope_events =
            interp.events_of_type(|e| matches!(e, SimulationEvent::EnvelopeQueued { .. }));
        assert_eq!(envelope_events.len(), 1);
    }

    #[tokio::test]
    async fn test_replay_capability() {
        let authority = AuthorityId::new();
        let addr = NetworkAddress::new("test://addr1".to_string());
        let time = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 4000,
            uncertainty: None,
        });

        // First execution
        let interp1 = SimulationEffectInterpreter::new(42, time.clone(), authority, addr.clone());
        interp1.set_initial_budget(authority, 1000);

        let cmds = vec![
            EffectCommand::ChargeBudget {
                context: ContextId::new(),
                authority,
                peer: authority,
                amount: 100,
            },
            EffectCommand::StoreMetadata {
                key: "k1".to_string(),
                value: "v1".to_string(),
            },
            EffectCommand::RecordLeakage { bits: 64 },
        ];

        for cmd in cmds {
            interp1.execute(cmd).await.unwrap();
        }

        let events = interp1.events();
        let final_state = interp1.snapshot_state();

        // Replay in new interpreter
        let interp2 = SimulationEffectInterpreter::new(99, time, authority, addr);
        interp2.replay(events).await.unwrap();

        let replay_state = interp2.snapshot_state();

        // States should match (except RNG and timestamps)
        assert_eq!(replay_state.flow_budgets, final_state.flow_budgets);
        assert_eq!(replay_state.metadata, final_state.metadata);
        assert_eq!(
            replay_state.total_leakage_bits,
            final_state.total_leakage_bits
        );
        assert_eq!(replay_state.journal.len(), final_state.journal.len());
    }

    #[tokio::test]
    async fn test_shared_state() {
        let authority1 = AuthorityId::new();
        let authority2 = AuthorityId::new();
        let time = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 5000,
            uncertainty: None,
        });

        // Create shared state
        let state = Arc::new(Mutex::new(SimulationState::new(42, time)));

        // Create two interpreters sharing state
        let interp1 = SimulationEffectInterpreter::from_state(
            state.clone(),
            authority1,
            NetworkAddress::new("test://addr1".to_string()),
        );
        let interp2 = SimulationEffectInterpreter::from_state(
            state,
            authority2,
            NetworkAddress::new("test://addr2".to_string()),
        );

        // Store metadata from both
        interp1
            .execute(EffectCommand::StoreMetadata {
                key: "key1".to_string(),
                value: "from_interp1".to_string(),
            })
            .await
            .unwrap();

        interp2
            .execute(EffectCommand::StoreMetadata {
                key: "key2".to_string(),
                value: "from_interp2".to_string(),
            })
            .await
            .unwrap();

        // Both should see both keys
        let state1 = interp1.snapshot_state();
        let state2 = interp2.snapshot_state();

        assert_eq!(state1.metadata.len(), 2);
        assert_eq!(state2.metadata.len(), 2);
        assert_eq!(
            state1.metadata.get("key1"),
            Some(&"from_interp1".to_string())
        );
        assert_eq!(
            state2.metadata.get("key2"),
            Some(&"from_interp2".to_string())
        );
    }
}
