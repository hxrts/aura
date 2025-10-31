//! Network Adversary Simulation
//!
//! Implements network-level attacks:
//! - Man-in-the-middle (message interception and modification)
//! - Denial of Service (message flooding)
//! - Eclipse attacks (isolating nodes)
//! - Sybil attacks (fake identities)
//!
//! All attacks are deterministic for reproducible testing.

use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Types of network attacks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkAttack {
    /// Man-in-the-middle: intercept and modify messages
    ManInTheMiddle,
    /// Denial of Service: flood with messages
    DenialOfService,
    /// Eclipse: isolate target from honest peers
    Eclipse,
    /// Sybil: create fake identities
    Sybil,
    /// Partition: split network into groups
    Partition,
}

/// Network adversary that controls message delivery
pub struct NetworkAdversary {
    attack_type: NetworkAttack,
    // For MITM
    mitm_targets: HashSet<(Uuid, Uuid)>, // (from, to) pairs to intercept
    // For DoS
    flood_target: Option<Uuid>,
    flood_rate: usize, // messages per time unit
    // For Eclipse
    eclipse_target: Option<Uuid>,
    controlled_peers: HashSet<Uuid>,
    // For Sybil
    sybil_identities: HashSet<Uuid>,
    // For Partition
    partitions: Vec<HashSet<Uuid>>,
}

impl NetworkAdversary {
    /// Create new network adversary
    pub fn new(attack_type: NetworkAttack) -> Self {
        Self {
            attack_type,
            mitm_targets: HashSet::new(),
            flood_target: None,
            flood_rate: 1000,
            eclipse_target: None,
            controlled_peers: HashSet::new(),
            sybil_identities: HashSet::new(),
            partitions: Vec::new(),
        }
    }

    /// Configure MITM attack on specific communication pairs
    pub fn with_mitm_targets(mut self, targets: impl IntoIterator<Item = (Uuid, Uuid)>) -> Self {
        self.mitm_targets = targets.into_iter().collect();
        self
    }

    /// Configure DoS attack
    pub fn with_dos_target(mut self, target: Uuid, rate: usize) -> Self {
        self.flood_target = Some(target);
        self.flood_rate = rate;
        self
    }

    /// Configure eclipse attack
    pub fn with_eclipse_target(
        mut self,
        target: Uuid,
        controlled_peers: impl IntoIterator<Item = Uuid>,
    ) -> Self {
        self.eclipse_target = Some(target);
        self.controlled_peers = controlled_peers.into_iter().collect();
        self
    }

    /// Configure Sybil attack
    pub fn with_sybil_identities(mut self, identities: impl IntoIterator<Item = Uuid>) -> Self {
        self.sybil_identities = identities.into_iter().collect();
        self
    }

    /// Configure network partition
    pub fn with_partitions(mut self, partitions: Vec<HashSet<Uuid>>) -> Self {
        self.partitions = partitions;
        self
    }

    /// Check if message should be delivered
    pub fn should_deliver(&self, from: Uuid, to: Uuid) -> bool {
        match self.attack_type {
            NetworkAttack::Eclipse => {
                if Some(to) == self.eclipse_target {
                    // Only deliver from controlled peers
                    self.controlled_peers.contains(&from)
                } else {
                    true
                }
            }
            NetworkAttack::Partition => {
                // Check if from and to are in same partition
                self.same_partition(from, to)
            }
            _ => true,
        }
    }

    /// Process message, potentially modifying it
    pub fn process_message(
        &self,
        from: Uuid,
        to: Uuid,
        message: Vec<u8>,
        current_time: u64,
    ) -> NetworkAction {
        match self.attack_type {
            NetworkAttack::ManInTheMiddle => {
                if self.mitm_targets.contains(&(from, to)) {
                    // Tamper with message
                    NetworkAction::Modify(self.tamper_message(message, current_time))
                } else {
                    NetworkAction::Deliver
                }
            }
            NetworkAttack::DenialOfService => {
                if Some(to) == self.flood_target {
                    // Generate flood messages
                    let flood_messages = self.generate_flood_messages(current_time);
                    NetworkAction::Flood(flood_messages)
                } else {
                    NetworkAction::Deliver
                }
            }
            NetworkAttack::Eclipse | NetworkAttack::Partition => {
                if self.should_deliver(from, to) {
                    NetworkAction::Deliver
                } else {
                    NetworkAction::Drop
                }
            }
            NetworkAttack::Sybil => {
                // Deliver, but track Sybil identities
                NetworkAction::Deliver
            }
        }
    }

    /// Check if device is a Sybil identity
    pub fn is_sybil(&self, device_id: Uuid) -> bool {
        self.sybil_identities.contains(&device_id)
    }

    /// Get controlled peers for eclipse attack
    pub fn controlled_peers(&self) -> &HashSet<Uuid> {
        &self.controlled_peers
    }

    /// Tamper with message (deterministic based on time)
    fn tamper_message(&self, mut message: Vec<u8>, current_time: u64) -> Vec<u8> {
        if message.is_empty() {
            return message;
        }

        // Flip some bits based on current time
        let mut rng_state = current_time;
        for i in 0..message.len().min(5) {
            // Tamper first few bytes
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
            let bit = (rng_state >> 8) & 7;
            message[i] ^= 1 << bit;
        }

        message
    }

    /// Generate flood messages for DoS
    fn generate_flood_messages(&self, current_time: u64) -> Vec<Vec<u8>> {
        let mut messages = Vec::new();
        let mut rng_state = current_time;

        for _ in 0..self.flood_rate {
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
            let message_len = ((rng_state % 100) + 50) as usize; // 50-150 bytes

            let mut message = Vec::with_capacity(message_len);
            for _ in 0..message_len {
                rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                message.push((rng_state >> 16) as u8);
            }
            messages.push(message);
        }

        messages
    }

