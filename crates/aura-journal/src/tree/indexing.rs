//! LBBT Indexing
//!
//! Implements TreeKEM-style deterministic node indexing for the left-balanced binary tree.
//!
//! ## Index Scheme
//!
//! - Leaves: `NodeIndex = 2 * LeafIndex`
//! - Branches: Derived from left/right child indices
//! - Root: Always the maximum index
//!
//! This scheme ensures:
//! - Deterministic indices regardless of insertion order
//! - Stable leaf indices (never change once assigned)
//! - Efficient path calculations

use serde::{Deserialize, Serialize};
use std::fmt;

/// Index for a leaf node in the tree
///
/// Leaves are numbered 0, 1, 2, ... in left-to-right order.
/// The corresponding NodeIndex is `2 * LeafIndex`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LeafIndex(pub usize);

impl LeafIndex {
    /// Create a new leaf index
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    /// Get the node index for this leaf
    pub fn to_node_index(self) -> NodeIndex {
        NodeIndex(2 * self.0)
    }

    /// Get the raw index value
    pub fn value(self) -> usize {
        self.0
    }
}

impl fmt::Display for LeafIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}", self.0)
    }
}

/// Index for any node (leaf or branch) in the tree
///
/// Uses TreeKEM indexing:
/// - Leaf indices: even numbers (0, 2, 4, 6, ...)
/// - Branch indices: odd numbers (1, 3, 5, 7, ...)
/// - Root: highest index in the tree
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeIndex(pub usize);

impl NodeIndex {
    /// Create a new node index
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    /// Check if this is a leaf node index (even)
    pub fn is_leaf(self) -> bool {
        self.0 % 2 == 0
    }

    /// Check if this is a branch node index (odd)
    pub fn is_branch(self) -> bool {
        self.0 % 2 == 1
    }

    /// Convert to leaf index if this is a leaf node
    pub fn to_leaf_index(self) -> Option<LeafIndex> {
        if self.is_leaf() {
            Some(LeafIndex(self.0 / 2))
        } else {
            None
        }
    }

    /// Get the raw index value
    pub fn value(self) -> usize {
        self.0
    }

    /// Calculate the parent node index
    ///
    /// The parent of node x is: ((x >> 1) | 1) << 1
    /// This works for both leaves and branches.
    pub fn parent(self, num_leaves: usize) -> Option<NodeIndex> {
        if self.value() >= root_index(num_leaves).value() {
            // Already at root
            None
        } else {
            let x = self.0;
            let parent = ((x >> 1) | 1) << 1;
            Some(NodeIndex(parent))
        }
    }

    /// Calculate the left child index
    pub fn left_child(self) -> Option<NodeIndex> {
        if self.is_leaf() {
            None
        } else {
            Some(NodeIndex(self.0 ^ (self.0 & 1)))
        }
    }

    /// Calculate the right child index
    pub fn right_child(self) -> Option<NodeIndex> {
        if self.is_leaf() {
            None
        } else {
            Some(NodeIndex(self.0 ^ 1))
        }
    }

    /// Get the sibling node index
    pub fn sibling(self) -> NodeIndex {
        NodeIndex(self.0 ^ 1)
    }
}

impl fmt::Display for NodeIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_leaf() {
            write!(f, "N{}(leaf)", self.0)
        } else {
            write!(f, "N{}(branch)", self.0)
        }
    }
}

/// Calculate the root node index for a tree with the given number of leaves
///
/// The root is always the highest index in the tree.
/// For n leaves, the root index is determined by the LBBT structure.
pub fn root_index(num_leaves: usize) -> NodeIndex {
    if num_leaves == 0 {
        panic!("Cannot calculate root index for empty tree");
    }
    if num_leaves == 1 {
        return NodeIndex(0); // Single leaf is the root
    }

    // The root index is 2 * (num_leaves - 1) + 1
    // This follows from the LBBT property
    NodeIndex(2 * (num_leaves - 1) + 1)
}

/// Calculate the path from a leaf to the root
///
/// Returns a vector of node indices starting from the leaf and ending at the root.
pub fn path_to_root(leaf_index: LeafIndex, num_leaves: usize) -> Vec<NodeIndex> {
    let mut path = Vec::new();
    let mut current = leaf_index.to_node_index();
    let root = root_index(num_leaves);

    path.push(current);

    while current != root {
        if let Some(parent) = current.parent(num_leaves) {
            current = parent;
            path.push(current);
        } else {
            break;
        }
    }

    path
}

/// Calculate the direct path from a leaf to the root (excluding the leaf itself)
///
/// The direct path contains only the ancestors, not the leaf.
pub fn direct_path(leaf_index: LeafIndex, num_leaves: usize) -> Vec<NodeIndex> {
    let full_path = path_to_root(leaf_index, num_leaves);
    full_path.into_iter().skip(1).collect()
}

