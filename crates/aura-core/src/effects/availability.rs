//! Layer 1: Data Availability Effect Trait Definitions
//!
//! This module defines the pure effect trait interface for data availability
//! within replication units (homes, neighborhoods). Data availability is
//! scoped to the unit that owns the data:
//! - Blocks provide DA for home-level shared data
//! - Neighborhoods provide DA for neighborhood-level shared data
//!
//! **Effect Classification**: Application Effect
//! - Implemented by protocol crates (aura-protocol provides home/neighborhood implementations)
//! - Used by feature crates (aura-social, aura-chat) for content retrieval
//! - Core trait definition belongs in Layer 1 (foundation)
//!
//! # Design Principles
//!
//! **Full replication within unit**: All members of a unit replicate all data.
//! No partial replication or erasure coding in v1.
//!
//! **Availability = reachability**: If you can reach any member, you can
//! retrieve any data. No threshold attestation required.
//!
//! **Consent follows structure**: Unit membership implies consent to
//! replication duties.

use crate::{domain::content::Hash32, types::identifiers::AuthorityId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::hash::Hash;

/// Error type for data availability operations.
///
/// These errors represent failures in the data availability layer,
/// from local storage issues to network retrieval failures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AvailabilityError {
    /// Content not found in the specified unit.
    ///
    /// The content hash was not found locally or from any reachable peer.
    NotFound {
        /// The hash of the content that wasn't found
        hash: Hash32,
    },

    /// Unit storage capacity exceeded.
    ///
    /// The store operation would exceed the unit's storage limit.
    CapacityExceeded {
        /// Currently used storage in bytes
        used: u64,
        /// Maximum storage limit in bytes
        limit: u64,
        /// Size of content that was rejected
        requested: u64,
    },

    /// No peers reachable for retrieval.
    ///
    /// Content wasn't available locally and no replication peers
    /// could be contacted.
    NoReachablePeers {
        /// Number of peers that were tried
        peers_tried: u32,
    },

    /// Network error during peer retrieval.
    ///
    /// Communication with a peer failed during content retrieval.
    NetworkError(String),

    /// Unit not found or invalid.
    ///
    /// The specified unit ID doesn't exist or isn't accessible.
    InvalidUnit(String),

    /// Storage backend error.
    ///
    /// The local storage system returned an error.
    StorageError(String),
}

impl std::fmt::Display for AvailabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { hash } => {
                write!(f, "content not found: {hash}")
            }
            Self::CapacityExceeded {
                used,
                limit,
                requested,
            } => {
                write!(
                    f,
                    "capacity exceeded: {used}/{limit} bytes used, {requested} bytes requested"
                )
            }
            Self::NoReachablePeers { peers_tried } => {
                write!(f, "no reachable peers (tried {peers_tried})")
            }
            Self::NetworkError(msg) => write!(f, "network error: {msg}"),
            Self::InvalidUnit(msg) => write!(f, "invalid unit: {msg}"),
            Self::StorageError(msg) => write!(f, "storage error: {msg}"),
        }
    }
}

impl std::error::Error for AvailabilityError {}

/// Effect trait for data availability within a replication unit.
///
/// This trait defines the interface for storing and retrieving content
/// from a fully-replicated unit (home or neighborhood). All members
/// of the unit maintain complete copies of the data.
///
/// # Type Parameters
///
/// The associated `UnitId` type identifies the replication unit. For blocks,
/// this is `HomeId`. For neighborhoods, this is `NeighborhoodId`.
///
/// # Implementation Notes
///
/// Implementations should:
/// - Try local storage first for retrieval
/// - Fall back to peers in deterministic order (for reproducibility)
/// - Cache retrieved content locally
/// - Enforce storage capacity limits on store operations
/// - Replication happens via journal sync, not explicit push
///
/// # Example
///
/// ```ignore
/// // Home availability implementation
/// impl DataAvailability for HomeAvailability {
///     type UnitId = HomeId;
///
///     fn replication_peers(&self, _unit: HomeId) -> Vec<AuthorityId> {
///         self.residents.iter()
///             .filter(|a| *a != &self.local_authority)
///             .copied()
///             .collect()
///     }
///     // ...
/// }
/// ```
#[async_trait]
pub trait DataAvailability: Send + Sync {
    /// The identifier type for replication units.
    ///
    /// For homes: `HomeId`
    /// For neighborhoods: `NeighborhoodId`
    type UnitId: Copy + Eq + Hash + Send + Sync;

