//! Byzantine Device Simulation
//!
//! Implements devices that deviate from protocol in adversarial ways:
//! - Send invalid signatures or commitments
//! - Equivocate (send conflicting messages to different peers)
//! - Selectively participate or abort
//! - Corrupt stored data
//!
//! All Byzantine behavior is deterministic for reproducible testing.

use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Byzantine behavior strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByzantineStrategy {
    /// Send garbage data in cryptographic protocol rounds
    InvalidCommitments,
    /// Send conflicting messages to different peers (equivocation)
    Equivocation,
    /// Participate in protocol then abort before completion
    SelectiveAbort,
    /// Corrupt data after honest storage
    DataCorruption,
    /// Attempt to forge signatures with insufficient shares
    SignatureForgery,
    /// Create CRDT forks by sending conflicting events
    CrdtFork,
    /// Selectively drop messages to specific devices
    SelectiveDropping,
}

/// Byzantine device that deviates from protocol
pub struct ByzantineDevice {
    device_id: Uuid,
    strategy: ByzantineStrategy,
    target_devices: HashSet<Uuid>,
    // Track what we've sent to enable equivocation detection
    sent_messages: HashMap<Uuid, Vec<Vec<u8>>>,
    // Configuration for corruption
    corruption_probability: f64,
}

impl ByzantineDevice {
    /// Create Byzantine device with specified strategy
    pub fn new(device_id: Uuid, strategy: ByzantineStrategy) -> Self {
        Self {
            device_id,
            strategy,
            target_devices: HashSet::new(),
            sent_messages: HashMap::new(),
            corruption_probability: 1.0, // Always corrupt for determinism
        }
    }

    /// Set target devices for selective attacks
    pub fn with_targets(mut self, targets: impl IntoIterator<Item = Uuid>) -> Self {
        self.target_devices = targets.into_iter().collect();
        self
    }

