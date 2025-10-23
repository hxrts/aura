//! Simulated network fabric
//!
//! This module implements a logical network that simulates:
//! - Message delivery with configurable latency
//! - Network partitions
//! - Message drops and reordering
//!
//! The network does NOT maintain canonical state - each participant has their own
//! independent view that converges through CRDT merges.

use crate::{Envelope, ParticipantId, Result, Tick};
use indexmap::IndexMap;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ops::Range;

/// Simulated network fabric
///
/// The network queues messages with simulated latency and handles partitions.
/// Messages are stored by their scheduled delivery tick.
pub struct SimulatedNetwork {
    /// Messages in flight, keyed by delivery tick
    inflight_messages: BTreeMap<Tick, Vec<Envelope>>,
    
    /// Per-participant mailboxes for delivered messages
    peer_mailboxes: IndexMap<ParticipantId, VecDeque<Envelope>>,
    
    /// Current logical tick
    current_tick: Tick,
    
    /// Latency configuration (min, max) in ticks
    latency_range: Range<Tick>,
    
    /// Active network partitions
    /// Each set represents a partition island - participants in the same set can communicate
    partitions: Vec<HashSet<ParticipantId>>,
    
    /// Message drop rate (0.0 = no drops, 1.0 = drop all)
    drop_rate: f64,
    
    /// Deterministic RNG for latency and drops
    rng: rand::rngs::StdRng,
}

impl SimulatedNetwork {
    /// Create a new simulated network
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        
        SimulatedNetwork {
            inflight_messages: BTreeMap::new(),
            peer_mailboxes: IndexMap::new(),
            current_tick: 0,
            latency_range: 1..10, // Default: 1-10 ticks latency
            partitions: Vec::new(),
            drop_rate: 0.0,
            rng: rand::rngs::StdRng::seed_from_u64(seed),
        }
    }
    
    /// Register a new participant
    pub fn add_participant(&mut self, participant: ParticipantId) {
        self.peer_mailboxes.entry(participant)
            .or_default();
    }
    
    /// Remove a participant
    pub fn remove_participant(&mut self, participant: ParticipantId) {
        self.peer_mailboxes.shift_remove(&participant);
    }
    
    /// Configure network latency range
    pub fn set_latency_range(&mut self, min: Tick, max: Tick) {
        self.latency_range = min..max;
    }
    
    /// Set message drop rate (0.0 to 1.0)
    pub fn set_drop_rate(&mut self, rate: f64) {
        self.drop_rate = rate.clamp(0.0, 1.0);
    }
    
    /// Create a network partition
    ///
    /// Messages between different partition islands will be dropped.
    /// Pass a vector of participant sets, where each set is an island.
    pub fn partition(&mut self, islands: Vec<HashSet<ParticipantId>>) {
        self.partitions = islands;
    }
    
    /// Clear all partitions (restore full connectivity)
    pub fn heal_partitions(&mut self) {
        self.partitions.clear();
    }
    
    /// Enqueue a message for delivery
    ///
    /// The message will be delivered after a random latency delay,
    /// unless it's dropped or blocked by a partition.
    pub fn enqueue_message(&mut self, envelope: Envelope) -> Result<()> {
        use rand::Rng;
        
        // Check if message should be dropped
        if self.rng.gen::<f64>() < self.drop_rate {
            return Ok(()); // Silently drop
        }
        
        // Expand recipients for broadcast
        let recipients = if envelope.recipients.is_empty() {
            // Broadcast to all participants
            self.peer_mailboxes.keys().copied().collect()
        } else {
            envelope.recipients.clone()
        };
        
        // Check partitions and filter recipients
        let deliverable_recipients: Vec<ParticipantId> = recipients
            .into_iter()
            .filter(|recipient| self.can_communicate(&envelope.sender, recipient))
            .collect();
        
        if deliverable_recipients.is_empty() {
            return Ok(()); // All recipients unreachable
        }
        
        // Calculate delivery tick
        let latency = if self.latency_range.start >= self.latency_range.end {
            self.latency_range.start
        } else {
            self.rng.gen_range(self.latency_range.clone())
        };
        let delivery_tick = self.current_tick + latency;
        
        // Create individual envelope for each recipient
        for recipient in deliverable_recipients {
            let mut recipient_envelope = envelope.clone();
            recipient_envelope.recipients = vec![recipient];
            
            self.inflight_messages
                .entry(delivery_tick)
                .or_default()
                .push(recipient_envelope);
        }
        
        Ok(())
    }
    
    /// Check if two participants can communicate given current partitions
    fn can_communicate(&self, from: &ParticipantId, to: &ParticipantId) -> bool {
        if self.partitions.is_empty() {
            return true; // No partitions, full connectivity
        }
        
        // Find which island each participant belongs to
        let from_island = self.partitions.iter().find(|island| island.contains(from));
        let to_island = self.partitions.iter().find(|island| island.contains(to));
        
        // Can communicate if in same island (or if either is not in any island)
        match (from_island, to_island) {
            (Some(from_i), Some(to_i)) => std::ptr::eq(from_i, to_i),
            _ => true, // If not explicitly partitioned, allow communication
        }
    }
    
    /// Advance network by one tick and deliver due messages
    ///
    /// Returns the number of messages delivered.
    pub fn advance_tick(&mut self) -> Result<usize> {
        self.current_tick += 1;
        
        // Deliver all messages due at or before current tick
        let mut delivered_count = 0;
        
        // Collect ticks to process
        let ticks_to_process: Vec<Tick> = self.inflight_messages
            .range(..=self.current_tick)
            .map(|(&tick, _)| tick)
            .collect();
        
        for tick in ticks_to_process {
            if let Some(messages) = self.inflight_messages.remove(&tick) {
                for envelope in messages {
                    if let Some(recipient) = envelope.recipients.first() {
                        if let Some(mailbox) = self.peer_mailboxes.get_mut(recipient) {
                            mailbox.push_back(envelope);
                            delivered_count += 1;
                        }
                    }
                }
            }
        }
        
        Ok(delivered_count)
    }
    
    /// Get current tick
    pub fn current_tick(&self) -> Tick {
        self.current_tick
    }
    
    /// Check if there are any messages in flight
    pub fn has_inflight_messages(&self) -> bool {
        !self.inflight_messages.is_empty()
    }
    
    /// Check if a participant has pending messages
    pub fn has_pending_messages(&self, participant: ParticipantId) -> bool {
        self.peer_mailboxes
            .get(&participant)
            .map(|mailbox| !mailbox.is_empty())
            .unwrap_or(false)
    }
    
    /// Get the next message for a participant
    pub fn receive_message(&mut self, participant: ParticipantId) -> Option<Envelope> {
        self.peer_mailboxes
            .get_mut(&participant)
            .and_then(|mailbox| mailbox.pop_front())
    }
    
    /// Drain all messages for a participant
    pub fn drain_messages(&mut self, participant: ParticipantId) -> Vec<Envelope> {
        self.peer_mailboxes
            .get_mut(&participant)
            .map(|mailbox| mailbox.drain(..).collect())
            .unwrap_or_default()
    }
    
    /// Get statistics about the network state
    pub fn stats(&self) -> NetworkStats {
        NetworkStats {
            current_tick: self.current_tick,
            inflight_message_count: self.inflight_messages.values().map(|v| v.len()).sum(),
            total_mailbox_count: self.peer_mailboxes.values().map(|v| v.len()).sum(),
            partition_count: self.partitions.len(),
            participant_count: self.peer_mailboxes.len(),
        }
    }
}

