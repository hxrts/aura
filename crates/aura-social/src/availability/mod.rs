//! Data Availability Module
//!
//! This module implements the `DataAvailability` trait for blocks and neighborhoods.
//! Data availability is unit-scoped: blocks replicate block data, neighborhoods
//! replicate neighborhood data.
//!
//! # Design
//!
//! **Block availability**: All residents of a block replicate all block-level
//! shared data. When retrieving data, we try local storage first, then query
//! peers in deterministic order.
//!
//! **Neighborhood availability**: Each block in a neighborhood maintains a
//! representative. Neighborhood-level data is replicated across all member
//! blocks via their representatives.
//!
//! # Example
//!
//! ```ignore
//! use aura_social::availability::{BlockAvailability, NeighborhoodAvailability};
//!
//! // Create block availability for a block
//! let block_da = BlockAvailability::new(block, local_authority, storage, network);
//!
//! // Retrieve data from the block
//! let data = block_da.retrieve(block_id, &content_hash).await?;
//! ```

mod block;
mod neighborhood;

pub use block::BlockAvailability;
pub use neighborhood::NeighborhoodAvailability;
