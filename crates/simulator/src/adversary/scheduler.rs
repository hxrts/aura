//! Adversarial Message Scheduling
//!
//! Controls message delivery order and timing to test worst-case scenarios.
//! All scheduling decisions are deterministic based on simulation Effects.

use std::collections::{BTreeMap, VecDeque};
use uuid::Uuid;

/// Message scheduling strategies for adversarial testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStrategy {
    /// Deliver messages in FIFO order (baseline)
    Fifo,
    /// Deliver messages in LIFO order (worst for causal ordering)
    Lifo,
    /// Deliver messages in random order (using deterministic RNG)
    Random,
    /// Maximize reordering to stress CRDT convergence
    MaximalReordering,
    /// Delay specific messages to create race conditions
    SelectiveDelay,
}

/// Controls how messages are reordered
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageReordering {
    /// No reordering
    None,
    /// Reverse order within each batch
    Reverse,
    /// Interleave messages from different sources
    Interleave,
    /// Maximum causal disorder
    MaximalDisorder,
}

/// Adversarial scheduler that controls message delivery
pub struct AdversarialScheduler {
    strategy: DeliveryStrategy,
    reordering: MessageReordering,
    pending_messages: BTreeMap<Uuid, VecDeque<PendingMessage>>,
    delayed_until: BTreeMap<MessageId, u64>,
}

#[derive(Debug, Clone)]
struct PendingMessage {
    id: MessageId,
    from: Uuid,
    to: Uuid,
    payload: Vec<u8>,
    enqueued_at: u64,
}

type MessageId = u64;

impl AdversarialScheduler {
    /// Create new scheduler with specified strategy
    pub fn new(strategy: DeliveryStrategy, reordering: MessageReordering) -> Self {
        Self {
            strategy,
            reordering,
            pending_messages: BTreeMap::new(),
            delayed_until: BTreeMap::new(),
        }
    }

    /// Create scheduler that maximizes CRDT stress
    pub fn maximal_reordering() -> Self {
        Self::new(
            DeliveryStrategy::MaximalReordering,
            MessageReordering::MaximalDisorder,
        )
    }

    /// Enqueue a message for delivery
    pub fn enqueue_message(
        &mut self,
        msg_id: MessageId,
        from: Uuid,
        to: Uuid,
        payload: Vec<u8>,
        current_time: u64,
    ) {
        let msg = PendingMessage {
            id: msg_id,
            from,
            to,
            payload,
            enqueued_at: current_time,
        };

        self.pending_messages
            .entry(to)
            .or_insert_with(VecDeque::new)
            .push_back(msg);
    }

    /// Delay specific message until specified time
    pub fn delay_message_until(&mut self, msg_id: MessageId, until: u64) {
        self.delayed_until.insert(msg_id, until);
    }

    /// Get next message to deliver based on strategy
    pub fn next_message(&mut self, current_time: u64) -> Option<(Uuid, Vec<u8>)> {
        match self.strategy {
            DeliveryStrategy::Fifo => self.next_fifo(current_time),
            DeliveryStrategy::Lifo => self.next_lifo(current_time),
            DeliveryStrategy::Random => self.next_random(current_time),
            DeliveryStrategy::MaximalReordering => self.next_maximal_reordering(current_time),
            DeliveryStrategy::SelectiveDelay => self.next_with_selective_delay(current_time),
        }
    }

    fn next_fifo(&mut self, current_time: u64) -> Option<(Uuid, Vec<u8>)> {
        // Find first available message
        let recipient_to_remove = {
            let mut result = None;
            for (recipient, queue) in &self.pending_messages {
                if let Some(msg) = queue.front() {
                    if self.is_ready_for_delivery(msg.id, current_time) {
                        result = Some(*recipient);
                        break;
                    }
                }
            }
            result
        };
        
        if let Some(recipient) = recipient_to_remove {
            let queue = self.pending_messages.get_mut(&recipient).unwrap();
            let msg = queue.pop_front().unwrap();
            return Some((msg.to, msg.payload));
        }
        
        None
    }

    fn next_lifo(&mut self, current_time: u64) -> Option<(Uuid, Vec<u8>)> {
        // Find last available message
        let recipient_to_remove = {
            let mut result = None;
            for (recipient, queue) in &self.pending_messages {
                if let Some(msg) = queue.back() {
                    if self.is_ready_for_delivery(msg.id, current_time) {
                        result = Some(*recipient);
                        break;
                    }
                }
            }
            result
        };
        
        if let Some(recipient) = recipient_to_remove {
            let queue = self.pending_messages.get_mut(&recipient).unwrap();
            let msg = queue.pop_back().unwrap();
            return Some((msg.to, msg.payload));
        }
        
        None
    }

