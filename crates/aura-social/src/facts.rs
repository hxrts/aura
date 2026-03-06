//! Social domain facts
//!
//! This module defines social-specific fact types that implement the `DomainFact`
//! trait from `aura-journal`. These facts are stored as `RelationalFact::Generic`
//! in the journal and can be reduced using the `SocialFactReducer`.
//!
//! # Architecture
//!
//! Following the Open/Closed Principle:
//! - `aura-journal` provides the generic fact infrastructure
//! - `aura-social` defines domain-specific fact types without modifying `aura-journal`
//! - Runtime registers `SocialFactReducer` with the `FactRegistry`
//!
//! # Example
//!
//! ```ignore
//! use aura_social::facts::{SocialFact, SOCIAL_FACT_TYPE_ID};
//! use aura_journal::DomainFact;
//!
//! // Create a home created fact
//! let fact = SocialFact::home_created(home_id, context_id, timestamp, creator_id, "My Home");
//!
//! // Convert to generic for storage
//! let generic = fact.to_generic();
//!
//! // Deserialize from bytes
//! let restored = SocialFact::from_bytes(&fact.to_bytes());
//! ```

use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::Hash32;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// Re-export HomeId and NeighborhoodId from aura-core for backwards compatibility
pub use aura_core::identifiers::{HomeId, NeighborhoodId};

// ============================================================================
// Home Facts
// ============================================================================

/// Home existence and configuration fact
///
/// Corresponds to: `home(home_id, created_at, storage_limit)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeFact {
    /// Unique identifier for this home
    pub home_id: HomeId,
    /// When the home was created
    pub created_at: TimeStamp,
    /// Total storage limit in bytes (default: 10 MB)
    pub storage_limit: u64,
}

impl HomeFact {
    /// Default storage limit: 10 MB
    pub const DEFAULT_STORAGE_LIMIT: u64 = 10 * 1024 * 1024;

    /// Create a new home with default storage limit
    pub fn new(home_id: HomeId, created_at: TimeStamp) -> Self {
        Self {
            home_id,
            created_at,
            storage_limit: Self::DEFAULT_STORAGE_LIMIT,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "home(\"{}\", \"{}\", {});",
            self.home_id,
            self.created_at.to_index_ms(),
            self.storage_limit
        )
    }
}

/// Home configuration fact
///
/// Corresponds to: `home_config(home_id, max_members, neighborhood_limit)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HomeConfigFact {
    /// Home this configuration applies to
    pub home_id: HomeId,
    /// Maximum number of members (v1: 8)
    pub max_members: u8,
    /// Maximum number of neighborhoods to join (v1: 4)
    pub neighborhood_limit: u8,
}

impl HomeConfigFact {
    /// v1 maximum members per home
    pub const V1_MAX_MEMBERS: u8 = 8;
    /// v1 maximum neighborhoods per home
    pub const V1_NEIGHBORHOOD_LIMIT: u8 = 4;

    /// Create a v1-compliant configuration
    pub fn v1_default(home_id: HomeId) -> Self {
        Self {
            home_id,
            max_members: Self::V1_MAX_MEMBERS,
            neighborhood_limit: Self::V1_NEIGHBORHOOD_LIMIT,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "home_config(\"{}\", {}, {});",
            self.home_id, self.max_members, self.neighborhood_limit
        )
    }

    /// Validate against v1 constraints
    pub fn validate_v1(&self) -> Result<(), SocialFactError> {
        if self.max_members > Self::V1_MAX_MEMBERS {
            return Err(SocialFactError::V1ConstraintViolation(format!(
                "max_members {} exceeds v1 limit of {}",
                self.max_members,
                Self::V1_MAX_MEMBERS
            )));
        }
        if self.neighborhood_limit > Self::V1_NEIGHBORHOOD_LIMIT {
            return Err(SocialFactError::V1ConstraintViolation(format!(
                "neighborhood_limit {} exceeds v1 limit of {}",
                self.neighborhood_limit,
                Self::V1_NEIGHBORHOOD_LIMIT
            )));
        }
        Ok(())
    }
}

/// Home member fact - user membership in a home
///
/// Corresponds to: `member(authority_id, home_id, joined_at, storage_allocated)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeMemberFact {
    /// Authority of the member
    pub authority_id: AuthorityId,
    /// Home where the authority is a member
    pub home_id: HomeId,
    /// When the authority joined the home
    pub joined_at: TimeStamp,
    /// Storage allocated by this member (default: 200 KB)
    pub storage_allocated: u64,
}

impl HomeMemberFact {
    /// Default storage allocation per member: 200 KB
    pub const DEFAULT_STORAGE_ALLOCATION: u64 = 200 * 1024;

    /// Create a new member with default storage allocation
    pub fn new(authority_id: AuthorityId, home_id: HomeId, joined_at: TimeStamp) -> Self {
        Self {
            authority_id,
            home_id,
            joined_at,
            storage_allocated: Self::DEFAULT_STORAGE_ALLOCATION,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "member(\"{}\", \"{}\", \"{}\", {});",
            self.authority_id,
            self.home_id,
            self.joined_at.to_index_ms(),
            self.storage_allocated
        )
    }
}

/// Individual moderator capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModeratorCapability {
    /// Can kick users from channels/home.
    Kick,
    /// Can ban users from channels/home.
    Ban,
    /// Can mute users in channels/home.
    Mute,
    /// Can pin and unpin messages.
    PinContent,
    /// Can grant/revoke moderator designation.
    GrantModerator,
    /// Can manage channel modes/settings.
    ManageChannel,
}

