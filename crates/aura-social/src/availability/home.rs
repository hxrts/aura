//! Home-Level Data Availability
//!
//! Implements data availability for blocks. All residents replicate all
//! home-level shared data.

use crate::home::Home;
use crate::storage::StorageService;
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
use crate::facts::HomeId;
use std::sync::Arc;

/// Home-level data availability.
///
/// Implements `DataAvailability` for home-scoped data. All residents
/// replicate all home data, providing redundancy equal to home size.
///
/// # Type Parameters
///
/// * `S` - Storage effects for local data access
/// * `N` - Network effects for peer communication
///
/// # Example
///
/// ```ignore
/// let home_da = HomeAvailability::new(home_instance, local_authority, storage, network);
///
/// // Store data to the home
/// let hash = home_da.store(home_id, &content).await?;
///
/// // Retrieve data (tries local, then peers)
/// let data = home_da.retrieve(home_id, &hash).await?;
/// ```
pub struct HomeAvailability<S, N> {
    /// The home this availability service is for.
    home_instance: Home,
    /// Our local authority ID.
    local_authority: AuthorityId,
    /// Storage effects for local data access.
    storage: Arc<S>,
    /// Network effects for peer communication.
    network: Arc<N>,
}

impl<S, N> HomeAvailability<S, N>
where
    S: StorageEffects,
    N: NetworkEffects,
{
    /// Create a new home availability service.
    pub fn new(
        home_instance: Home,
        local_authority: AuthorityId,
        storage: Arc<S>,
        network: Arc<N>,
    ) -> Self {
        Self {
            home_instance,
            local_authority,
            storage,
            network,
        }
    }

    /// Get the home this service is for.
    pub fn home(&self) -> &Home {
        &self.home_instance
    }

    /// Check if we are a resident of this home.
    pub fn is_resident(&self) -> bool {
        self.home_instance.is_resident(&self.local_authority)
    }

    /// Get replication peers (other residents).
    fn replication_peers_internal(&self) -> Vec<AuthorityId> {
        self.home_instance.home_peers(&self.local_authority)
    }

    /// Check storage capacity for a store operation.
    fn check_capacity(&self, size: u64) -> Result<(), AvailabilityError> {
        let budget = &self.home_instance.storage_budget;
        if !StorageService::can_pin(budget, size) {
            let used = budget.resident_storage_spent
                + budget.neighborhood_donations
                + budget.pinned_storage_spent;
            return Err(AvailabilityError::CapacityExceeded {
                used,
                limit: self.home_instance.storage_limit,
                requested: size,
            });
        }
        Ok(())
    }

    /// Convert hash to storage key.
    fn hash_to_key(hash: &Hash32) -> String {
        format!("content:{hash}")
    }
}

#[async_trait]
impl<S, N> DataAvailability for HomeAvailability<S, N>
where
    S: StorageEffects + Send + Sync,
    N: NetworkEffects + Send + Sync,
{
    type UnitId = HomeId;

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

        // Try peers in deterministic order (sorted by authority ID)
        let mut peers = self.replication_peers_internal();
        peers.sort_by_key(|p| p.uuid());

        let mut peers_tried = 0;
        for peer in peers {
            peers_tried += 1;

            // Request from peer
            let request = RetrieveRequest { hash: *hash };
            let serialized = aura_core::util::serialization::to_vec(&request)
                .map_err(|e| AvailabilityError::NetworkError(e.to_string()))?;

            match self.network.send_to_peer(peer.uuid(), serialized).await {
                Ok(()) => {
                    // Wait for response (simplified - real impl would use request/response)
                    match self.network.receive_from(peer.uuid()).await {
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
                    }
                }
                Err(_) => continue,
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
        // Check capacity
        self.check_capacity(content.len() as u64)?;

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
    use crate::facts::{HomeFact, ResidentFact, StewardFact};
    use std::collections::HashMap;

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    fn test_home() -> Home {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let steward = AuthorityId::new_from_entropy([1u8; 32]);

        let home_fact = HomeFact::new(home_id, test_timestamp());

        let residents = vec![
            ResidentFact::new(steward, home_id, test_timestamp()),
            ResidentFact::new(
                AuthorityId::new_from_entropy([2u8; 32]),
                home_id,
                test_timestamp(),
            ),
            ResidentFact::new(
                AuthorityId::new_from_entropy([3u8; 32]),
                home_id,
                test_timestamp(),
            ),
        ];

        let stewards = vec![StewardFact::new(steward, home_id, test_timestamp())];

        Home::from_facts(&home_fact, None, &residents, &stewards)
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
        async fn store_batch(&self, _pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
            Ok(())
        }
        async fn retrieve_batch(
            &self,
            _keys: &[String],
        ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
            Ok(HashMap::new())
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
        let home_instance = test_home();
        let local = AuthorityId::new_from_entropy([1u8; 32]);

        let da =
            HomeAvailability::new(home_instance, local, Arc::new(DummyStorage), Arc::new(DummyNetwork));

        let peers = da.replication_peers(HomeId::from_bytes([1u8; 32]));
        assert_eq!(peers.len(), 2); // 3 residents - 1 self = 2 peers
        assert!(!peers.contains(&local));
    }

    #[test]
    fn test_is_resident() {
        let home_instance = test_home();
        let resident = AuthorityId::new_from_entropy([1u8; 32]);
        let non_resident = AuthorityId::new_from_entropy([99u8; 32]);

        let da_resident = HomeAvailability::new(
            home_instance.clone(),
            resident,
            Arc::new(DummyStorage),
            Arc::new(DummyNetwork),
        );
        assert!(da_resident.is_resident());

        let da_non_resident = HomeAvailability::new(
            home_instance,
            non_resident,
            Arc::new(DummyStorage),
            Arc::new(DummyNetwork),
        );
        assert!(!da_non_resident.is_resident());
    }
}