    fn next_random(&mut self, current_time: u64) -> Option<(Uuid, Vec<u8>)> {
        // Use deterministic "random" selection based on current time
        // First collect all available messages without borrowing conflicts
        let mut available = Vec::new();
        for (recipient, queue) in &self.pending_messages {
            for (idx, msg) in queue.iter().enumerate() {
                if self.is_ready_for_delivery(msg.id, current_time) {
                    available.push((*recipient, idx));
                }
            }
        }

        if available.is_empty() {
            return None;
        }

        // Deterministic "random" selection using time as seed
        let selection = (current_time as usize) % available.len();
        let (recipient, idx) = available[selection];

        let queue = self.pending_messages.get_mut(&recipient).unwrap();
        let msg = queue.remove(idx).unwrap();
        Some((msg.to, msg.payload))
    }

    fn next_maximal_reordering(&mut self, current_time: u64) -> Option<(Uuid, Vec<u8>)> {
        // Deliver message that maximizes causal disorder
        // Strategy: prefer older messages from different sources

        let mut best: Option<(Uuid, usize, u64)> = None;

        for (recipient, queue) in self.pending_messages.iter() {
            for (idx, msg) in queue.iter().enumerate() {
                if !self.is_ready_for_delivery(msg.id, current_time) {
                    continue;
                }

                // Prefer older messages (maximize delay)
                let age = current_time.saturating_sub(msg.enqueued_at);

                match best {
                    None => best = Some((*recipient, idx, age)),
                    Some((_, _, best_age)) if age > best_age => {
                        best = Some((*recipient, idx, age));
                    }
                    _ => {}
                }
            }
        }

        if let Some((recipient, idx, _)) = best {
            let queue = self.pending_messages.get_mut(&recipient).unwrap();
            let msg = queue.remove(idx).unwrap();
            return Some((msg.to, msg.payload));
        }

        None
    }

    fn next_with_selective_delay(&mut self, current_time: u64) -> Option<(Uuid, Vec<u8>)> {
        // Respect selective delays, otherwise FIFO
        self.next_fifo(current_time)
    }

    fn is_ready_for_delivery(&self, msg_id: MessageId, current_time: u64) -> bool {
        match self.delayed_until.get(&msg_id) {
            Some(&delay_until) => current_time >= delay_until,
            None => true,
        }
    }

    /// Check if any messages are pending
    pub fn has_pending_messages(&self) -> bool {
        self.pending_messages.values().any(|q| !q.is_empty())
    }

    /// Count pending messages
    pub fn pending_count(&self) -> usize {
        self.pending_messages.values().map(|q| q.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_delivery() {
        let mut scheduler =
            AdversarialScheduler::new(DeliveryStrategy::Fifo, MessageReordering::None);

        let alice = Uuid::from_bytes([1; 16]);
        let bob = Uuid::from_bytes([2; 16]);

        scheduler.enqueue_message(1, alice, bob, vec![1], 100);
        scheduler.enqueue_message(2, alice, bob, vec![2], 200);
        scheduler.enqueue_message(3, alice, bob, vec![3], 300);

        // FIFO order
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![1])));
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![2])));
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![3])));
        assert_eq!(scheduler.next_message(1000), None);
    }

    #[test]
    fn test_lifo_delivery() {
        let mut scheduler =
            AdversarialScheduler::new(DeliveryStrategy::Lifo, MessageReordering::None);

        let alice = Uuid::from_bytes([1; 16]);
        let bob = Uuid::from_bytes([2; 16]);

        scheduler.enqueue_message(1, alice, bob, vec![1], 100);
        scheduler.enqueue_message(2, alice, bob, vec![2], 200);
        scheduler.enqueue_message(3, alice, bob, vec![3], 300);

        // LIFO order (reversed)
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![3])));
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![2])));
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![1])));
        assert_eq!(scheduler.next_message(1000), None);
    }

    #[test]
    fn test_selective_delay() {
        let mut scheduler =
            AdversarialScheduler::new(DeliveryStrategy::SelectiveDelay, MessageReordering::None);

        let alice = Uuid::from_bytes([1; 16]);
        let bob = Uuid::from_bytes([2; 16]);

        scheduler.enqueue_message(1, alice, bob, vec![1], 100);
        scheduler.enqueue_message(2, alice, bob, vec![2], 200);

        // Delay message 1 until time 5000
        scheduler.delay_message_until(1, 5000);

        // At time 1000, only message 2 should be available
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![2])));
        assert_eq!(scheduler.next_message(1000), None);

        // At time 5000, message 1 becomes available
        assert_eq!(scheduler.next_message(5000), Some((bob, vec![1])));
    }

    #[test]
    fn test_maximal_reordering_prefers_oldest() {
        let mut scheduler = AdversarialScheduler::new(
            DeliveryStrategy::MaximalReordering,
            MessageReordering::MaximalDisorder,
        );

        let alice = Uuid::from_bytes([1; 16]);
        let bob = Uuid::from_bytes([2; 16]);

        scheduler.enqueue_message(1, alice, bob, vec![1], 100);
        scheduler.enqueue_message(2, alice, bob, vec![2], 500);
        scheduler.enqueue_message(3, alice, bob, vec![3], 300);

        // Should deliver oldest first (message 1 from time 100)
        assert_eq!(scheduler.next_message(1000), Some((bob, vec![1])));
    }
}