/// Moderator capability bundle.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModeratorCapabilities {
    /// Can kick users.
    pub kick: bool,
    /// Can ban users.
    pub ban: bool,
    /// Can mute users.
    pub mute: bool,
    /// Can pin/unpin content
    pub pin_content: bool,
    /// Can grant moderator capabilities to others
    pub grant_moderator: bool,
    /// Can manage channels
    pub manage_channel: bool,
    /// Custom capability strings
    pub custom: BTreeSet<String>,
}

impl Default for ModeratorCapabilities {
    fn default() -> Self {
        Self {
            kick: true,
            ban: true,
            mute: true,
            pin_content: true,
            grant_moderator: false,
            manage_channel: true,
            custom: BTreeSet::new(),
        }
    }
}

impl ModeratorCapabilities {
    /// Create full moderator capabilities
    pub fn full() -> Self {
        Self {
            kick: true,
            ban: true,
            mute: true,
            pin_content: true,
            grant_moderator: true,
            manage_channel: true,
            custom: BTreeSet::new(),
        }
    }

    /// Check whether this bundle allows a specific moderator capability.
    pub fn allows(&self, capability: ModeratorCapability) -> bool {
        match capability {
            ModeratorCapability::Kick => self.kick,
            ModeratorCapability::Ban => self.ban,
            ModeratorCapability::Mute => self.mute,
            ModeratorCapability::PinContent => self.pin_content,
            ModeratorCapability::GrantModerator => self.grant_moderator,
            ModeratorCapability::ManageChannel => self.manage_channel,
        }
    }

    /// Convert to a capability string set for Biscuit
    pub fn to_capability_set(&self) -> BTreeSet<String> {
        let mut caps = BTreeSet::new();
        if self.kick {
            caps.insert("moderate:kick".to_string());
        }
        if self.ban {
            caps.insert("moderate:ban".to_string());
        }
        if self.mute {
            caps.insert("moderate:mute".to_string());
        }
        if self.pin_content {
            caps.insert("pin_content".to_string());
        }
        if self.grant_moderator {
            caps.insert("grant_moderator".to_string());
        }
        if self.manage_channel {
            caps.insert("manage_channel".to_string());
        }
        caps.extend(self.custom.clone());
        caps
    }
}

/// Moderator fact - authority with elevated capabilities in a home
///
/// Corresponds to: `moderator(authority_id, home_id, granted_at, capabilities)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeratorFact {
    /// Authority with moderator role
    pub authority_id: AuthorityId,
    /// Home where the moderator operates
    pub home_id: HomeId,
    /// When moderator capabilities were granted
    pub granted_at: TimeStamp,
    /// Capability bundle for this moderator
    pub capabilities: ModeratorCapabilities,
}

impl ModeratorFact {
    /// Create a new moderator with default capabilities
    pub fn new(authority_id: AuthorityId, home_id: HomeId, granted_at: TimeStamp) -> Self {
        Self {
            authority_id,
            home_id,
            granted_at,
            capabilities: ModeratorCapabilities::default(),
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        let caps = self.capabilities.to_capability_set();
        let caps_str: Vec<_> = caps.iter().map(|s| format!("\"{s}\"")).collect();
        format!(
            "moderator(\"{}\", \"{}\", \"{}\", [{}]);",
            self.authority_id,
            self.home_id,
            self.granted_at.to_index_ms(),
            caps_str.join(", ")
        )
    }
}

/// Moderator designation attached to an existing home member.
///
/// This models moderator as a designation, not a membership tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeratorDesignation {
    /// Authority holding moderator designation.
    pub authority_id: AuthorityId,
    /// Home where designation applies.
    pub home_id: HomeId,
    /// When designation was applied.
    pub designated_at: TimeStamp,
    /// Capability bundle for this moderator.
    pub capabilities: ModeratorCapabilities,
}

impl ModeratorDesignation {
    /// Create a designation with default moderator capabilities.
    pub fn new(authority_id: AuthorityId, home_id: HomeId, designated_at: TimeStamp) -> Self {
        Self {
            authority_id,
            home_id,
            designated_at,
            capabilities: ModeratorCapabilities::default(),
        }
    }

    /// Check whether this designation grants a specific capability.
    pub fn allows(&self, capability: ModeratorCapability) -> bool {
        self.capabilities.allows(capability)
    }
}

impl From<&ModeratorFact> for ModeratorDesignation {
    fn from(value: &ModeratorFact) -> Self {
        Self {
            authority_id: value.authority_id,
            home_id: value.home_id,
            designated_at: value.granted_at.clone(),
            capabilities: value.capabilities.clone(),
        }
    }
}

/// Home message membership fact (derived from residency)
///
/// Corresponds to: `home_message_member(authority_id, channel_id, home_id)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HomeMessageMemberFact {
    /// Authority with message access
    pub authority_id: AuthorityId,
    /// Channel the authority can access
    pub channel_id: ChannelId,
    /// Home this channel belongs to
    pub home_id: HomeId,
}

impl HomeMessageMemberFact {
    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "home_message_member(\"{}\", \"{}\", \"{}\");",
            self.authority_id, self.channel_id, self.home_id
        )
    }
}

/// Pinned fact
///
/// Corresponds to: `pinned(content_hash, home_id, pinned_by, pinned_at, size_bytes)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedFact {
    /// Hash of the pinned content
    pub content_hash: Hash32,
    /// Home where content is pinned
    pub home_id: HomeId,
    /// Authority who pinned the content
    pub pinned_by: AuthorityId,
    /// When the content was pinned
    pub pinned_at: TimeStamp,
    /// Size of the pinned content in bytes
    pub size_bytes: u64,
}

