//! Social error types
//!
//! Errors specific to social topology operations.

use crate::facts::{HomeId, NeighborhoodId};
use thiserror::Error;

/// Errors from social topology operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SocialError {
    /// Home has reached maximum resident capacity.
    #[error("home {home_id} is full (max {max} residents)")]
    HomeFull {
        /// The home that is full
        home_id: HomeId,
        /// Maximum residents allowed
        max: u8,
    },

    /// Home has reached maximum neighborhood membership.
    #[error("home {home_id} has reached neighborhood limit ({max} neighborhoods)")]
    NeighborhoodLimitReached {
        /// The home that hit the limit
        home_id: HomeId,
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

    /// Authority is not a resident of the home.
    #[error("authority is not a resident of home {home_id}")]
    NotResident {
        /// The home in question
        home_id: HomeId,
    },

    /// Authority is not a steward of the home.
    #[error("authority is not a steward of home {home_id}")]
    NotSteward {
        /// The home in question
        home_id: HomeId,
    },

    /// Authority is already a member.
    #[error("authority is already a resident of home {home_id}")]
    AlreadyResident {
        /// The home in question
        home_id: HomeId,
    },

    /// Home is already a member of the neighborhood.
    #[error("home {home_id} is already a member of neighborhood {neighborhood_id}")]
    AlreadyMember {
        /// The home in question
        home_id: HomeId,
        /// The neighborhood in question
        neighborhood_id: NeighborhoodId,
    },

    /// Home not found.
    #[error("home {0} not found")]
    HomeNotFound(HomeId),

    /// Neighborhood not found.
    #[error("neighborhood {0} not found")]
    NeighborhoodNotFound(NeighborhoodId),

    /// Homes are not adjacent.
    #[error("homes {0} and {1} are not adjacent")]
    NotAdjacent(HomeId, HomeId),

    /// Missing capability for operation.
    #[error("missing capability: {0}")]
    MissingCapability(String),
}

impl SocialError {
    /// Create a home full error.
    pub fn home_full(home_id: HomeId, max: u8) -> Self {
        Self::HomeFull { home_id, max }
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
    pub fn not_resident(home_id: HomeId) -> Self {
        Self::NotResident { home_id }
    }

    /// Create a not steward error.
    pub fn not_steward(home_id: HomeId) -> Self {
        Self::NotSteward { home_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let home_id = HomeId::from_bytes([1u8; 32]);

        let err = SocialError::home_full(home_id, 8);
        assert!(err.to_string().contains("full"));

        let err = SocialError::storage_exceeded(1000, 2000);
        assert!(err.to_string().contains("1000"));
        assert!(err.to_string().contains("2000"));

        let err = SocialError::traversal_denied("no capability");
        assert!(err.to_string().contains("no capability"));
    }
}
