//! Delivery and ordering effects for CRDT protocols
//!
//! This module provides delivery guarantees required by different CRDT types:
//! - Causal broadcast for CmRDTs
//! - At-least-once delivery with deduplication
//! - Gossip ticking for periodic synchronization

use aura_core::identifiers::{DeviceId, SessionId};
use aura_journal::CausalContext;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Topic identifier for delivery effects
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TopicId {
    /// Session this topic belongs to
    pub session_id: SessionId,
    /// Topic name within the session
    pub topic_name: String,
}

impl TopicId {
    /// Create a new topic ID
    pub fn new(session_id: SessionId, topic_name: String) -> Self {
        Self {
            session_id,
            topic_name,
        }
    }

    /// Create a topic ID for CRDT synchronization
    pub fn crdt_sync(session_id: SessionId, crdt_type: &str) -> Self {
        Self::new(session_id, format!("crdt-sync-{}", crdt_type))
    }

    /// Create a topic ID for operation broadcast
    pub fn operation_broadcast(session_id: SessionId, crdt_type: &str) -> Self {
        Self::new(session_id, format!("op-broadcast-{}", crdt_type))
    }
}

/// Delivery effects for CRDT communication
///
/// These effects provide the necessary delivery guarantees for different
/// CRDT types to maintain their semantic properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryEffect {
    /// Causal broadcast ensures operations are delivered in causal order
    ///
    /// Required for CmRDTs to maintain commutativity guarantees.
    /// Operations with causal dependencies are delivered only after
    /// their dependencies have been applied.
    CausalBroadcast {
        /// Topic for this causal broadcast group
        topic: TopicId,
        /// Sender of the message
        sender: DeviceId,
        /// Message payload (serialized)
        payload: Vec<u8>,
        /// Vector clock or causal context
        causal_context: Vec<u8>,
    },

    /// At-least-once delivery with deduplication support
    ///
    /// Ensures messages are delivered at least once, with handlers
    /// responsible for deduplication using message IDs.
    AtLeastOnce {
        /// Topic for delivery
        topic: TopicId,
        /// Sender of the message
        sender: DeviceId,
        /// Unique message ID for deduplication
        message_id: String,
        /// Message payload (serialized)
        payload: Vec<u8>,
        /// Maximum number of retries
        max_retries: u32,
        /// Retry interval
        retry_interval: Duration,
    },

    /// Periodic gossip tick for state synchronization
    ///
    /// Triggers periodic exchange of CRDT states or deltas between peers.
    /// Used for anti-entropy and eventual consistency maintenance.
    GossipTick {
        /// Topic for gossip
        topic: TopicId,
        /// Interval between gossip rounds
        interval: Duration,
        /// List of peers to gossip with
        peers: Vec<DeviceId>,
        /// Gossip strategy configuration
        strategy: GossipStrategy,
    },

    /// Trigger digest exchange for repair protocols
    ///
    /// Initiates repair by comparing operation digests between replicas
    /// and requesting missing operations.
    ExchangeDigest {
        /// Topic for digest exchange
        topic: TopicId,
        /// Peer to exchange digests with
        peer: DeviceId,
        /// Local operation digest
        local_digest: Vec<u8>,
    },
}

/// Gossip strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipStrategy {
    /// Random peer selection
    Random {
        /// Number of peers to select per round
        fanout: usize,
    },

    /// Round-robin peer selection
    RoundRobin,

    /// Epidemic spreading with infection probability
    Epidemic {
        /// Probability of gossiping in each round
        infection_probability: f64,
        /// Maximum number of rounds for epidemic spreading
        max_rounds: u32,
    },

    /// All-to-all gossip (for small networks)
    All,
}

/// Delivery guarantee level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryGuarantee {
    /// Best effort delivery (no guarantees)
    BestEffort,
    /// At most once delivery (no duplicates, may lose messages)
    AtMostOnce,
    /// At least once delivery (no lost messages, may have duplicates)
    AtLeastOnce,
    /// Exactly once delivery (no duplicates, no lost messages)
    ExactlyOnce,
    /// Causal ordering (respects causal dependencies)
    CausalOrder,
    /// Total ordering (global order across all replicas)
    TotalOrder,
}

/// Delivery effect configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryConfig {
    /// Required delivery guarantee
    pub guarantee: DeliveryGuarantee,
    /// Maximum message retention time
    pub max_retention: Duration,
    /// Buffer size for out-of-order messages
    pub buffer_size: usize,
    /// Enable compression for large messages
    pub enable_compression: bool,
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self {
            guarantee: DeliveryGuarantee::AtLeastOnce,
            max_retention: Duration::from_secs(300), // 5 minutes
            buffer_size: 1000,
            enable_compression: true,
        }
    }
}

impl Default for GossipStrategy {
    fn default() -> Self {
        GossipStrategy::Random { fanout: 3 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_id_creation() {
        let session_id = SessionId::new();
        let topic = TopicId::new(session_id, "test-topic".to_string());

        assert_eq!(topic.session_id, session_id);
        assert_eq!(topic.topic_name, "test-topic");
    }

    #[test]
    fn test_topic_id_crdt_sync() {
        let session_id = SessionId::new();
        let topic = TopicId::crdt_sync(session_id, "journal-map");

        assert_eq!(topic.topic_name, "crdt-sync-journal-map");
    }

    #[test]
    fn test_causal_context_empty() {
        let ctx = CausalContext::empty();
        assert!(ctx.vector_clock.is_empty());
        assert!(ctx.dependencies.is_none());
    }

    #[test]
    fn test_causal_context_happens_before() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();

        let mut ctx1 = CausalContext::empty();
        ctx1.vector_clock.insert(device_a, 1);
        ctx1.vector_clock.insert(device_b, 0);

        let mut ctx2 = CausalContext::empty();
        ctx2.vector_clock.insert(device_a, 2);
        ctx2.vector_clock.insert(device_b, 0);

        assert!(ctx1.happens_before(&ctx2));
        assert!(!ctx2.happens_before(&ctx1));
    }

    #[test]
    fn test_causal_context_concurrent() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();

        let mut ctx1 = CausalContext::empty();
        ctx1.vector_clock.insert(device_a, 1);
        ctx1.vector_clock.insert(device_b, 0);

        let mut ctx2 = CausalContext::empty();
        ctx2.vector_clock.insert(device_a, 0);
        ctx2.vector_clock.insert(device_b, 1);

        assert!(ctx1.is_concurrent_with(&ctx2));
        assert!(ctx2.is_concurrent_with(&ctx1));
    }

    #[test]
    fn test_causal_context_increment() {
        let device = DeviceId::new();
        let mut ctx = CausalContext::empty();

        ctx.increment(device);
        assert_eq!(ctx.vector_clock.get(&device), Some(&1));

        ctx.increment(device);
        assert_eq!(ctx.vector_clock.get(&device), Some(&2));
    }

    #[test]
    fn test_causal_context_merge() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();

        let mut ctx1 = CausalContext::empty();
        ctx1.vector_clock.insert(device_a, 2);
        ctx1.vector_clock.insert(device_b, 1);

        let mut ctx2 = CausalContext::empty();
        ctx2.vector_clock.insert(device_a, 1);
        ctx2.vector_clock.insert(device_b, 3);

        ctx1.merge(&ctx2);

        assert_eq!(ctx1.vector_clock.get(&device_a), Some(&2)); // max(2, 1)
        assert_eq!(ctx1.vector_clock.get(&device_b), Some(&3)); // max(1, 3)
    }
}
