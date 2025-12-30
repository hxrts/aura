//! Neighborhood-Level Data Availability
//!
//! Implements data availability for neighborhoods. Each member home provides
//! a representative, and neighborhood-level data is replicated across all
//! representatives.

use crate::neighborhood::Neighborhood;
use async_trait::async_trait;
use aura_core::{
    domain::content::Hash32,
    effects::{
        availability::{AvailabilityError, DataAvailability},
        network::{NetworkCoreEffects, NetworkEffects},
        storage::{StorageCoreEffects, StorageEffects},
    },
    identifiers::AuthorityId,
};
use crate::facts::{HomeId, NeighborhoodId};
use std::collections::HashMap;
use std::sync::Arc;

/// Neighborhood-level data availability.
///
/// Implements `DataAvailability` for neighborhood-scoped data. Each member
/// home designates a representative who replicates neighborhood data.
///
/// # Type Parameters
///
/// * `S` - Storage effects for local data access
/// * `N` - Network effects for peer communication
///
/// # Example
///
/// ```ignore
/// let neighborhood_da = NeighborhoodAvailability::new(
///     neighborhood,
///     representatives,
///     local_authority,
///     local_home,
///     storage,
///     network,
/// );
///
/// // Store data to the neighborhood
/// let hash = neighborhood_da.store(neighborhood_id, &content).await?;
/// ```
pub struct NeighborhoodAvailability<S, N> {
    /// The neighborhood this availability service is for.
    neighborhood: Neighborhood,
    /// Representatives for each member home.
    ///
    /// Maps HomeId -> AuthorityId of the representative.
    representatives: HashMap<HomeId, AuthorityId>,
    /// Our local authority ID.
    local_authority: AuthorityId,
    /// Our local home ID (if we're in a member home).
    local_home: Option<HomeId>,
    /// Storage effects for local data access.
    storage: Arc<S>,
    /// Network effects for peer communication.
    network: Arc<N>,
}

impl<S, N> NeighborhoodAvailability<S, N>
where
    S: StorageEffects,
    N: NetworkEffects,
{
    /// Create a new neighborhood availability service.
    pub fn new(
        neighborhood: Neighborhood,
        representatives: HashMap<HomeId, AuthorityId>,
        local_authority: AuthorityId,
        local_home: Option<HomeId>,
        storage: Arc<S>,
        network: Arc<N>,
    ) -> Self {
        Self {
            neighborhood,
            representatives,
            local_authority,
            local_home,
            storage,
            network,
        }
    }

    /// Get the neighborhood this service is for.
    pub fn neighborhood(&self) -> &Neighborhood {
        &self.neighborhood
    }

    /// Check if we are a representative for our home.
    pub fn is_representative(&self) -> bool {
        if let Some(local_home) = self.local_home {
            self.representatives
                .get(&local_home)
                .map(|r| *r == self.local_authority)
                .unwrap_or(false)
        } else {
            false
        }
    }

    /// Get all representatives except self.
    fn replication_peers_internal(&self) -> Vec<AuthorityId> {
        self.representatives
            .values()
            .filter(|r| **r != self.local_authority)
            .copied()
            .collect()
    }

    /// Get the representative for a specific home.
    pub fn representative_for(&self, home_id: &HomeId) -> Option<AuthorityId> {
        self.representatives.get(home_id).copied()
    }

    /// Get all member blocks.
    pub fn member_homes(&self) -> Vec<HomeId> {
        self.neighborhood.member_homes.clone()
    }

    /// Convert hash to storage key.
    fn hash_to_key(hash: &Hash32) -> String {
        format!("neighborhood:{hash}")
    }
}