impl PinnedFact {
    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "pinned(hex:{:?}, \"{}\", \"{}\", \"{}\", {});",
            self.content_hash.0,
            self.home_id,
            self.pinned_by,
            self.pinned_at.to_index_ms(),
            self.size_bytes
        )
    }
}

// ============================================================================
// Neighborhood Facts
// ============================================================================

/// Neighborhood existence fact
///
/// Corresponds to: `neighborhood(neighborhood_id, created_at)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborhoodFact {
    /// Unique identifier for this neighborhood
    pub neighborhood_id: NeighborhoodId,
    /// When the neighborhood was created
    pub created_at: TimeStamp,
}

impl NeighborhoodFact {
    /// Create a new neighborhood
    pub fn new(neighborhood_id: NeighborhoodId, created_at: TimeStamp) -> Self {
        Self {
            neighborhood_id,
            created_at,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "neighborhood(\"{}\", \"{}\");",
            self.neighborhood_id,
            self.created_at.to_index_ms()
        )
    }
}

/// Home membership in a neighborhood
///
/// Corresponds to: `home_member(home_id, neighborhood_id, joined_at, allocated_storage)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborhoodMemberFact {
    /// Home joining the neighborhood
    pub home_id: HomeId,
    /// Neighborhood being joined
    pub neighborhood_id: NeighborhoodId,
    /// When the home joined
    pub joined_at: TimeStamp,
    /// Storage allocated to neighborhood infrastructure (default: 1 MB)
    #[serde(alias = "donated_storage")]
    pub allocated_storage: u64,
}

impl NeighborhoodMemberFact {
    /// Default storage allocation per neighborhood: 1 MB
    pub const DEFAULT_ALLOCATION: u64 = 1024 * 1024;

    /// Create a new home membership with default allocation
    pub fn new(home_id: HomeId, neighborhood_id: NeighborhoodId, joined_at: TimeStamp) -> Self {
        Self {
            home_id,
            neighborhood_id,
            joined_at,
            allocated_storage: Self::DEFAULT_ALLOCATION,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "home_member(\"{}\", \"{}\", \"{}\", {});",
            self.home_id,
            self.neighborhood_id,
            self.joined_at.to_index_ms(),
            self.allocated_storage
        )
    }
}

/// OneHopLink relationship between homes
///
/// Corresponds to: `adjacent(home_a, home_b, neighborhood_id)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OneHopLinkFact {
    /// First home in the one_hop_link relationship
    pub home_a: HomeId,
    /// Second home in the one_hop_link relationship
    pub home_b: HomeId,
    /// Neighborhood where this one_hop_link exists
    pub neighborhood_id: NeighborhoodId,
}

impl OneHopLinkFact {
    /// Create a new one_hop_link (ordered by home IDs for consistency)
    pub fn new(home_a: HomeId, home_b: HomeId, neighborhood_id: NeighborhoodId) -> Self {
        // Ensure consistent ordering
        let (a, b) = if home_a <= home_b {
            (home_a, home_b)
        } else {
            (home_b, home_a)
        };
        Self {
            home_a: a,
            home_b: b,
            neighborhood_id,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "adjacent(\"{}\", \"{}\", \"{}\");",
            self.home_a, self.home_b, self.neighborhood_id
        )
    }
}

/// Traversal permission fact
///
/// Corresponds to: `traversal_allowed(from_home, to_home, capability_requirement)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TraversalAllowedFact {
    /// Home to traverse from
    pub from_home: HomeId,
    /// Home to traverse to
    pub to_home: HomeId,
    /// Capability required for this traversal
    pub capability_requirement: String,
}

impl TraversalAllowedFact {
    /// Create a traversal permission
    pub fn new(
        from_home: HomeId,
        to_home: HomeId,
        capability_requirement: impl Into<String>,
    ) -> Self {
        Self {
            from_home,
            to_home,
            capability_requirement: capability_requirement.into(),
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "traversal_allowed(\"{}\", \"{}\", \"{}\");",
            self.from_home, self.to_home, self.capability_requirement
        )
    }
}

// ============================================================================
// Traversal Position
// ============================================================================

/// Access level within a home
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AccessLevel {
    /// Limited capability set (default mapping: 2-hop-or-greater or disconnected)
    Limited,
    /// Partial capability set (default mapping: 1-hop neighborhood)
    Partial,
    /// Full capability set (default mapping: same-home)
    Full,
}

impl AccessLevel {
    /// Check if an explicit override from `self` to `target` is allowed.
    ///
    /// Policy:
    /// - Limited -> Partial (upgrade) is allowed.
    /// - Partial -> Limited (downgrade) is allowed.
    /// - All other transitions are rejected.
    pub fn allows_override_to(self, target: AccessLevel) -> bool {
        matches!(
            (self, target),
            (AccessLevel::Limited, AccessLevel::Partial)
                | (AccessLevel::Partial, AccessLevel::Limited)
        )
    }
}

/// Per-authority access-level override for a specific home.
///
/// Overrides are applied after deterministic default mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessOverrideFact {
    /// Authority receiving the override.
    pub authority_id: AuthorityId,
    /// Home where this override applies.
    pub home_id: HomeId,
    /// Override level to apply for this authority+home pair.
    pub access_level: AccessLevel,
    /// When this override was set.
    pub set_at: TimeStamp,
}

