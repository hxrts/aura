//! Aura Social - Social Topology Layer
//!
//! This crate provides the service/logic layer for Aura's urban social topology.
//! It builds on fact types defined in `aura-social::facts` to provide:
//!
//! - Materialized views: `Home`, `Neighborhood` aggregates from journal facts
//! - Services: `HomeService`, `NeighborhoodService`, `TraversalService`, `StorageService`
//! - Social topology: `SocialTopology` aggregated view for relay and discovery
//!
//! # Architecture
//!
//! This is a **Layer 5 (Feature/Protocol)** crate that:
//! - Depends on `aura-journal` (Layer 2) for journal infrastructure
//! - Is a peer to `aura-chat`, `aura-recovery`, `aura-invitation`
//!
//! # Fact vs Service Separation
//!
//! Facts (data structures) live in `aura-social::facts`:
//! - `HomeId`, `NeighborhoodId` - Identifiers
//! - `HomeFact`, `ResidentFact`, `StewardFact` - Home facts
//! - `NeighborhoodFact`, `HomeMemberFact`, `AdjacencyFact` - Neighborhood facts
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
//! use aura_social::{Home, Neighborhood, SocialTopology};
//! use aura_social::facts::{HomeFact, ResidentFact};
//!
//! // Build a Home view from journal facts
//! let home = Home::from_facts(&home_fact, &residents, &stewards);
//!
//! // Check membership
//! if home.is_resident(&authority_id) {
//!     println!("Authority is a resident");
//! }
//!
//! // Build social topology for relay selection
//! let topology = SocialTopology::from_journal(&journal, local_authority);
//! let home_peers = topology.home_peers();
//! ```

pub mod availability;
pub mod home;
pub mod error;
pub mod facts;
pub mod membership;
pub mod moderation;
pub mod neighborhood;
pub mod relay;
pub mod storage;
pub mod topology;
pub mod traversal;

/// Operation category map (A/B/C) for protocol gating and review.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    ("social:home-create", "B"),
    ("social:home-delete", "B"),
    ("social:resident-join", "B"),
    ("social:resident-leave", "B"),
    ("social:steward-grant", "B"),
    ("social:steward-revoke", "B"),
    ("social:neighborhood-create", "B"),
    ("social:neighborhood-join", "B"),
];

/// Lookup the operation category (A/B/C) for a given operation.
pub fn operation_category(operation: &str) -> Option<&'static str> {
    OPERATION_CATEGORIES
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, category)| *category)
}

// Re-export primary types
pub use availability::{HomeAvailability, NeighborhoodAvailability};
pub use home::Home;
pub use error::SocialError;
pub use facts::{SocialFact, SocialFactReducer, SOCIAL_FACT_TYPE_ID};
pub use moderation::{
    is_user_banned, is_user_muted, query_current_bans, query_current_mutes, query_kick_history,
    register_moderation_facts, BanStatus, HomeBanFact, HomeGrantStewardFact, HomeKickFact,
    HomeMuteFact, HomeRevokeStewardFact, HomeUnbanFact, HomeUnmuteFact, KickRecord, MuteStatus,
};
pub use neighborhood::Neighborhood;
pub use relay::{ReachabilityChecker, RelayCandidateBuilder};
pub use storage::StorageService;
pub use topology::{DiscoveryLayer, SocialTopology};
pub use traversal::TraversalService;

// Re-export fact types for convenience
pub use crate::facts::{
    AdjacencyFact, HomeConfigFact, HomeFact, HomeId, HomeMemberFact, HomeMessageMemberFact,
    HomeStorageBudget, NeighborhoodFact, NeighborhoodId, PinnedContentFact, ResidentFact,
    SocialFactError, StewardCapabilities, StewardFact, TraversalAllowedFact, TraversalDepth,
    TraversalPosition,
};