/// Calculate the copath nodes for a given node
///
/// The copath consists of the siblings of nodes on the direct path.
pub fn copath(leaf_index: LeafIndex, num_leaves: usize) -> Vec<NodeIndex> {
    let path = direct_path(leaf_index, num_leaves);
    path.iter().map(|node| node.sibling()).collect()
}

/// Find the next available leaf index in a left-balanced tree
///
/// Returns the index where the next leaf should be inserted to maintain LBBT property.
pub fn next_leaf_index(num_leaves: usize) -> LeafIndex {
    LeafIndex(num_leaves)
}

/// Calculate the level (height) of a node in the tree
///
/// Leaves are at level 0, their parents at level 1, etc.
pub fn node_level(node: NodeIndex, num_leaves: usize) -> usize {
    if node.is_leaf() {
        return 0;
    }

    // Count how many times we can go up to the root
    let mut level = 0;
    let mut current = node;
    let root = root_index(num_leaves);

    while current != root {
        if let Some(parent) = current.parent(num_leaves) {
            current = parent;
            level += 1;
        } else {
            break;
        }
    }

    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_to_node_index() {
        assert_eq!(LeafIndex(0).to_node_index(), NodeIndex(0));
        assert_eq!(LeafIndex(1).to_node_index(), NodeIndex(2));
        assert_eq!(LeafIndex(2).to_node_index(), NodeIndex(4));
        assert_eq!(LeafIndex(3).to_node_index(), NodeIndex(6));
    }

    #[test]
    fn test_node_index_is_leaf() {
        assert!(NodeIndex(0).is_leaf());
        assert!(NodeIndex(2).is_leaf());
        assert!(NodeIndex(4).is_leaf());
        assert!(!NodeIndex(1).is_leaf());
        assert!(!NodeIndex(3).is_leaf());
    }

    #[test]
    fn test_node_index_is_branch() {
        assert!(!NodeIndex(0).is_branch());
        assert!(NodeIndex(1).is_branch());
        assert!(!NodeIndex(2).is_branch());
        assert!(NodeIndex(3).is_branch());
    }

    #[test]
    fn test_root_index() {
        assert_eq!(root_index(1), NodeIndex(0)); // Single leaf
        assert_eq!(root_index(2), NodeIndex(3)); // Two leaves
        assert_eq!(root_index(3), NodeIndex(5)); // Three leaves
        assert_eq!(root_index(4), NodeIndex(7)); // Four leaves
    }

    #[test]
    fn test_path_to_root_single_leaf() {
        let path = path_to_root(LeafIndex(0), 1);
        assert_eq!(path, vec![NodeIndex(0)]);
    }

    #[test]
    fn test_path_to_root_two_leaves() {
        // Tree with 2 leaves: L0(0) <- B(3) -> L1(2)
        let path = path_to_root(LeafIndex(0), 2);
        assert_eq!(path, vec![NodeIndex(0), NodeIndex(3)]);

        let path = path_to_root(LeafIndex(1), 2);
        assert_eq!(path, vec![NodeIndex(2), NodeIndex(3)]);
    }

    #[test]
    fn test_direct_path() {
        // Direct path excludes the leaf itself
        let path = direct_path(LeafIndex(0), 2);
        assert_eq!(path, vec![NodeIndex(3)]);

        let path = direct_path(LeafIndex(0), 1);
        assert_eq!(path, vec![]); // Single leaf has no ancestors
    }

    #[test]
    fn test_sibling() {
        assert_eq!(NodeIndex(0).sibling(), NodeIndex(1));
        assert_eq!(NodeIndex(1).sibling(), NodeIndex(0));
        assert_eq!(NodeIndex(2).sibling(), NodeIndex(3));
        assert_eq!(NodeIndex(3).sibling(), NodeIndex(2));
    }

    #[test]
    fn test_next_leaf_index() {
        assert_eq!(next_leaf_index(0), LeafIndex(0));
        assert_eq!(next_leaf_index(1), LeafIndex(1));
        assert_eq!(next_leaf_index(5), LeafIndex(5));
    }

    #[test]
    fn test_copath() {
        // For a tree with 2 leaves
        let copath0 = copath(LeafIndex(0), 2);
        assert_eq!(copath0, vec![NodeIndex(2)]); // Sibling of root's child

        let copath1 = copath(LeafIndex(1), 2);
        assert_eq!(copath1, vec![NodeIndex(0)]);
    }

    #[test]
    #[should_panic(expected = "Cannot calculate root index for empty tree")]
    fn test_root_index_empty() {
        root_index(0);
    }
}
