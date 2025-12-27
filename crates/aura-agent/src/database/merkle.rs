//! Merkle tree construction for integrity verification.

/// Merkle tree node for integrity verification
#[derive(Debug, Clone)]
pub(crate) struct MerkleNode {
    pub(crate) hash: [u8; 32],
    /// Left child (used for Merkle proof generation)
    #[allow(dead_code)]
    _left: Option<Box<MerkleNode>>,
    /// Right child (used for Merkle proof generation)
    #[allow(dead_code)]
    _right: Option<Box<MerkleNode>>,
}

impl MerkleNode {
    fn branch(left: MerkleNode, right: MerkleNode) -> Self {
        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(&left.hash);
        combined.extend_from_slice(&right.hash);
        let hash = aura_core::hash::hash(&combined);
        Self {
            hash,
            _left: Some(Box::new(left)),
            _right: Some(Box::new(right)),
        }
    }
}

/// Build a Merkle tree from leaf hashes
pub(crate) fn build_merkle_tree(leaves: Vec<[u8; 32]>) -> Option<MerkleNode> {
    if leaves.is_empty() {
        return None;
    }

    let mut nodes: Vec<MerkleNode> = leaves
        .into_iter()
        .map(|hash| MerkleNode {
            hash,
            _left: None,
            _right: None,
        })
        .collect();

    while nodes.len() > 1 {
        let mut next_level = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            if i + 1 < nodes.len() {
                let left = nodes[i].clone();
                let right = nodes[i + 1].clone();
                next_level.push(MerkleNode::branch(left, right));
                i += 2;
            } else {
                // Odd node - promote to next level
                next_level.push(nodes[i].clone());
                i += 1;
            }
        }
        nodes = next_level;
    }

    nodes.into_iter().next()
}
