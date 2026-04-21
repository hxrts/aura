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
//! - `HomeFact`, `HomeMemberFact`, `ModeratorFact` - Home facts
//! - `NeighborhoodFact`, `NeighborhoodMemberFact`, `OneHopLinkFact` - Neighborhood facts
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
//! use aura_social::facts::{HomeFact, HomeMemberFact};
//!
//! // Build a Home view from journal facts
//! let home = Home::from_facts(&home_fact, &members, &moderators);
//!
//! // Check membership
//! if home.is_member(&authority_id) {
//!     println!("Authority is a member");
//! }
//!
//! // Build social topology for relay selection
//! let topology = SocialTopology::from_journal(&journal, local_authority);
//! let same_home_members = topology.same_home_members();
//! ```

#[cfg(all(feature = "transparent_onion", not(any(test, debug_assertions))))]
compile_error!(
    "Feature `transparent_onion` is a debug/test/simulation-only tool and must \
     not be enabled in release production builds."
);

pub mod access;
pub mod availability;
pub mod error;
pub mod facts;
pub mod home;
pub mod membership;
pub mod moderation;
pub mod neighborhood;
pub mod relay;
pub mod storage;
pub mod topology;

/// Operation category map (A/B/C) for protocol gating and review.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    // Category A: read-only social queries
    ("social:neighborhood-list-members", "A"),
    ("social:neighborhood-compute-access-level", "A"),
    ("social:neighborhood-view-routes", "A"),
    // Category B: monotone additions
    ("social:home-create", "B"),
    ("social:home-delete", "B"),
    ("social:member-join", "B"),
    ("social:member-leave", "B"),
    ("social:moderator-grant", "B"),
    ("social:moderator-revoke", "B"),
    ("social:neighborhood-create", "B"),
    ("social:neighborhood-join", "B"),
    ("social:neighborhood-propose-join", "B"),
    ("social:neighborhood-accept-join", "B"),
    ("social:neighborhood-publish-reentry", "B"),
    // Category C: non-monotone neighborhood mutations
    ("social:neighborhood-remove-member", "C"),
    ("social:neighborhood-policy-change", "C"),
    ("social:neighborhood-override-change", "C"),
];

/// Neighborhood authority invariants.
pub const NEIGHBORHOOD_INVARIANTS: &[&str] = &[
    "InvariantNeighborhoodMemberType",
    "InvariantNeighborhoodMembershipUnique",
    "InvariantNeighborhoodMembershipFactBacked",
    "InvariantNeighborhoodViewDeterministic",
];

/// Lookup the operation category (A/B/C) for a given operation.
pub fn operation_category(operation: &str) -> Option<&'static str> {
    OPERATION_CATEGORIES
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, category)| *category)
}

// Re-export primary types
pub use access::{
    determine_access_level, determine_default_access_level, has_access_capability,
    minimum_hop_distance, resolve_access_capabilities, resolve_access_level_capability_config,
    TraversalService,
};
pub use availability::{HomeAvailability, NeighborhoodAvailability};
pub use error::SocialError;
pub use facts::{SocialFact, SocialFactReducer, SOCIAL_FACT_TYPE_ID};
pub use home::Home;
pub use moderation::{
    is_user_banned, is_user_muted, query_current_bans, query_current_bans_in_live_channels,
    query_current_mutes, query_current_mutes_in_live_channels, query_kick_history,
    register_moderation_facts, BanStatus, HomeBanFact, HomeGrantModeratorFact, HomeKickFact,
    HomeMuteFact, HomeRevokeModeratorFact, HomeUnbanFact, HomeUnmuteFact, KickRecord,
    ModerationScopeKey, MuteStatus,
};
pub use neighborhood::Neighborhood;
pub use relay::{ReachabilityChecker, RelayCandidateBuilder};
pub use storage::StorageService;
pub use topology::{DiscoveryLayer, SocialTopology};

// Re-export fact types for convenience
pub use crate::facts::{
    AccessLevel, AccessLevelCapabilityConfig, AccessLevelCapabilityConfigFact, AccessOverrideFact,
    HomeConfigFact, HomeFact, HomeId, HomeMemberFact, HomeMessageMemberFact, HomeStorageBudget,
    ModeratorCapabilities, ModeratorCapability, ModeratorDesignation, ModeratorFact,
    NeighborhoodFact, NeighborhoodId, NeighborhoodMemberFact, OneHopLinkFact, PinnedFact,
    SocialFactError, TraversalAllowedFact, TraversalPosition,
};
