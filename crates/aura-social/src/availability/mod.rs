//! Data Availability Module
//!
//! This module implements the `DataAvailability` trait for blocks and neighborhoods.
//! Data availability is unit-scoped: homes replicate home data, neighborhoods
//! replicate neighborhood data.
//!
//! # Design
//!
//! **Home availability**: All members of a home replicate all home-level
//! shared data. When retrieving data, we try local storage first, then query
//! peers in deterministic order.
//!
//! **Neighborhood availability**: Each home in a neighborhood maintains a
//! representative. Neighborhood-level data is replicated across all member
//! blocks via their representatives.
//!
//! # Example
//!
//! ```ignore
//! use aura_social::availability::{HomeAvailability, NeighborhoodAvailability};
//!
//! // Create home availability for a home
//! let home_da = HomeAvailability::new(home_instance, local_authority, storage, network);
//!
//! // Retrieve data from the home
//! let data = home_da.retrieve(home_id, &content_hash).await?;
//! ```

mod home;
mod neighborhood;

use aura_core::{
    domain::content::Hash32,
    effects::{availability::AvailabilityError, storage::StorageEffects},
};

pub use home::HomeAvailability;
pub use neighborhood::NeighborhoodAvailability;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct RetrieveRequest {
    pub(super) hash: Hash32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct RetrieveResponse {
    pub(super) content: Option<Vec<u8>>,
}

pub(super) fn storage_key(scope: &str, hash: &Hash32) -> String {
    format!("{scope}:{hash}")
}

pub(super) async fn is_stored_locally<S>(storage: &S, key: &str) -> bool
where
    S: StorageEffects + ?Sized,
{
    storage.exists(key).await.unwrap_or(false)
}

pub(super) async fn retrieve_stored_locally<S>(storage: &S, key: &str) -> Option<Vec<u8>>
where
    S: StorageEffects + ?Sized,
{
    storage.retrieve(key).await.ok().flatten()
}

pub(super) fn serialize_retrieve_request(hash: &Hash32) -> Result<Vec<u8>, AvailabilityError> {
    aura_core::util::serialization::to_vec(&RetrieveRequest { hash: *hash })
        .map_err(|error| AvailabilityError::NetworkError(error.to_string()))
}

pub(super) fn verified_response_content(hash: &Hash32, response: &[u8]) -> Option<Vec<u8>> {
    let response = aura_core::util::serialization::from_slice::<RetrieveResponse>(response).ok()?;
    let content = response.content?;
    (Hash32::from_bytes(&content) == *hash).then_some(content)
}
