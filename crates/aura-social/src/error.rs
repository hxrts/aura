//! Social error types
//!
//! Errors specific to social topology operations.

use aura_journal::facts::social::{BlockId, NeighborhoodId};
use thiserror::Error;

/// Errors from social topology operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SocialError {
    /// Block has reached maximum resident capacity.
    #[error("block {block_id} is full (max {max} residents)")]
    BlockFull {
        /// The block that is full
        block_id: BlockId,
        /// Maximum residents allowed
        max: u8,
    },

    /// Block has reached maximum neighborhood membership.
    #[error("block {block_id} has reached neighborhood limit ({max} neighborhoods)")]
    NeighborhoodLimitReached {
        /// The block that hit the limit
        block_id: BlockId,
        /// Maximum neighborhoods allowed
        max: u8,
    },

    /// Storage capacity exceeded.
    #[error("storage exceeded: {available} bytes available, {requested} bytes requested")]
    StorageExceeded {
        /// Available storage in bytes
        available: u64,
        /// Requested storage in bytes
        requested: u64,
    },

    /// Traversal denied.
    #[error("traversal denied: {reason}")]
    TraversalDenied {
        /// Reason for denial
        reason: String,
    },

    /// Authority is not a resident of the block.
    #[error("authority is not a resident of block {block_id}")]
    NotResident {
        /// The block in question
        block_id: BlockId,
    },

    /// Authority is not a steward of the block.
    #[error("authority is not a steward of block {block_id}")]
    NotSteward {
        /// The block in question
        block_id: BlockId,
    },

    /// Authority is already a member.
    #[error("authority is already a resident of block {block_id}")]
    AlreadyResident {
        /// The block in question
        block_id: BlockId,
    },

    /// Block is already a member of the neighborhood.
    #[error("block {block_id} is already a member of neighborhood {neighborhood_id}")]
    AlreadyMember {
        /// The block in question
        block_id: BlockId,
        /// The neighborhood in question
        neighborhood_id: NeighborhoodId,
    },

    /// Block not found.
    #[error("block {0} not found")]
    BlockNotFound(BlockId),

    /// Neighborhood not found.
    #[error("neighborhood {0} not found")]
    NeighborhoodNotFound(NeighborhoodId),

    /// Blocks are not adjacent.
    #[error("blocks {0} and {1} are not adjacent")]
    NotAdjacent(BlockId, BlockId),

    /// Missing capability for operation.
    #[error("missing capability: {0}")]
    MissingCapability(String),
}

impl SocialError {
    /// Create a block full error.
    pub fn block_full(block_id: BlockId, max: u8) -> Self {
        Self::BlockFull { block_id, max }
    }

    /// Create a storage exceeded error.
    pub fn storage_exceeded(available: u64, requested: u64) -> Self {
        Self::StorageExceeded {
            available,
            requested,
        }
    }

    /// Create a traversal denied error.
    pub fn traversal_denied(reason: impl Into<String>) -> Self {
        Self::TraversalDenied {
            reason: reason.into(),
        }
    }

    /// Create a not resident error.
    pub fn not_resident(block_id: BlockId) -> Self {
        Self::NotResident { block_id }
    }

    /// Create a not steward error.
    pub fn not_steward(block_id: BlockId) -> Self {
        Self::NotSteward { block_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let block_id = BlockId::from_bytes([1u8; 32]);

        let err = SocialError::block_full(block_id, 8);
        assert!(err.to_string().contains("full"));

        let err = SocialError::storage_exceeded(1000, 2000);
        assert!(err.to_string().contains("1000"));
        assert!(err.to_string().contains("2000"));

        let err = SocialError::traversal_denied("no capability");
        assert!(err.to_string().contains("no capability"));
    }
}