impl AccessOverrideFact {
    /// Create an override fact, validating that the override transition is allowed.
    pub fn new_validated(
        authority_id: AuthorityId,
        home_id: HomeId,
        default_level: AccessLevel,
        access_level: AccessLevel,
        set_at: TimeStamp,
    ) -> Result<Self, SocialFactError> {
        if !default_level.allows_override_to(access_level) {
            return Err(SocialFactError::InvalidFact(format!(
                "invalid access override transition: {default_level:?} -> {access_level:?}"
            )));
        }
        Ok(Self {
            authority_id,
            home_id,
            access_level,
            set_at,
        })
    }

    /// Check whether this override is valid for the computed default level.
    pub fn is_valid_for_default(&self, default_level: AccessLevel) -> bool {
        default_level.allows_override_to(self.access_level)
    }

    /// Convert to Datalog fact string.
    pub fn to_datalog(&self) -> String {
        format!(
            "access_override(\"{}\", \"{}\", \"{:?}\", \"{}\");",
            self.authority_id,
            self.home_id,
            self.access_level,
            self.set_at.to_index_ms()
        )
    }
}

/// Per-home capability configuration for Full/Partial/Limited access levels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessLevelCapabilityConfig {
    /// Capabilities granted to Full access users.
    pub full: BTreeSet<String>,
    /// Capabilities granted to Partial access users.
    pub partial: BTreeSet<String>,
    /// Capabilities granted to Limited access users.
    pub limited: BTreeSet<String>,
}

impl Default for AccessLevelCapabilityConfig {
    fn default() -> Self {
        let full = [
            "send_dm",
            "send_message",
            "update_contact",
            "view_members",
            "join_channel",
            "leave_context",
            "invite",
            "manage_channel",
            "pin_content",
            "moderate:kick",
            "moderate:ban",
            "moderate:mute",
            "grant_moderator",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let partial = [
            "send_dm",
            "send_message",
            "update_contact",
            "view_members",
            "join_channel",
            "leave_context",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let limited = ["send_dm", "view_members"]
            .into_iter()
            .map(str::to_string)
            .collect();

        Self {
            full,
            partial,
            limited,
        }
    }
}

impl AccessLevelCapabilityConfig {
    /// Return capabilities granted for the given level.
    pub fn capabilities_for(&self, level: AccessLevel) -> &BTreeSet<String> {
        match level {
            AccessLevel::Full => &self.full,
            AccessLevel::Partial => &self.partial,
            AccessLevel::Limited => &self.limited,
        }
    }

    /// Check whether a capability is granted at the given access level.
    pub fn allows(&self, level: AccessLevel, capability: &str) -> bool {
        self.capabilities_for(level).contains(capability)
    }
}

/// Fact storing per-home access-level capability configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessLevelCapabilityConfigFact {
    /// Home where this configuration applies.
    pub home_id: HomeId,
    /// Capability configuration payload.
    pub config: AccessLevelCapabilityConfig,
    /// When this configuration was set.
    pub configured_at: TimeStamp,
}

impl AccessLevelCapabilityConfigFact {
    /// Create default capability config for a home.
    pub fn default_for_home(home_id: HomeId, configured_at: TimeStamp) -> Self {
        Self {
            home_id,
            config: AccessLevelCapabilityConfig::default(),
            configured_at,
        }
    }

    /// Convert to Datalog fact string.
    pub fn to_datalog(&self) -> String {
        let render_caps = |caps: &BTreeSet<String>| {
            caps.iter()
                .map(|cap| format!("\"{cap}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };

        format!(
            "access_level_capabilities(\"{}\", [{}], [{}], [{}], \"{}\");",
            self.home_id,
            render_caps(&self.config.full),
            render_caps(&self.config.partial),
            render_caps(&self.config.limited),
            self.configured_at.to_index_ms()
        )
    }
}

/// Current position in neighborhood traversal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalPosition {
    /// Current neighborhood (None = outside a neighborhood context)
    pub neighborhood: Option<NeighborhoodId>,
    /// Current home (None = between homes)
    pub current_home: Option<HomeId>,
    /// Depth of access
    pub depth: AccessLevel,
    /// Relational context containing capabilities
    pub context_id: ContextId,
    /// When this position was entered (for expiration tracking)
    pub entered_at: TimeStamp,
}

// ============================================================================
// Storage Budget
// ============================================================================

/// Home storage budget tracking
///
/// Tracks spent counters as facts; limits are derived at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeStorageBudget {
    /// Home this budget tracks
    pub home_id: HomeId,
    /// Current member storage spent (sum of allocations)
    pub member_storage_spent: u64,
    /// Current pinned storage spent
    pub pinned_storage_spent: u64,
    /// Current neighborhood allocations (n * 1 MB)
    pub neighborhood_allocations: u64,
}

impl HomeStorageBudget {
    /// Create a new empty budget
    pub fn new(home_id: HomeId) -> Self {
        Self {
            home_id,
            member_storage_spent: 0,
            pinned_storage_spent: 0,
            neighborhood_allocations: 0,
        }
    }

    /// Calculate remaining shared storage
    ///
    /// Formula: Total (10 MB) - Member - Neighborhood Allocations - Pinned
    pub fn remaining_shared_storage(&self) -> u64 {
        let total = HomeFact::DEFAULT_STORAGE_LIMIT;
        let spent =
            self.member_storage_spent + self.neighborhood_allocations + self.pinned_storage_spent;
        total.saturating_sub(spent)
    }

    /// Derive member storage limit (8 * 200 KB = 1.6 MB)
    pub fn member_storage_limit(&self) -> u64 {
        HomeConfigFact::V1_MAX_MEMBERS as u64 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION
    }