#[async_trait]
impl<S, N> DataAvailability for NeighborhoodAvailability<S, N>
where
    S: StorageEffects + Send + Sync,
    N: NetworkEffects + Send + Sync,
{
    type UnitId = NeighborhoodId;

    fn replication_peers(&self, _unit: Self::UnitId) -> Vec<AuthorityId> {
        self.replication_peers_internal()
    }

    async fn is_locally_available(&self, _unit: Self::UnitId, hash: &Hash32) -> bool {
        let key = Self::hash_to_key(hash);
        self.storage.exists(&key).await.unwrap_or(false)
    }

    async fn retrieve_local(&self, _unit: Self::UnitId, hash: &Hash32) -> Option<Vec<u8>> {
        let key = Self::hash_to_key(hash);
        self.storage.retrieve(&key).await.ok().flatten()
    }

    async fn retrieve(
        &self,
        unit: Self::UnitId,
        hash: &Hash32,
    ) -> Result<Vec<u8>, AvailabilityError> {
        // Try local first
        if let Some(data) = self.retrieve_local(unit, hash).await {
            return Ok(data);
        }

        // Try representatives in deterministic order (sorted by home ID)
        let mut blocks: Vec<_> = self.representatives.keys().copied().collect();
        blocks.sort();

        let mut peers_tried = 0;
        for home_id in blocks {
            if Some(home_id) == self.local_home {
                continue; // Skip our own home
            }

            if let Some(rep) = self.representatives.get(&home_id) {
                peers_tried += 1;

                // Request from representative
                let request = RetrieveRequest { hash: *hash };
                let serialized = aura_core::util::serialization::to_vec(&request)
                    .map_err(|e| AvailabilityError::NetworkError(e.to_string()))?;

                match self.network.send_to_peer(rep.uuid(), serialized).await {
                    Ok(()) => match self.network.receive_from(rep.uuid()).await {
                        Ok(response) => {
                            if let Ok(data) = aura_core::util::serialization::from_slice::<
                                RetrieveResponse,
                            >(&response)
                            {
                                if let Some(content) = data.content {
                                    // Verify hash
                                    let computed = Hash32::from_bytes(&content);
                                    if computed == *hash {
                                        return Ok(content);
                                    }
                                }
                            }
                        }
                        Err(_) => continue,
                    },
                    Err(_) => continue,
                }
            }
        }

        if peers_tried > 0 {
            Err(AvailabilityError::NoReachablePeers {
                peers_tried: peers_tried as u32,
            })
        } else {
            Err(AvailabilityError::NotFound { hash: *hash })
        }
    }

    async fn store(
        &self,
        _unit: Self::UnitId,
        content: &[u8],
    ) -> Result<Hash32, AvailabilityError> {
        // Only representatives can store neighborhood data
        if !self.is_representative() {
            return Err(AvailabilityError::NetworkError(
                "not a neighborhood representative".to_string(),
            ));
        }

        // Compute hash
        let hash = Hash32::from_bytes(content);
        let key = Self::hash_to_key(&hash);

        // Store locally
        self.storage
            .store(&key, content.to_vec())
            .await
            .map_err(|e| AvailabilityError::StorageError(e.to_string()))?;

        Ok(hash)
    }
}

/// Request message for data retrieval.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RetrieveRequest {
    hash: Hash32,
}

