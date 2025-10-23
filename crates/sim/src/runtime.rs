//! Side effect runtime
//!
//! The runtime receives effects from all participants and routes them appropriately:
//! - Send effects go to the simulated network
//! - Ledger writes are handed back to the originating participant
//! - Storage operations are handled by in-memory stores
//!
//! The runtime never fabricates effects - it only processes what participants produce.

use crate::{Effect, Envelope, ParticipantId, Result, SimError, SimulatedNetwork, Tick};
use indexmap::IndexMap;
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Side effect runtime
///
/// Central hub that receives and processes effects from all participants.
pub struct SideEffectRuntime {
    /// The simulated network
    network: SimulatedNetwork,
    
    /// Effect channels for each participant
    effect_sinks: HashMap<ParticipantId, mpsc::UnboundedSender<Effect>>,
    
    /// Effect sources to receive from participants
    effect_sources: HashMap<ParticipantId, mpsc::UnboundedReceiver<Effect>>,
    
    /// In-memory storage for each participant
    participant_storage: IndexMap<ParticipantId, HashMap<Vec<u8>, Vec<u8>>>,
    
    /// Current tick
    current_tick: Tick,
}

impl SideEffectRuntime {
    /// Create a new runtime with the given network
    pub fn new(network: SimulatedNetwork) -> Self {
        SideEffectRuntime {
            network,
            effect_sinks: HashMap::new(),
            effect_sources: HashMap::new(),
            participant_storage: IndexMap::new(),
            current_tick: 0,
        }
    }
    
    /// Register a new participant and return their effect sink
    pub fn register_participant(&mut self, participant: ParticipantId) -> mpsc::UnboundedSender<Effect> {
        let (tx, rx) = mpsc::unbounded_channel();
        
        self.effect_sinks.insert(participant, tx.clone());
        self.effect_sources.insert(participant, rx);
        self.participant_storage.insert(participant, HashMap::new());
        self.network.add_participant(participant);
        
        tx
    }
    
    /// Remove a participant
    pub fn unregister_participant(&mut self, participant: ParticipantId) {
        self.effect_sinks.remove(&participant);
        self.effect_sources.remove(&participant);
        self.participant_storage.shift_remove(&participant);
        self.network.remove_participant(participant);
    }
    
    /// Process all pending effects from all participants
    ///
    /// Returns the number of effects processed.
    pub async fn process_effects(&mut self) -> Result<usize> {
        let mut processed_count = 0;
        
        // Collect all participant IDs to avoid borrow issues
        let participants: Vec<ParticipantId> = self.effect_sources.keys().copied().collect();
        
        for participant in participants {
            // Process all available effects for this participant
            while let Ok(effect) = self.effect_sources
                .get_mut(&participant)
                .ok_or(SimError::ParticipantNotFound(participant))?
                .try_recv()
            {
                self.process_effect(participant, effect)?;
                processed_count += 1;
            }
        }
        
        Ok(processed_count)
    }
    
    /// Process a single effect
    fn process_effect(&mut self, _source: ParticipantId, effect: Effect) -> Result<()> {
        match effect {
            Effect::Send(envelope) => {
                // Forward to network
                self.network.enqueue_message(envelope)
                    .map_err(|e| SimError::NetworkError(e.to_string()))?;
            }
            
            Effect::WriteToLocalLedger { participant: _, event_data: _ } => {
                // In a real implementation, this would be handled by the participant's
                // AccountLedger. For now, we just acknowledge it.
                // The participant's ledger is managed internally by the agent.
                // This effect is mainly for coordination/observation.
            }
            
            Effect::ReadFromStorage { participant, key: _ } => {
                // Look up value in participant's storage
                let _storage = self.participant_storage
                    .get(&participant)
                    .ok_or(SimError::ParticipantNotFound(participant))?;
                
                // In a full implementation, we'd send the value back to the participant
                // For now, storage reads are synchronous within the participant
            }
            
            Effect::WriteToStorage { participant, key, value } => {
                // Write to participant's storage
                let storage = self.participant_storage
                    .get_mut(&participant)
                    .ok_or(SimError::ParticipantNotFound(participant))?;
                
                storage.insert(key, value);
            }
            
            Effect::Log { participant: _, level: _, message: _ } => {
                // In test mode, we might capture logs
                // For now, just ignore (or could print for debugging)
                // NOTE: variables are intentionally ignored as indicated by underscores
            }
        }
        
        Ok(())
    }
    