    /// Derive pinned storage limit based on neighborhood count
    pub fn pinned_storage_limit(&self) -> u64 {
        let total = HomeFact::DEFAULT_STORAGE_LIMIT;
        let member_limit = self.member_storage_limit();
        total.saturating_sub(member_limit + self.neighborhood_allocations)
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Errors related to social facts
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocialFactError {
    /// v1 constraint violation
    V1ConstraintViolation(String),
    /// Invalid fact data
    InvalidFact(String),
    /// Storage limit exceeded
    StorageLimitExceeded {
        /// Type of budget that was exceeded
        budget_type: String,
        /// Current usage in bytes
        current: u64,
        /// Maximum limit in bytes
        limit: u64,
    },
}

impl std::fmt::Display for SocialFactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocialFactError::V1ConstraintViolation(msg) => {
                write!(f, "v1 constraint violation: {msg}")
            }
            SocialFactError::InvalidFact(msg) => {
                write!(f, "invalid fact: {msg}")
            }
            SocialFactError::StorageLimitExceeded {
                budget_type,
                current,
                limit,
            } => {
                write!(
                    f,
                    "{budget_type} storage limit exceeded: {current} / {limit} bytes"
                )
            }
        }
    }
}

impl std::error::Error for SocialFactError {}

/// Type identifier for social facts
pub const SOCIAL_FACT_TYPE_ID: &str = "social";

/// Key for indexing social facts in the journal
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocialFactKey {
    /// Sub-type discriminator for the fact
    pub sub_type: &'static str,
    /// Serialized key data for lookup
    pub data: Vec<u8>,
}

/// Social domain fact types
///
/// These facts represent social-related state changes in the journal,
/// including blocks, members, moderators, and neighborhoods.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "social", schema_version = 1, context = "context_id")]
pub enum SocialFact {
    /// Home created
    HomeCreated {
        /// Unique home identifier
        home_id: HomeId,
        /// Relational context for this home
        context_id: ContextId,
        /// When the home was created
        created_at: PhysicalTime,
        /// Authority that created the home
        creator_id: AuthorityId,
        /// Human-readable home name
        name: String,
        /// Storage limit in bytes (default: 10 MB)
        storage_limit: u64,
    },
    /// Home deleted/archived
    HomeDeleted {
        /// Home being deleted
        home_id: HomeId,
        /// Relational context for this home
        context_id: ContextId,
        /// When the home was deleted
        deleted_at: PhysicalTime,
        /// Authority that deleted the home
        actor_id: AuthorityId,
    },
    /// Member joined a home
    MemberJoined {
        /// Authority joining the home
        authority_id: AuthorityId,
        /// Home being joined
        home_id: HomeId,
        /// Relational context
        context_id: ContextId,
        /// When the member joined
        joined_at: PhysicalTime,
        /// Human-readable name for the member
        name: String,
        /// Storage allocated in bytes (default: 200 KB)
        storage_allocated: u64,
    },
    /// Member left a home
    MemberLeft {
        /// Authority leaving the home
        authority_id: AuthorityId,
        /// Home being left
        home_id: HomeId,
        /// Relational context
        context_id: ContextId,
        /// When the member left
        left_at: PhysicalTime,
    },
    /// Moderator granted capabilities in a home
    ModeratorGranted {
        /// Authority being granted moderator role
        authority_id: AuthorityId,
        /// Home where moderator operates
        home_id: HomeId,
        /// Relational context
        context_id: ContextId,
        /// When moderator was granted
        granted_at: PhysicalTime,
        /// Authority granting the moderator role
        grantor_id: AuthorityId,
        /// Capability strings granted
        capabilities: Vec<String>,
    },
    /// Moderator revoked from a home
    ModeratorRevoked {
        /// Authority losing moderator role
        authority_id: AuthorityId,
        /// Home where moderator was revoked
        home_id: HomeId,
        /// Relational context
        context_id: ContextId,
        /// When moderator was revoked
        revoked_at: PhysicalTime,
        /// Authority revoking the moderator role
        revoker_id: AuthorityId,
    },
    /// Explicit per-authority access override within a home.
    AccessOverrideSet {
        /// Authority receiving the override.
        authority_id: AuthorityId,
        /// Home where the override applies.
        home_id: HomeId,
        /// Relational context for the home.
        context_id: ContextId,
        /// Override level.
        access_level: AccessLevel,
        /// When the override was set.
        set_at: PhysicalTime,
    },
    /// Per-home capability mapping for full/partial/limited access.
    AccessLevelCapabilitiesConfigured {
        /// Home whose capability mapping changed.
        home_id: HomeId,
        /// Relational context for the home.
        context_id: ContextId,
        /// Capabilities granted to full access.
        full_caps: Vec<String>,
        /// Capabilities granted to partial access.
        partial_caps: Vec<String>,
        /// Capabilities granted to limited access.
        limited_caps: Vec<String>,
        /// When the capability mapping was configured.
        configured_at: PhysicalTime,
    },
    /// Home storage updated
    StorageUpdated {
        /// Home whose storage changed
        home_id: HomeId,
        /// Relational context
        context_id: ContextId,
        /// Total bytes used
        used_bytes: u64,
        /// Total bytes available
        total_bytes: u64,
        /// When storage was updated
        updated_at: PhysicalTime,
    },
    /// Neighborhood created
    NeighborhoodCreated {
        /// Unique neighborhood identifier
        neighborhood_id: NeighborhoodId,
        /// Relational context
        context_id: ContextId,
        /// When the neighborhood was created
        created_at: PhysicalTime,
        /// Human-readable neighborhood name
        name: String,
    },
    /// Home joined a neighborhood
    HomeJoinedNeighborhood {
        /// Home joining the neighborhood
        home_id: HomeId,
        /// Neighborhood being joined
        neighborhood_id: NeighborhoodId,
        /// Relational context
        context_id: ContextId,
        /// When the home joined
        joined_at: PhysicalTime,
    },
    /// Home left a neighborhood
    HomeLeftNeighborhood {
        /// Home leaving the neighborhood
        home_id: HomeId,
        /// Neighborhood being left
        neighborhood_id: NeighborhoodId,
        /// Relational context
        context_id: ContextId,
        /// When the home left
        left_at: PhysicalTime,
    },
}

impl SocialFact {
    /// Default storage limit for blocks: 10 MB
    pub const DEFAULT_BLOCK_STORAGE_LIMIT: u64 = 10 * 1024 * 1024;

