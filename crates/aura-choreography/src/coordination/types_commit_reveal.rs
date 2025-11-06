//! Generic commit-reveal protocol pattern for Byzantine fault tolerance
//!
//! This module provides reusable types for commit-reveal protocols, where participants
//! first commit to a value (by publishing its hash), then later reveal the actual value.
//! This prevents participants from adapting their choices based on others' values.

use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Commitment to a value (hash only, value hidden)
///
/// Generic over the type being committed to. The actual value is not included,
/// only its hash. This prevents others from seeing the value until the reveal phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commitment<T> {
    /// Device making the commitment
    pub device_id: DeviceId,

    /// Blake3 hash of (value || nonce || epoch)
    pub commitment_hash: [u8; 32],

    /// Epoch for replay protection
    pub epoch: u64,

    /// Phantom data for type safety
    #[serde(skip)]
    pub _phantom: PhantomData<T>,
}

impl<T> Commitment<T> {
    /// Create a new commitment
    pub fn new(device_id: DeviceId, commitment_hash: [u8; 32], epoch: u64) -> Self {
        Self {
            device_id,
            commitment_hash,
            epoch,
            _phantom: PhantomData,
        }
    }
}

/// Reveal of a previously committed value
///
/// Contains the actual value and nonce, allowing verification against the commitment hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reveal<T> {
    /// Device revealing the value
    pub device_id: DeviceId,

    /// The actual value being revealed
    pub value: T,

    /// Nonce used in the commitment
    pub nonce: [u8; 32],

    /// Epoch for replay protection
    pub epoch: u64,
}

impl<T> Reveal<T> {
    /// Create a new reveal
    pub fn new(device_id: DeviceId, value: T, nonce: [u8; 32], epoch: u64) -> Self {
        Self {
            device_id,
            value,
            nonce,
            epoch,
        }
    }
}

/// Collection of commitments from all participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedCommitments<T> {
    /// All commitments received
    pub commitments: Vec<Commitment<T>>,

    /// Epoch when commitments were collected
    pub epoch: u64,
}

impl<T> CollectedCommitments<T> {
    /// Create a new collection
    pub fn new(commitments: Vec<Commitment<T>>, epoch: u64) -> Self {
        Self { commitments, epoch }
    }

    /// Get number of commitments
    pub fn len(&self) -> usize {
        self.commitments.len()
    }

    /// Check if collection is empty
    pub fn is_empty(&self) -> bool {
        self.commitments.is_empty()
    }
}

/// Collection of reveals from all participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectedReveals<T> {
    /// All reveals received
    pub reveals: Vec<Reveal<T>>,

    /// Epoch when reveals were collected
    pub epoch: u64,
}

impl<T> CollectedReveals<T> {
    /// Create a new collection
    pub fn new(reveals: Vec<Reveal<T>>, epoch: u64) -> Self {
        Self { reveals, epoch }
    }

    /// Get number of reveals
    pub fn len(&self) -> usize {
        self.reveals.len()
    }

    /// Check if collection is empty
    pub fn is_empty(&self) -> bool {
        self.reveals.is_empty()
    }
}

/// Helper functions for commit-reveal protocol
pub mod helpers {
    use super::*;

    /// Compute commitment hash for a value
    ///
    /// Hash of: value_bytes || nonce || epoch_bytes
    pub fn compute_commitment_hash(value_bytes: &[u8], nonce: &[u8; 32], epoch: u64) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(value_bytes);
        hasher.update(nonce);
        hasher.update(&epoch.to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Verify a reveal matches its commitment
    pub fn verify_reveal<T: serde::Serialize>(
        reveal: &Reveal<T>,
        commitment: &Commitment<T>,
    ) -> Result<(), String> {
        // Verify device IDs match
        if reveal.device_id != commitment.device_id {
            return Err("Device ID mismatch".to_string());
        }

        // Verify epochs match
        if reveal.epoch != commitment.epoch {
            return Err("Epoch mismatch".to_string());
        }

        // Serialize value
        let value_bytes = bincode::serialize(&reveal.value)
            .map_err(|e| format!("Failed to serialize value: {}", e))?;

        // Compute expected hash
        let expected_hash = compute_commitment_hash(&value_bytes, &reveal.nonce, reveal.epoch);

        // Verify hash matches
        if expected_hash != commitment.commitment_hash {
            return Err("Commitment hash mismatch".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceId;

    #[test]
    fn test_commitment_creation() {
        let device_id = DeviceId::new();
        let hash = [0u8; 32];
        let epoch = 42;

        let commitment = Commitment::<u64>::new(device_id, hash, epoch);
        assert_eq!(commitment.device_id, device_id);
        assert_eq!(commitment.commitment_hash, hash);
        assert_eq!(commitment.epoch, epoch);
    }

    #[test]
    fn test_reveal_creation() {
        let device_id = DeviceId::new();
        let value = 123u64;
        let nonce = [1u8; 32];
        let epoch = 42;

        let reveal = Reveal::new(device_id, value, nonce, epoch);
        assert_eq!(reveal.device_id, device_id);
        assert_eq!(reveal.value, value);
        assert_eq!(reveal.nonce, nonce);
        assert_eq!(reveal.epoch, epoch);
    }

    #[test]
    fn test_commit_reveal_workflow() {
        let device_id = DeviceId::new();
        let value = 123u64;
        let nonce = [1u8; 32];
        let epoch = 42;

        // Serialize value
        let value_bytes = bincode::serialize(&value).unwrap();

        // Create commitment
        let hash = helpers::compute_commitment_hash(&value_bytes, &nonce, epoch);
        let commitment = Commitment::new(device_id, hash, epoch);

        // Create reveal
        let reveal = Reveal::new(device_id, value, nonce, epoch);

        // Verify reveal matches commitment
        assert!(helpers::verify_reveal(&reveal, &commitment).is_ok());
    }

    #[test]
    fn test_verify_reveal_fails_on_wrong_value() {
        let device_id = DeviceId::new();
        let nonce = [1u8; 32];
        let epoch = 42;

        // Create commitment for value 123
        let value_bytes = bincode::serialize(&123u64).unwrap();
        let hash = helpers::compute_commitment_hash(&value_bytes, &nonce, epoch);
        let commitment = Commitment::new(device_id, hash, epoch);

        // Try to reveal different value
        let reveal = Reveal::new(device_id, 456u64, nonce, epoch);

        // Should fail
        assert!(helpers::verify_reveal(&reveal, &commitment).is_err());
    }

    #[test]
    fn test_collected_commitments() {
        let commitments = vec![
            Commitment::<u64>::new(DeviceId::new(), [0u8; 32], 1),
            Commitment::<u64>::new(DeviceId::new(), [1u8; 32], 1),
        ];

        let collected = CollectedCommitments::new(commitments, 1);
        assert_eq!(collected.len(), 2);
        assert!(!collected.is_empty());
        assert_eq!(collected.epoch, 1);
    }
}