    /// Advance the runtime by one tick
    ///
    /// This advances the network and delivers due messages.
    /// Returns the number of messages delivered.
    pub fn advance_tick(&mut self) -> Result<usize> {
        self.current_tick += 1;
        self.network.advance_tick()
    }
    
    /// Get current tick
    pub fn current_tick(&self) -> Tick {
        self.current_tick
    }
    
    /// Check if there are any inflight messages or pending effects
    pub fn is_idle(&self) -> bool {
        !self.network.has_inflight_messages()
            && self.effect_sources.values().all(|rx| rx.is_empty())
    }
    
    /// Get mutable reference to the network (for configuration)
    pub fn network_mut(&mut self) -> &mut SimulatedNetwork {
        &mut self.network
    }
    
    /// Get reference to the network
    pub fn network(&self) -> &SimulatedNetwork {
        &self.network
    }
    
    /// Deliver messages to a specific participant
    ///
    /// Returns the list of envelopes delivered.
    pub fn deliver_messages_to(&mut self, participant: ParticipantId) -> Vec<Envelope> {
        self.network.drain_messages(participant)
    }
    
    /// Get storage value for a participant
    pub fn get_storage(&self, participant: ParticipantId, key: &[u8]) -> Option<Vec<u8>> {
        self.participant_storage
            .get(&participant)
            .and_then(|storage| storage.get(key).cloned())
    }
}

/// Effect sink handle for a participant
///
/// This allows participants to emit effects into the runtime.
#[derive(Clone)]
pub struct EffectSink {
    sender: mpsc::UnboundedSender<Effect>,
    participant: ParticipantId,
}

impl EffectSink {
    /// Create a new effect sink
    pub fn new(sender: mpsc::UnboundedSender<Effect>, participant: ParticipantId) -> Self {
        EffectSink { sender, participant }
    }
    
    /// Emit an effect
    pub fn emit(&self, effect: Effect) -> Result<()> {
        self.sender.send(effect)
            .map_err(|e| SimError::EffectError(format!("Failed to send effect: {}", e)))
    }
    
    /// Get participant ID
    pub fn participant(&self) -> ParticipantId {
        self.participant
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeliverySemantics, SimulatedNetwork};
    use uuid::Uuid;
    
    #[tokio::test]
    async fn test_runtime_basic() {
        let network = SimulatedNetwork::new(42);
        let mut runtime = SideEffectRuntime::new(network);
        
        let alice = ParticipantId::from_name("alice");
        let bob = ParticipantId::from_name("bob");
        
        let alice_sink = runtime.register_participant(alice);
        runtime.register_participant(bob);
        
        // Alice sends a message to Bob
        let envelope = Envelope {
            message_id: Uuid::new_v4(),
            sender: alice,
            recipients: vec![bob],
            payload: vec![1, 2, 3],
            delivery: DeliverySemantics::Unicast,
        };
        
        alice_sink.send(Effect::Send(envelope)).unwrap();
        
        // Process effects
        let count = runtime.process_effects().await.unwrap();
        assert_eq!(count, 1);
        
        // Advance tick to deliver message
        runtime.advance_tick().unwrap();
        
        // Check that Bob received the message
        let messages = runtime.deliver_messages_to(bob);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].sender, alice);
    }
    
    #[tokio::test]
    async fn test_runtime_storage() {
        let network = SimulatedNetwork::new(42);
        let mut runtime = SideEffectRuntime::new(network);
        
        let alice = ParticipantId::from_name("alice");
        let alice_sink = runtime.register_participant(alice);
        
        // Write to storage
        alice_sink.send(Effect::WriteToStorage {
            participant: alice,
            key: b"test_key".to_vec(),
            value: b"test_value".to_vec(),
        }).unwrap();
        
        runtime.process_effects().await.unwrap();
        
        // Read from storage
        let value = runtime.get_storage(alice, b"test_key");
        assert_eq!(value, Some(b"test_value".to_vec()));
    }
}

