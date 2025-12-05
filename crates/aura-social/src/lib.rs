//! Aura Social - Social Topology Layer
//!
//! This crate provides the service/logic layer for Aura's urban social topology.
//! It builds on fact types defined in `aura-journal::facts::social` to provide:
//!
//! - Materialized views: `Block`, `Neighborhood` aggregates from journal facts
//! - Services: `BlockService`, `NeighborhoodService`, `TraversalService`, `StorageService`
//! - Social topology: `SocialTopology` aggregated view for relay and discovery
//!
//! # Architecture
//!
//! This is a **Layer 5 (Feature/Protocol)** crate that:
//! - Depends on `aura-journal` (Layer 2) for fact types
//! - Depends on `aura-wot` (Layer 2) for capability evaluation
//! - Is a peer to `aura-chat`, `aura-recovery`, `aura-invitation`
//!
//! # Fact vs Service Separation
//!
//! Facts (data structures) live in `aura-journal::facts::social`:
//! - `BlockId`, `NeighborhoodId` - Identifiers
//! - `BlockFact`, `ResidentFact`, `StewardFact` - Block facts
//! - `NeighborhoodFact`, `BlockMemberFact`, `AdjacencyFact` - Neighborhood facts
//!
//! Services (business logic) live in this crate:
//! - Membership validation
//! - Traversal rules
//! - Storage policies
//! - Relay candidate building
//!
//! # Example
//!
//! ```ignore
//! use aura_social::{Block, Neighborhood, SocialTopology};
//! use aura_journal::facts::social::{BlockFact, ResidentFact};
//!
//! // Build a Block view from journal facts
//! let block = Block::from_facts(&block_fact, &residents, &stewards);
//!
//! // Check membership
//! if block.is_resident(&authority_id) {
//!     println!("Authority is a resident");
//! }
//!
//! // Build social topology for relay selection
//! let topology = SocialTopology::from_journal(&journal, local_authority);
//! let block_peers = topology.block_peers();
//! ```

pub mod availability;
pub mod block;
pub mod error;
pub mod membership;
pub mod neighborhood;
pub mod relay;
pub mod storage;
pub mod topology;
pub mod traversal;

// Re-export primary types
pub use availability::{BlockAvailability, NeighborhoodAvailability};
pub use block::Block;
pub use error::SocialError;
pub use neighborhood::Neighborhood;
pub use relay::{ReachabilityChecker, RelayCandidateBuilder};
pub use storage::StorageService;
pub use topology::{DiscoveryLayer, SocialTopology};
pub use traversal::TraversalService;

// Re-export fact types from aura-journal for convenience
pub use aura_journal::facts::social::{
    AdjacencyFact, BlockConfigFact, BlockFact, BlockId, BlockMemberFact, BlockStorageBudget,
    NeighborhoodFact, NeighborhoodId, ResidentFact, StewardCapabilities, StewardFact,
    TraversalAllowedFact, TraversalDepth, TraversalPosition,
};