/// Network statistics
#[derive(Debug, Clone)]
pub struct NetworkStats {
    pub current_tick: Tick,
    pub inflight_message_count: usize,
    pub total_mailbox_count: usize,
    pub partition_count: usize,
    pub participant_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeliverySemantics};
    use uuid::Uuid;
    
    fn create_test_envelope(sender: ParticipantId, recipients: Vec<ParticipantId>) -> Envelope {
        Envelope {
            message_id: Uuid::new_v4(),
            sender,
            recipients,
            payload: vec![1, 2, 3],
            delivery: DeliverySemantics::Unicast,
        }
    }
    
    #[test]
    fn test_network_basic_delivery() {
        let mut network = SimulatedNetwork::new(42);
        let alice = ParticipantId::from_name("alice");
        let bob = ParticipantId::from_name("bob");
        
        network.add_participant(alice);
        network.add_participant(bob);
        network.set_latency_range(1, 1); // Fixed 1-tick latency
        
        let envelope = create_test_envelope(alice, vec![bob]);
        network.enqueue_message(envelope).unwrap();
        
        assert!(network.has_inflight_messages());
        assert!(!network.has_pending_messages(bob));
        
        // Advance tick - message should be delivered
        network.advance_tick().unwrap();
        
        assert!(!network.has_inflight_messages());
        assert!(network.has_pending_messages(bob));
        
        let received = network.receive_message(bob).unwrap();
        assert_eq!(received.sender, alice);
    }
    
    #[test]
    fn test_network_partition() {
        let mut network = SimulatedNetwork::new(42);
        let alice = ParticipantId::from_name("alice");
        let bob = ParticipantId::from_name("bob");
        let carol = ParticipantId::from_name("carol");
        
        network.add_participant(alice);
        network.add_participant(bob);
        network.add_participant(carol);
        network.set_latency_range(1, 1);
        
        // Create partition: {alice, bob} | {carol}
        network.partition(vec![
            HashSet::from([alice, bob]),
            HashSet::from([carol]),
        ]);
        
        // Alice -> Bob should work (same island)
        let envelope1 = create_test_envelope(alice, vec![bob]);
        network.enqueue_message(envelope1).unwrap();
        
        // Alice -> Carol should be dropped (different island)
        let envelope2 = create_test_envelope(alice, vec![carol]);
        network.enqueue_message(envelope2).unwrap();
        
        network.advance_tick().unwrap();
        
        assert!(network.has_pending_messages(bob));
        assert!(!network.has_pending_messages(carol));
    }
}