    /// Check if two devices are in same partition
    fn same_partition(&self, dev1: Uuid, dev2: Uuid) -> bool {
        for partition in &self.partitions {
            if partition.contains(&dev1) && partition.contains(&dev2) {
                return true;
            }
        }
        // If not in any partition, consider them isolated
        false
    }

    /// Get attack type
    pub fn attack_type(&self) -> NetworkAttack {
        self.attack_type
    }
}

/// Actions a network adversary can take
#[derive(Debug, Clone)]
pub enum NetworkAction {
    /// Deliver message unchanged
    Deliver,
    /// Modify message content
    Modify(Vec<u8>),
    /// Drop message
    Drop,
    /// Deliver plus flood with additional messages
    Flood(Vec<Vec<u8>>),
}

/// Statistics tracker for network adversary
pub struct NetworkAdversaryStats {
    messages_intercepted: usize,
    messages_modified: usize,
    messages_dropped: usize,
    flood_messages_sent: usize,
}

impl NetworkAdversaryStats {
    /// Create new stats tracker
    pub fn new() -> Self {
        Self {
            messages_intercepted: 0,
            messages_modified: 0,
            messages_dropped: 0,
            flood_messages_sent: 0,
        }
    }

    /// Record intercepted message
    pub fn record_intercept(&mut self) {
        self.messages_intercepted += 1;
    }

    /// Record modified message
    pub fn record_modification(&mut self) {
        self.messages_modified += 1;
    }

    /// Record dropped message
    pub fn record_drop(&mut self) {
        self.messages_dropped += 1;
    }

    /// Record flood messages
    pub fn record_flood(&mut self, count: usize) {
        self.flood_messages_sent += count;
    }

    /// Get stats
    pub fn stats(&self) -> (usize, usize, usize, usize) {
        (
            self.messages_intercepted,
            self.messages_modified,
            self.messages_dropped,
            self.flood_messages_sent,
        )
    }
}

impl Default for NetworkAdversaryStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mitm_attack() {
        let alice = Uuid::from_bytes([1; 16]);
        let bob = Uuid::from_bytes([2; 16]);

        let adversary = NetworkAdversary::new(NetworkAttack::ManInTheMiddle)
            .with_mitm_targets(vec![(alice, bob)]);

        let message = vec![1, 2, 3, 4];
        let action = adversary.process_message(alice, bob, message.clone(), 1000);

        match action {
            NetworkAction::Modify(modified) => {
                assert_ne!(modified, message);
            }
            _ => panic!("Expected modification"),
        }
    }

    #[test]
    fn test_dos_attack() {
        let target = Uuid::from_bytes([1; 16]);
        let attacker = Uuid::from_bytes([2; 16]);

        let adversary =
            NetworkAdversary::new(NetworkAttack::DenialOfService).with_dos_target(target, 100);

        let action = adversary.process_message(attacker, target, vec![1], 1000);

        match action {
            NetworkAction::Flood(messages) => {
                assert_eq!(messages.len(), 100);
            }
            _ => panic!("Expected flood"),
        }
    }

    #[test]
    fn test_eclipse_attack() {
        let target = Uuid::from_bytes([1; 16]);
        let controlled = Uuid::from_bytes([2; 16]);
        let honest = Uuid::from_bytes([3; 16]);

        let adversary = NetworkAdversary::new(NetworkAttack::Eclipse)
            .with_eclipse_target(target, vec![controlled]);

        // Should deliver from controlled peer
        assert!(adversary.should_deliver(controlled, target));

        // Should not deliver from honest peer
        assert!(!adversary.should_deliver(honest, target));
    }

    #[test]
    fn test_partition_attack() {
        let dev1 = Uuid::from_bytes([1; 16]);
        let dev2 = Uuid::from_bytes([2; 16]);
        let dev3 = Uuid::from_bytes([3; 16]);
        let dev4 = Uuid::from_bytes([4; 16]);

        let partition1: HashSet<_> = vec![dev1, dev2].into_iter().collect();
        let partition2: HashSet<_> = vec![dev3, dev4].into_iter().collect();

        let adversary = NetworkAdversary::new(NetworkAttack::Partition)
            .with_partitions(vec![partition1, partition2]);

        // Within partition 1
        assert!(adversary.should_deliver(dev1, dev2));

        // Within partition 2
        assert!(adversary.should_deliver(dev3, dev4));

        // Across partitions
        assert!(!adversary.should_deliver(dev1, dev3));
        assert!(!adversary.should_deliver(dev2, dev4));
    }

    #[test]
    fn test_sybil_attack() {
        let sybil1 = Uuid::from_bytes([99; 16]);
        let sybil2 = Uuid::from_bytes([98; 16]);
        let honest = Uuid::from_bytes([1; 16]);

        let adversary =
            NetworkAdversary::new(NetworkAttack::Sybil).with_sybil_identities(vec![sybil1, sybil2]);

        assert!(adversary.is_sybil(sybil1));
        assert!(adversary.is_sybil(sybil2));
        assert!(!adversary.is_sybil(honest));
    }

    #[test]
    fn test_stats_tracking() {
        let mut stats = NetworkAdversaryStats::new();

        stats.record_intercept();
        stats.record_modification();
        stats.record_drop();
        stats.record_flood(100);

        let (intercepted, modified, dropped, flooded) = stats.stats();
        assert_eq!(intercepted, 1);
        assert_eq!(modified, 1);
        assert_eq!(dropped, 1);
        assert_eq!(flooded, 100);
    }
}