    /// Default storage allocation for members: 200 KB
    pub const DEFAULT_MEMBER_STORAGE: u64 = 200 * 1024;

    /// Get the timestamp in milliseconds
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            SocialFact::HomeCreated { created_at, .. } => created_at.ts_ms,
            SocialFact::HomeDeleted { deleted_at, .. } => deleted_at.ts_ms,
            SocialFact::MemberJoined { joined_at, .. } => joined_at.ts_ms,
            SocialFact::MemberLeft { left_at, .. } => left_at.ts_ms,
            SocialFact::ModeratorGranted { granted_at, .. } => granted_at.ts_ms,
            SocialFact::ModeratorRevoked { revoked_at, .. } => revoked_at.ts_ms,
            SocialFact::AccessOverrideSet { set_at, .. } => set_at.ts_ms,
            SocialFact::AccessLevelCapabilitiesConfigured { configured_at, .. } => {
                configured_at.ts_ms
            }
            SocialFact::StorageUpdated { updated_at, .. } => updated_at.ts_ms,
            SocialFact::NeighborhoodCreated { created_at, .. } => created_at.ts_ms,
            SocialFact::HomeJoinedNeighborhood { joined_at, .. } => joined_at.ts_ms,
            SocialFact::HomeLeftNeighborhood { left_at, .. } => left_at.ts_ms,
        }
    }

    /// Validate that this fact can be reduced under the provided context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id() == context_id
    }

    /// Derive the relational binding subtype and key data for this fact.
    pub fn binding_key(&self) -> SocialFactKey {
        match self {
            SocialFact::HomeCreated { home_id, .. } => SocialFactKey {
                sub_type: "home-created",
                data: home_id.as_bytes().to_vec(),
            },
            SocialFact::HomeDeleted { home_id, .. } => SocialFactKey {
                sub_type: "home-deleted",
                data: home_id.as_bytes().to_vec(),
            },
            SocialFact::MemberJoined { authority_id, .. } => SocialFactKey {
                sub_type: "member-joined",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::MemberLeft { authority_id, .. } => SocialFactKey {
                sub_type: "member-left",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::ModeratorGranted { authority_id, .. } => SocialFactKey {
                sub_type: "moderator-granted",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::ModeratorRevoked { authority_id, .. } => SocialFactKey {
                sub_type: "moderator-revoked",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::AccessOverrideSet {
                authority_id,
                home_id,
                ..
            } => {
                let mut data = authority_id.to_string().into_bytes();
                data.extend_from_slice(home_id.as_bytes());
                SocialFactKey {
                    sub_type: "access-override-set",
                    data,
                }
            }
            SocialFact::AccessLevelCapabilitiesConfigured { home_id, .. } => SocialFactKey {
                sub_type: "access-level-capabilities-configured",
                data: home_id.as_bytes().to_vec(),
            },
            SocialFact::StorageUpdated { home_id, .. } => SocialFactKey {
                sub_type: "storage-updated",
                data: home_id.as_bytes().to_vec(),
            },
            SocialFact::NeighborhoodCreated {
                neighborhood_id, ..
            } => SocialFactKey {
                sub_type: "neighborhood-created",
                data: neighborhood_id.as_bytes().to_vec(),
            },
            SocialFact::HomeJoinedNeighborhood {
                home_id,
                neighborhood_id,
                ..
            } => {
                let mut data = home_id.as_bytes().to_vec();
                data.extend_from_slice(neighborhood_id.as_bytes());
                SocialFactKey {
                    sub_type: "home-joined-neighborhood",
                    data,
                }
            }
            SocialFact::HomeLeftNeighborhood {
                home_id,
                neighborhood_id,
                ..
            } => {
                let mut data = home_id.as_bytes().to_vec();
                data.extend_from_slice(neighborhood_id.as_bytes());
                SocialFactKey {
                    sub_type: "home-left-neighborhood",
                    data,
                }
            }
        }
    }

    /// Create a HomeCreated fact with millisecond timestamp
    pub fn home_created_ms(
        home_id: HomeId,
        context_id: ContextId,
        created_at_ms: u64,
        creator_id: AuthorityId,
        name: String,
    ) -> Self {
        Self::HomeCreated {
            home_id,
            context_id,
            created_at: PhysicalTime {
                ts_ms: created_at_ms,
                uncertainty: None,
            },
            creator_id,
            name,
            storage_limit: Self::DEFAULT_BLOCK_STORAGE_LIMIT,
        }
    }

    /// Create a MemberJoined fact with millisecond timestamp
    pub fn member_joined_ms(
        authority_id: AuthorityId,
        home_id: HomeId,
        context_id: ContextId,
        joined_at_ms: u64,
        name: String,
    ) -> Self {
        Self::MemberJoined {
            authority_id,
            home_id,
            context_id,
            joined_at: PhysicalTime {
                ts_ms: joined_at_ms,
                uncertainty: None,
            },
            name,
            storage_allocated: Self::DEFAULT_MEMBER_STORAGE,
        }
    }

    /// Create a MemberLeft fact with millisecond timestamp
    pub fn member_left_ms(
        authority_id: AuthorityId,
        home_id: HomeId,
        context_id: ContextId,
        left_at_ms: u64,
    ) -> Self {
        Self::MemberLeft {
            authority_id,
            home_id,
            context_id,
            left_at: PhysicalTime {
                ts_ms: left_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a StorageUpdated fact with millisecond timestamp
    pub fn storage_updated_ms(
        home_id: HomeId,
        context_id: ContextId,
        used_bytes: u64,
        total_bytes: u64,
        updated_at_ms: u64,
    ) -> Self {
        Self::StorageUpdated {
            home_id,
            context_id,
            used_bytes,
            total_bytes,
            updated_at: PhysicalTime {
                ts_ms: updated_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AccessOverrideSet fact with millisecond timestamp.
    pub fn access_override_set_ms(
        authority_id: AuthorityId,
        home_id: HomeId,
        context_id: ContextId,
        access_level: AccessLevel,
        set_at_ms: u64,
    ) -> Self {
        Self::AccessOverrideSet {
            authority_id,
            home_id,
            context_id,
            access_level,
            set_at: PhysicalTime {
                ts_ms: set_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create an AccessLevelCapabilitiesConfigured fact with millisecond timestamp.
    pub fn access_level_capabilities_configured_ms(
        home_id: HomeId,
        context_id: ContextId,
        full_caps: Vec<String>,
        partial_caps: Vec<String>,
        limited_caps: Vec<String>,
        configured_at_ms: u64,
    ) -> Self {
        Self::AccessLevelCapabilitiesConfigured {
            home_id,
            context_id,
            full_caps,
            partial_caps,
            limited_caps,
            configured_at: PhysicalTime {
                ts_ms: configured_at_ms,
                uncertainty: None,
            },
        }
    }
}

/// Reducer for social facts
///
/// Converts social facts to relational bindings during journal reduction.
pub struct SocialFactReducer;

impl FactReducer for SocialFactReducer {
    fn handles_type(&self) -> &'static str {
        SOCIAL_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != SOCIAL_FACT_TYPE_ID {
            return None;
        }

        let fact = SocialFact::from_envelope(envelope)?;

        if !fact.validate_for_reduction(context_id) {
            return None;
        }

        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_home_id() -> HomeId {
        HomeId::from_bytes([1u8; 32])
    }

    fn test_authority_id() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    #[test]
    fn test_home_created_serialization() {
        let fact = SocialFact::home_created_ms(
            test_home_id(),
            test_context_id(),
            1234567890,
            test_authority_id(),
            "Test Home".to_string(),
        );

        let bytes = fact.to_bytes();
        let restored = match SocialFact::from_bytes(&bytes) {
            Some(restored) => restored,
            None => panic!("should deserialize"),
        };

        assert_eq!(fact, restored);
    }

    #[test]
    fn test_member_joined_serialization() {
        let fact = SocialFact::member_joined_ms(
            test_authority_id(),
            test_home_id(),
            test_context_id(),
            1234567890,
            "Alice".to_string(),
        );

        let bytes = fact.to_bytes();
        let restored = match SocialFact::from_bytes(&bytes) {
            Some(restored) => restored,
            None => panic!("should deserialize"),
        };

        assert_eq!(fact, restored);
    }

    #[test]
    fn test_storage_updated_serialization() {
        let fact = SocialFact::storage_updated_ms(
            test_home_id(),
            test_context_id(),
            1024 * 1024,      // 1 MB used
            10 * 1024 * 1024, // 10 MB total
            1234567890,
        );

        let bytes = fact.to_bytes();
        let restored = match SocialFact::from_bytes(&bytes) {
            Some(restored) => restored,
            None => panic!("should deserialize"),
        };

        assert_eq!(fact, restored);
    }

    #[test]
    fn test_domain_fact_trait() {
        let fact = SocialFact::home_created_ms(
            test_home_id(),
            test_context_id(),
            1234567890,
            test_authority_id(),
            "Test Home".to_string(),
        );

        assert_eq!(fact.type_id(), SOCIAL_FACT_TYPE_ID);
        assert_eq!(fact.context_id(), test_context_id());
    }

    #[test]
    fn test_reducer() {
        let reducer = SocialFactReducer;
        assert_eq!(reducer.handles_type(), SOCIAL_FACT_TYPE_ID);

        let fact = SocialFact::home_created_ms(
            test_home_id(),
            test_context_id(),
            1234567890,
            test_authority_id(),
            "Test Home".to_string(),
        );

        let binding = match reducer.reduce_envelope(test_context_id(), &fact.to_envelope()) {
            Some(binding) => binding,
            None => panic!("should reduce"),
        };

        assert_eq!(binding.context_id, test_context_id());
        match binding.binding_type {
            RelationalBindingType::Generic(sub_type) => {
                assert_eq!(sub_type, "home-created");
            }
            _ => panic!("expected Generic binding type"),
        }
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = SocialFactReducer;
        let context_id = test_context_id();
        let fact = SocialFact::home_created_ms(
            test_home_id(),
            context_id,
            1234567890,
            test_authority_id(),
            "Test Home".to_string(),
        );

        let envelope = fact.to_envelope();
        let binding1 = reducer.reduce_envelope(context_id, &envelope);
        let binding2 = reducer.reduce_envelope(context_id, &envelope);
        assert!(binding1.is_some());
        assert!(binding2.is_some());
        let binding1 = binding1.unwrap();
        let binding2 = binding2.unwrap();
        assert_eq!(binding1.binding_type, binding2.binding_type);
        assert_eq!(binding1.context_id, binding2.context_id);
        assert_eq!(binding1.data, binding2.data);
    }

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    #[test]
    fn test_home_id_display() {
        let id = HomeId::from_bytes([
            0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0,
        ]);
        // HomeId uses hash_id! macro which formats as "home:<full-hex>"
        assert_eq!(
            format!("{id}"),
            "home:deadbeef00000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn test_home_fact_to_datalog() {
        let home_fact = HomeFact::new(HomeId::from_bytes([1u8; 32]), test_timestamp());
        let datalog = home_fact.to_datalog();
        assert!(datalog.starts_with("home("));
        assert!(datalog.contains("10485760")); // 10 MB
    }

    #[test]
    fn test_home_config_v1_validation() {
        let valid = HomeConfigFact::v1_default(HomeId::from_bytes([1u8; 32]));
        assert!(valid.validate_v1().is_ok());

        let invalid = HomeConfigFact {
            home_id: HomeId::from_bytes([2u8; 32]),
            max_members: 10, // > 8
            neighborhood_limit: 4,
        };
        assert!(invalid.validate_v1().is_err());
    }

    #[test]
    fn test_member_fact_to_datalog() {
        let member = HomeMemberFact::new(
            AuthorityId::new_from_entropy([3u8; 32]),
            HomeId::from_bytes([4u8; 32]),
            test_timestamp(),
        );
        let datalog = member.to_datalog();
        assert!(datalog.starts_with("member("));
        assert!(datalog.contains("204800")); // 200 KB
    }

    #[test]
    fn test_moderator_capabilities() {
        let caps = ModeratorCapabilities::full();
        let cap_set = caps.to_capability_set();
        assert!(cap_set.contains("moderate:kick"));
        assert!(cap_set.contains("moderate:ban"));
        assert!(cap_set.contains("moderate:mute"));
        assert!(cap_set.contains("grant_moderator"));
    }

    #[test]
    fn test_neighborhood_fact_to_datalog() {
        let neighborhood =
            NeighborhoodFact::new(NeighborhoodId::from_bytes([5u8; 32]), test_timestamp());
        let datalog = neighborhood.to_datalog();
        assert!(datalog.starts_with("neighborhood("));
    }

    #[test]
    fn test_home_member_fact_to_datalog() {
        let member = NeighborhoodMemberFact::new(
            HomeId::from_bytes([6u8; 32]),
            NeighborhoodId::from_bytes([7u8; 32]),
            test_timestamp(),
        );
        let datalog = member.to_datalog();
        assert!(datalog.starts_with("home_member("));
        assert!(datalog.contains("1048576")); // 1 MB allocation
    }

    #[test]
    fn test_one_hop_link_ordering() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let neighborhood = NeighborhoodId::from_bytes([8u8; 32]);

        // Should order consistently regardless of input order
        let adj1 = OneHopLinkFact::new(home_a, home_b, neighborhood);
        let adj2 = OneHopLinkFact::new(home_b, home_a, neighborhood);

        assert_eq!(adj1.home_a, adj2.home_a);
        assert_eq!(adj1.home_b, adj2.home_b);
    }

    #[test]
    fn test_storage_budget_calculations() {
        let mut budget = HomeStorageBudget::new(HomeId::from_bytes([9u8; 32]));

        // Initial state
        assert_eq!(
            budget.remaining_shared_storage(),
            HomeFact::DEFAULT_STORAGE_LIMIT
        );

        // Add 8 members
        budget.member_storage_spent = 8 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION;
        // Join 4 neighborhoods
        budget.neighborhood_allocations = 4 * NeighborhoodMemberFact::DEFAULT_ALLOCATION;

        // Remaining should be 10 MB - 1.6 MB (members) - 4 MB (allocations) = ~4.4 MB
        // 10,485,760 - 1,638,400 (8 * 204,800) - 4,194,304 (4 * 1,048,576) = 4,653,056
        let expected = HomeFact::DEFAULT_STORAGE_LIMIT
            - (8 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION)
            - (4 * NeighborhoodMemberFact::DEFAULT_ALLOCATION);
        assert_eq!(budget.remaining_shared_storage(), expected);
        assert_eq!(expected, 4_653_056); // ~4.4 MB
    }

    #[test]
    fn test_access_level_ordering() {
        assert!(AccessLevel::Limited < AccessLevel::Partial);
        assert!(AccessLevel::Partial < AccessLevel::Full);
    }

    #[test]
    fn test_access_override_validation_rules() {
        let authority_id = AuthorityId::new_from_entropy([33u8; 32]);
        let home_id = HomeId::from_bytes([44u8; 32]);
        let now = test_timestamp();

        let valid_upgrade = AccessOverrideFact::new_validated(
            authority_id,
            home_id,
            AccessLevel::Limited,
            AccessLevel::Partial,
            now.clone(),
        );
        assert!(valid_upgrade.is_ok());

        let invalid = AccessOverrideFact::new_validated(
            authority_id,
            home_id,
            AccessLevel::Full,
            AccessLevel::Limited,
            now,
        );
        assert!(invalid.is_err());
    }

    #[test]
    fn test_access_level_capability_config_defaults() {
        let config = AccessLevelCapabilityConfig::default();
        assert!(config.allows(AccessLevel::Full, "moderate:kick"));
        assert!(config.allows(AccessLevel::Partial, "send_message"));
        assert!(!config.allows(AccessLevel::Limited, "send_message"));
    }
}