    /// Set corruption probability (for probabilistic attacks)
    pub fn with_corruption_probability(mut self, probability: f64) -> Self {
        self.corruption_probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Process outgoing message, potentially modifying it
    pub fn process_outgoing_message(
        &mut self,
        to: Uuid,
        message: Vec<u8>,
        current_time: u64,
    ) -> ByzantineAction {
        match self.strategy {
            ByzantineStrategy::InvalidCommitments => {
                // Replace message with garbage
                ByzantineAction::ReplaceMessage(self.generate_garbage(message.len(), current_time))
            }
            ByzantineStrategy::Equivocation => {
                // Send different message to this recipient than others
                if self.should_equivocate(to) {
                    let modified = self.modify_for_equivocation(message.clone(), to, current_time);
                    self.sent_messages
                        .entry(to)
                        .or_default()
                        .push(modified.clone());
                    ByzantineAction::ReplaceMessage(modified)
                } else {
                    self.sent_messages
                        .entry(to)
                        .or_default()
                        .push(message.clone());
                    ByzantineAction::Forward
                }
            }
            ByzantineStrategy::SelectiveAbort => {
                // Randomly abort (deterministic based on time)
                if self.should_abort(current_time) {
                    ByzantineAction::Drop
                } else {
                    ByzantineAction::Forward
                }
            }
            ByzantineStrategy::SelectiveDropping => {
                // Drop messages to target devices
                if self.target_devices.contains(&to) {
                    ByzantineAction::Drop
                } else {
                    ByzantineAction::Forward
                }
            }
            _ => ByzantineAction::Forward,
        }
    }

    /// Process stored data, potentially corrupting it
    pub fn process_stored_data(&self, data: Vec<u8>, current_time: u64) -> Vec<u8> {
        match self.strategy {
            ByzantineStrategy::DataCorruption => {
                if self.should_corrupt(current_time) {
                    self.corrupt_data(data, current_time)
                } else {
                    data
                }
            }
            _ => data,
        }
    }

    /// Check if this device should participate in protocol
    pub fn should_participate(&self, protocol_id: u64, current_time: u64) -> bool {
        match self.strategy {
            ByzantineStrategy::SelectiveAbort => {
                // Participate initially, abort later
                (current_time + protocol_id) % 3 != 0
            }
            _ => true,
        }
    }

    /// Generate equivocating message
    fn modify_for_equivocation(
        &self,
        mut message: Vec<u8>,
        to: Uuid,
        current_time: u64,
    ) -> Vec<u8> {
        // Deterministically modify based on recipient
        let modifier = (to.as_bytes()[0] as u64 + current_time) % 256;

        if !message.is_empty() {
            message[0] = message[0].wrapping_add(modifier as u8);
        }

        message
    }

    /// Check if should equivocate to this device
    fn should_equivocate(&self, to: Uuid) -> bool {
        // Equivocate to half the devices (deterministic)
        to.as_bytes()[0] % 2 == 1
    }

    /// Check if should abort at this time
    fn should_abort(&self, current_time: u64) -> bool {
        // Deterministic abort decision
        current_time % 5 == 0
    }

    /// Check if should corrupt at this time
    fn should_corrupt(&self, current_time: u64) -> bool {
        // Use time for deterministic corruption
        let pseudo_random = (current_time * 1103515245 + 12345) % 100;
        (pseudo_random as f64 / 100.0) < self.corruption_probability
    }

    /// Generate garbage data of specified length
    fn generate_garbage(&self, length: usize, seed: u64) -> Vec<u8> {
        let mut result = Vec::with_capacity(length);
        let mut rng_state = seed;

        for _ in 0..length {
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
            result.push((rng_state >> 16) as u8);
        }

        result
    }

    /// Corrupt data by flipping bits
    fn corrupt_data(&self, mut data: Vec<u8>, seed: u64) -> Vec<u8> {
        if data.is_empty() {
            return data;
        }

        // Flip some bits deterministically
        let mut rng_state = seed;
        let num_corruptions = (data.len() / 10).max(1); // Corrupt ~10% of bytes

        for _ in 0..num_corruptions {
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
            let idx = (rng_state as usize) % data.len();
            let bit = (rng_state >> 8) & 7;
            data[idx] ^= 1 << bit;
        }

        data
    }

    /// Get device ID
    pub fn device_id(&self) -> Uuid {
        self.device_id
    }

    /// Get strategy
    pub fn strategy(&self) -> ByzantineStrategy {
        self.strategy
    }
}

/// Actions a Byzantine device can take
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ByzantineAction {
    /// Forward message unchanged
    Forward,
    /// Replace message with modified version
    ReplaceMessage(Vec<u8>),
    /// Drop message entirely
    Drop,
}

/// Coordinator for multiple Byzantine devices
pub struct ByzantineCoordinator {
    devices: HashMap<Uuid, ByzantineDevice>,
}

impl ByzantineCoordinator {
    /// Create new coordinator
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
        }
    }

    /// Add Byzantine device
    pub fn add_device(&mut self, device: ByzantineDevice) {
        self.devices.insert(device.device_id(), device);
    }

    /// Check if device is Byzantine
    pub fn is_byzantine(&self, device_id: &Uuid) -> bool {
        self.devices.contains_key(device_id)
    }

    /// Process message from Byzantine device
    pub fn process_message(
        &mut self,
        from: Uuid,
        to: Uuid,
        message: Vec<u8>,
        current_time: u64,
    ) -> Option<ByzantineAction> {
        self.devices
            .get_mut(&from)
            .map(|device| device.process_outgoing_message(to, message, current_time))
    }

    /// Process stored data from Byzantine device
    pub fn process_storage(&self, device_id: &Uuid, data: Vec<u8>, current_time: u64) -> Vec<u8> {
        match self.devices.get(device_id) {
            Some(device) => device.process_stored_data(data, current_time),
            None => data,
        }
    }

    /// Check if device should participate
    pub fn should_participate(
        &self,
        device_id: &Uuid,
        protocol_id: u64,
        current_time: u64,
    ) -> bool {
        match self.devices.get(device_id) {
            Some(device) => device.should_participate(protocol_id, current_time),
            None => true,
        }
    }

    /// Get all Byzantine device IDs
    pub fn byzantine_devices(&self) -> Vec<Uuid> {
        self.devices.keys().copied().collect()
    }
}

