// Core types for BeeKEM CGKA implementation

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Unique identifier for a group member
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MemberId(pub String);

impl MemberId {
    pub fn new(id: &str) -> Self {
        Self(id.to_string())
    }
    
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Group epoch number for CGKA operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Epoch(pub u64);

impl Epoch {
    pub fn initial() -> Self {
        Self(0)
    }
    
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }
    
    pub fn value(&self) -> u64 {
        self.0
    }
}

/// Tree position in the BeeKEM binary tree
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TreePosition(pub u32);

impl TreePosition {
    pub fn leaf(index: u32) -> Self {
        Self(2 * index)
    }
    
    pub fn parent(&self) -> Self {
        Self(self.0 / 2)
    }
    
    pub fn left_child(&self) -> Self {
        Self(2 * self.0)
    }
    
    pub fn right_child(&self) -> Self {
        Self(2 * self.0 + 1)
    }
    
    pub fn is_leaf(&self) -> bool {
        self.0 % 2 == 0
    }
}

/// Public key for CGKA operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKey(pub Vec<u8>);

impl PublicKey {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Private key for CGKA operations  
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateKey(pub Vec<u8>);

impl PrivateKey {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Key package for member initialization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPackage {
    pub member_id: MemberId,
    pub init_key: PublicKey,
    pub signature: Vec<u8>,
    pub created_at: u64,
}

/// Application secret derived from group key
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationSecret {
    pub secret: Vec<u8>,
    pub epoch: Epoch,
    pub context: String,
}

impl ApplicationSecret {
    pub fn derive_key(&self, purpose: &str) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.secret);
        hasher.update(purpose.as_bytes());
        hasher.update(&self.epoch.0.to_le_bytes());
        hasher.update(self.context.as_bytes());
        hasher.finalize().as_bytes().to_vec()
    }
}

/// Member roster for deterministic ordering
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    pub members: BTreeMap<MemberId, TreePosition>,
    pub epoch: Epoch,
    pub size: u32,
}

impl Roster {
    pub fn new(epoch: Epoch) -> Self {
        Self {
            members: BTreeMap::new(),
            epoch,
            size: 0,
        }
    }
    
    pub fn add_member(&mut self, member_id: MemberId) -> TreePosition {
        let position = TreePosition::leaf(self.size);
        self.members.insert(member_id, position);
        self.size += 1;
        position
    }
    
    pub fn remove_member(&mut self, member_id: &MemberId) -> Option<TreePosition> {
        self.members.remove(member_id)
    }
    
    pub fn get_position(&self, member_id: &MemberId) -> Option<TreePosition> {
        self.members.get(member_id).copied()
    }
    
    pub fn is_member(&self, member_id: &MemberId) -> bool {
        self.members.contains_key(member_id)
    }
    
    pub fn member_count(&self) -> usize {
        self.members.len()
    }
}


/// BeeKEM tree node containing cryptographic material
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeNode {
    pub position: TreePosition,
    pub public_key: Option<PublicKey>,
    pub private_key: Option<PrivateKey>,
    pub unmerged_leaves: Vec<TreePosition>,
}

impl TreeNode {
    pub fn new(position: TreePosition) -> Self {
        Self {
            position,
            public_key: None,
            private_key: None,
            unmerged_leaves: Vec::new(),
        }
    }
    
    pub fn with_keypair(position: TreePosition, public_key: PublicKey, private_key: PrivateKey) -> Self {
        Self {
            position,
            public_key: Some(public_key),
            private_key: Some(private_key),
            unmerged_leaves: Vec::new(),
        }
    }
}