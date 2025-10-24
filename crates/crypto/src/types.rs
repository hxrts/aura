// Shared types for Aura cryptographic system

use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

/// Device identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    /// Create a new device ID using injected effects (for production/testing)
    pub fn new_with_effects(effects: &crate::Effects) -> Self {
        DeviceId(effects.gen_uuid())
    }
    
    
    /// Create device ID from string, generating random UUID if parsing fails
    pub fn from_string_with_effects(s: &str, effects: &crate::Effects) -> Self {
        DeviceId(Uuid::parse_str(s).unwrap_or_else(|_| effects.gen_uuid()))
    }
}


impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for DeviceId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DeviceId(Uuid::parse_str(s)?))
    }
}

/// Guardian identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct GuardianId(pub Uuid);

impl GuardianId {
    /// Create a new guardian ID using injected effects (for production/testing)
    pub fn new_with_effects(effects: &crate::Effects) -> Self {
        GuardianId(effects.gen_uuid())
    }
    
}


impl std::fmt::Display for GuardianId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for GuardianId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(GuardianId(Uuid::parse_str(s)?))
    }
}

/// Account identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AccountId(pub Uuid);

impl AccountId {
    /// Create a new account ID using injected effects (for production/testing)
    pub fn new_with_effects(effects: &crate::Effects) -> Self {
        AccountId(effects.gen_uuid())
    }
    
}


impl std::fmt::Display for AccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AccountId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(AccountId(Uuid::parse_str(s)?))
    }
}

/// Merkle proof for commitment verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Hash of the commitment being proven
    pub commitment_hash: [u8; 32],
    /// Sibling hashes along the path to root
    pub siblings: Vec<[u8; 32]>,
    /// Path direction indices (true = right, false = left)
    pub path_indices: Vec<bool>, // true = right, false = left
}

impl MerkleProof {
    /// Verify this proof against a Merkle root
    pub fn verify(&self, root: &[u8; 32]) -> bool {
        let mut current_hash = self.commitment_hash;

        for (sibling, is_right) in self.siblings.iter().zip(self.path_indices.iter()) {
            current_hash = if *is_right {
                // Current is left child
                compute_parent_hash(&current_hash, sibling)
            } else {
                // Current is right child
                compute_parent_hash(sibling, &current_hash)
            };
        }

        current_hash == *root
    }
}

/// Compute parent hash from left and right children
fn compute_parent_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(left);
    hasher.update(right);
    *hasher.finalize().as_bytes()
}