/// Response message for data retrieval.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RetrieveResponse {
    content: Option<Vec<u8>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::{
        network::NetworkError, storage::StorageError, NetworkCoreEffects, NetworkExtendedEffects,
        StorageCoreEffects, StorageExtendedEffects,
    };
    use aura_core::time::{PhysicalTime, TimeStamp};
    use crate::facts::{HomeMemberFact, NeighborhoodFact};

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    fn test_neighborhood() -> (Neighborhood, Vec<HomeId>) {
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);
        let block1 = HomeId::from_bytes([1u8; 32]);
        let block2 = HomeId::from_bytes([2u8; 32]);
        let block3 = HomeId::from_bytes([3u8; 32]);

        let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, test_timestamp());

        let members = vec![
            HomeMemberFact::new(block1, neighborhood_id, test_timestamp()),
            HomeMemberFact::new(block2, neighborhood_id, test_timestamp()),
            HomeMemberFact::new(block3, neighborhood_id, test_timestamp()),
        ];

        let neighborhood = Neighborhood::from_facts(&neighborhood_fact, &members, &[]);
        (neighborhood, vec![block1, block2, block3])
    }

    struct DummyStorage;
    struct DummyNetwork;

    #[async_trait]
    impl StorageCoreEffects for DummyStorage {
        async fn store(&self, _key: &str, _value: Vec<u8>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn retrieve(&self, _key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            Ok(None)
        }
        async fn remove(&self, _key: &str) -> Result<bool, StorageError> {
            Ok(false)
        }
        async fn list_keys(&self, _prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl StorageExtendedEffects for DummyStorage {
        async fn exists(&self, _key: &str) -> Result<bool, StorageError> {
            Ok(false)
        }
        async fn store_batch(
            &self,
            _pairs: std::collections::HashMap<String, Vec<u8>>,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        async fn retrieve_batch(
            &self,
            _keys: &[String],
        ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
            Ok(std::collections::HashMap::new())
        }
        async fn clear_all(&self) -> Result<(), StorageError> {
            Ok(())
        }
        async fn stats(&self) -> Result<aura_core::effects::storage::StorageStats, StorageError> {
            Ok(aura_core::effects::storage::StorageStats::default())
        }
    }

    #[async_trait]
    impl NetworkCoreEffects for DummyNetwork {
        async fn send_to_peer(
            &self,
            _peer_id: uuid::Uuid,
            _message: Vec<u8>,
        ) -> Result<(), NetworkError> {
            Ok(())
        }
        async fn broadcast(&self, _message: Vec<u8>) -> Result<(), NetworkError> {
            Ok(())
        }
        async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), NetworkError> {
            Err(NetworkError::NoMessage)
        }
    }

    #[async_trait]
    impl NetworkExtendedEffects for DummyNetwork {
        async fn receive_from(&self, _peer_id: uuid::Uuid) -> Result<Vec<u8>, NetworkError> {
            Err(NetworkError::ReceiveFailed {
                reason: "not connected".to_string(),
            })
        }
        async fn connected_peers(&self) -> Vec<uuid::Uuid> {
            vec![]
        }
        async fn is_peer_connected(&self, _peer_id: uuid::Uuid) -> bool {
            false
        }
        async fn subscribe_to_peer_events(
            &self,
        ) -> Result<aura_core::effects::network::PeerEventStream, NetworkError> {
            Err(NetworkError::NotImplemented)
        }
        async fn open(&self, _address: &str) -> Result<String, NetworkError> {
            Err(NetworkError::NotImplemented)
        }
        async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
            Err(NetworkError::NotImplemented)
        }
        async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
            Err(NetworkError::NotImplemented)
        }
    }

    #[test]
    fn test_replication_peers_excludes_self() {
        let (neighborhood, blocks) = test_neighborhood();

        let rep1 = AuthorityId::new_from_entropy([1u8; 32]);
        let rep2 = AuthorityId::new_from_entropy([2u8; 32]);
        let rep3 = AuthorityId::new_from_entropy([3u8; 32]);

        let mut representatives = HashMap::new();
        representatives.insert(blocks[0], rep1);
        representatives.insert(blocks[1], rep2);
        representatives.insert(blocks[2], rep3);

        let da = NeighborhoodAvailability::new(
            neighborhood,
            representatives,
            rep1, // We are rep1
            Some(blocks[0]),
            Arc::new(DummyStorage),
            Arc::new(DummyNetwork),
        );

        let peers = da.replication_peers(NeighborhoodId::from_bytes([1u8; 32]));
        assert_eq!(peers.len(), 2); // 3 representatives - 1 self = 2 peers
        assert!(!peers.contains(&rep1));
        assert!(peers.contains(&rep2));
        assert!(peers.contains(&rep3));
    }

    #[test]
    fn test_is_representative() {
        let (neighborhood, blocks) = test_neighborhood();

        let rep1 = AuthorityId::new_from_entropy([1u8; 32]);
        let non_rep = AuthorityId::new_from_entropy([99u8; 32]);

        let mut representatives = HashMap::new();
        representatives.insert(blocks[0], rep1);

        // rep1 is a representative for block0
        let da_rep = NeighborhoodAvailability::new(
            neighborhood.clone(),
            representatives.clone(),
            rep1,
            Some(blocks[0]),
            Arc::new(DummyStorage),
            Arc::new(DummyNetwork),
        );
        assert!(da_rep.is_representative());

        // non_rep is not a representative
        let da_non_rep = NeighborhoodAvailability::new(
            neighborhood,
            representatives,
            non_rep,
            Some(blocks[0]),
            Arc::new(DummyStorage),
            Arc::new(DummyNetwork),
        );
        assert!(!da_non_rep.is_representative());
    }
}