    /// List peers who replicate this unit's data.
    ///
    /// Returns all authorities that maintain copies of the unit's data,
    /// excluding the local authority.
    ///
    /// # Arguments
    /// * `unit` - The unit to query peers for
    ///
    /// # Returns
    /// A list of authority IDs that replicate this unit
    fn replication_peers(&self, unit: Self::UnitId) -> Vec<AuthorityId>;

    /// Check if content is available in local storage.
    ///
    /// This is a fast check that doesn't contact peers.
    ///
    /// # Arguments
    /// * `unit` - The unit the content belongs to
    /// * `hash` - The content hash to check
    ///
    /// # Returns
    /// `true` if the content is available locally
    async fn is_locally_available(&self, unit: Self::UnitId, hash: &Hash32) -> bool;

    /// Retrieve content from local storage only.
    ///
    /// Does not contact peers. Returns `None` if not available locally.
    ///
    /// # Arguments
    /// * `unit` - The unit the content belongs to
    /// * `hash` - The content hash to retrieve
    ///
    /// # Returns
    /// The content bytes if available locally, `None` otherwise
    async fn retrieve_local(&self, unit: Self::UnitId, hash: &Hash32) -> Option<Vec<u8>>;

    /// Retrieve content from the unit (local or peers).
    ///
    /// Tries local storage first, then contacts peers in deterministic
    /// order until the content is found. Caches retrieved content locally.
    ///
    /// # Arguments
    /// * `unit` - The unit the content belongs to
    /// * `hash` - The content hash to retrieve
    ///
    /// # Returns
    /// The content bytes
    ///
    /// # Errors
    /// - `NotFound` if content isn't available from any source
    /// - `NoReachablePeers` if local retrieval failed and no peers responded
    /// - `NetworkError` if peer communication failed
    async fn retrieve(
        &self,
        unit: Self::UnitId,
        hash: &Hash32,
    ) -> Result<Vec<u8>, AvailabilityError>;

    /// Store content in the unit.
    ///
    /// Stores content locally. Replication to peers happens via
    /// journal synchronization, not explicit push.
    ///
    /// # Arguments
    /// * `unit` - The unit to store content in
    /// * `content` - The content bytes to store
    ///
    /// # Returns
    /// The hash of the stored content
    ///
    /// # Errors
    /// - `CapacityExceeded` if the unit's storage limit would be exceeded
    /// - `StorageError` if local storage failed
    async fn store(&self, unit: Self::UnitId, content: &[u8]) -> Result<Hash32, AvailabilityError>;
}

/// Blanket implementation for Arc<T> where T: DataAvailability
#[async_trait]
impl<T: DataAvailability + ?Sized> DataAvailability for std::sync::Arc<T> {
    type UnitId = T::UnitId;

    fn replication_peers(&self, unit: Self::UnitId) -> Vec<AuthorityId> {
        (**self).replication_peers(unit)
    }

    async fn is_locally_available(&self, unit: Self::UnitId, hash: &Hash32) -> bool {
        (**self).is_locally_available(unit, hash).await
    }

    async fn retrieve_local(&self, unit: Self::UnitId, hash: &Hash32) -> Option<Vec<u8>> {
        (**self).retrieve_local(unit, hash).await
    }

    async fn retrieve(
        &self,
        unit: Self::UnitId,
        hash: &Hash32,
    ) -> Result<Vec<u8>, AvailabilityError> {
        (**self).retrieve(unit, hash).await
    }

    async fn store(&self, unit: Self::UnitId, content: &[u8]) -> Result<Hash32, AvailabilityError> {
        (**self).store(unit, content).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability_error_display() {
        let hash = Hash32::from([0u8; 32]);

        let not_found = AvailabilityError::NotFound { hash };
        assert!(not_found.to_string().contains("not found"));

        let capacity = AvailabilityError::CapacityExceeded {
            used: 1000,
            limit: 1000,
            requested: 100,
        };
        assert!(capacity.to_string().contains("capacity exceeded"));

        let no_peers = AvailabilityError::NoReachablePeers { peers_tried: 5 };
        assert!(no_peers.to_string().contains("no reachable peers"));

        let network = AvailabilityError::NetworkError("timeout".to_string());
        assert!(network.to_string().contains("network error"));
    }

    #[test]
    fn test_availability_error_equality() {
        let hash = Hash32::from([1u8; 32]);
        let e1 = AvailabilityError::NotFound { hash };
        let e2 = AvailabilityError::NotFound { hash };
        assert_eq!(e1, e2);

        let e3 = AvailabilityError::NoReachablePeers { peers_tried: 3 };
        assert_ne!(e1, e3);
    }
}