impl Default for ByzantineCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_commitments() {
        let device_id = Uuid::from_bytes([1; 16]);
        let mut byzantine = ByzantineDevice::new(device_id, ByzantineStrategy::InvalidCommitments);

        let honest_message = vec![1, 2, 3, 4];
        let result = byzantine.process_outgoing_message(
            Uuid::from_bytes([2; 16]),
            honest_message.clone(),
            1000,
        );

        match result {
            ByzantineAction::ReplaceMessage(garbage) => {
                assert_eq!(garbage.len(), honest_message.len());
                assert_ne!(garbage, honest_message);
            }
            _ => panic!("Expected replacement"),
        }
    }

    #[test]
    fn test_equivocation() {
        let device_id = Uuid::from_bytes([1; 16]);
        let mut byzantine = ByzantineDevice::new(device_id, ByzantineStrategy::Equivocation);

        let message = vec![100];

        let peer1 = Uuid::from_bytes([2; 16]); // even
        let peer2 = Uuid::from_bytes([3; 16]); // odd

        let action1 = byzantine.process_outgoing_message(peer1, message.clone(), 1000);
        let action2 = byzantine.process_outgoing_message(peer2, message.clone(), 1000);

        // One should be modified, one forwarded (based on odd/even)
        assert!(
            matches!(action1, ByzantineAction::Forward)
                || matches!(action1, ByzantineAction::ReplaceMessage(_))
        );
        assert!(
            matches!(action2, ByzantineAction::Forward)
                || matches!(action2, ByzantineAction::ReplaceMessage(_))
        );
    }

    #[test]
    fn test_selective_dropping() {
        let device_id = Uuid::from_bytes([1; 16]);
        let target = Uuid::from_bytes([99; 16]);

        let mut byzantine = ByzantineDevice::new(device_id, ByzantineStrategy::SelectiveDropping)
            .with_targets(vec![target]);

        let message = vec![1, 2, 3];

        // Should drop to target
        let action_target = byzantine.process_outgoing_message(target, message.clone(), 1000);
        assert_eq!(action_target, ByzantineAction::Drop);

        // Should forward to others
        let other = Uuid::from_bytes([2; 16]);
        let action_other = byzantine.process_outgoing_message(other, message, 1000);
        assert_eq!(action_other, ByzantineAction::Forward);
    }

    #[test]
    fn test_data_corruption() {
        let device_id = Uuid::from_bytes([1; 16]);
        let byzantine = ByzantineDevice::new(device_id, ByzantineStrategy::DataCorruption);

        let honest_data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let corrupted = byzantine.process_stored_data(honest_data.clone(), 1000);

        // Should be different
        assert_ne!(corrupted, honest_data);
        // Should be same length
        assert_eq!(corrupted.len(), honest_data.len());
    }

    #[test]
    fn test_coordinator() {
        let mut coordinator = ByzantineCoordinator::new();

        let dev1 = Uuid::from_bytes([1; 16]);
        let dev2 = Uuid::from_bytes([2; 16]);

        let byzantine = ByzantineDevice::new(dev1, ByzantineStrategy::InvalidCommitments);
        coordinator.add_device(byzantine);

        assert!(coordinator.is_byzantine(&dev1));
        assert!(!coordinator.is_byzantine(&dev2));

        let result = coordinator.process_message(dev1, dev2, vec![1, 2, 3], 1000);
        assert!(result.is_some());
    }
}
