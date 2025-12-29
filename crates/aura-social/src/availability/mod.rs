//! Data Availability Module
//!
//! This module implements the `DataAvailability` trait for blocks and neighborhoods.
//! Data availability is unit-scoped: homes replicate home data, neighborhoods
//! replicate neighborhood data.
//!
//! # Design
//!
//! **Home availability**: All residents of a home replicate all home-level
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

pub use home::HomeAvailability;
pub use neighborhood::NeighborhoodAvailability;
