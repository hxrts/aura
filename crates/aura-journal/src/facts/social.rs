//! Social Fact Schemas - Block and Neighborhood Facts
//!
//! This module defines the fact types for Aura's urban social topology,
//! implementing blocks, neighborhoods, and their relationships as per
//! the design in `work/neighbor.md`.
//!
//! # Fact Model
//!
//! All social facts are journal facts that enable Datalog queries via Biscuit.
//! They follow the join-semilattice model where facts merge via set union.
//!
//! # v1 Constraints
//!
//! - Each user resides in exactly one block (1:1 association)
//! - Each block has a maximum of 8 residents
//! - Each block can join a maximum of 4 neighborhoods
//!
//! # Example Datalog Queries
//!
//! ```datalog,ignore
//! // Find all residents of a block
//! residents_of(Block) <- resident(Auth, Block, _, _).
//!
//! // Find blocks a user can visit from current position
//! visitable(Target) <-
//!     resident(Me, Current, _, _),
//!     adjacent(Current, Target, _),
//!     traversal_allowed(Current, Target, Cap),
//!     has_capability(Me, Cap).
//! ```

use aura_core::{
    identifiers::{AuthorityId, ChannelId, ContextId},
    time::TimeStamp,
    Hash32,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

// ============================================================================
// Block Types
// ============================================================================

/// Unique identifier for a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockId(pub [u8; 32]);

impl BlockId {
    /// Create a new random BlockId
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        let mut bytes = [0u8; 32];
        bytes[..16].copy_from_slice(uuid.as_bytes());
        // Generate second UUID for remaining 16 bytes
        let uuid2 = Uuid::new_v4();
        bytes[16..].copy_from_slice(uuid2.as_bytes());
        Self(bytes)
    }

    /// Create a BlockId from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Default for BlockId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Display first 8 bytes as hex
        for byte in &self.0[..4] {
            write!(f, "{:02x}", byte)?;
        }
        write!(f, "...")
    }
}

/// Unique identifier for a neighborhood
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NeighborhoodId(pub [u8; 32]);

impl NeighborhoodId {
    /// Create a new random NeighborhoodId
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        let mut bytes = [0u8; 32];
        bytes[..16].copy_from_slice(uuid.as_bytes());
        let uuid2 = Uuid::new_v4();
        bytes[16..].copy_from_slice(uuid2.as_bytes());
        Self(bytes)
    }

    /// Create a NeighborhoodId from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Default for NeighborhoodId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for NeighborhoodId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0[..4] {
            write!(f, "{:02x}", byte)?;
        }
        write!(f, "...")
    }
}

// ============================================================================
// Block Facts (Section 6.1)
// ============================================================================

/// Block existence and configuration fact
///
/// Corresponds to: `block(block_id, created_at, storage_limit)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockFact {
    /// Unique identifier for this block
    pub block_id: BlockId,
    /// When the block was created
    pub created_at: TimeStamp,
    /// Total storage limit in bytes (default: 10 MB)
    pub storage_limit: u64,
}

impl BlockFact {
    /// Default storage limit: 10 MB
    pub const DEFAULT_STORAGE_LIMIT: u64 = 10 * 1024 * 1024;

    /// Create a new block with default storage limit
    pub fn new(block_id: BlockId, created_at: TimeStamp) -> Self {
        Self {
            block_id,
            created_at,
            storage_limit: Self::DEFAULT_STORAGE_LIMIT,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "block(\"{}\", {}, {});",
            self.block_id,
            self.created_at.to_index_ms(),
            self.storage_limit
        )
    }
}

/// Block configuration fact
///
/// Corresponds to: `block_config(block_id, max_residents, neighborhood_limit)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockConfigFact {
    /// Block this configuration applies to
    pub block_id: BlockId,
    /// Maximum number of residents (v1: 8)
    pub max_residents: u8,
    /// Maximum number of neighborhoods to join (v1: 4)
    pub neighborhood_limit: u8,
}

impl BlockConfigFact {
    /// v1 maximum residents per block
    pub const V1_MAX_RESIDENTS: u8 = 8;
    /// v1 maximum neighborhoods per block
    pub const V1_NEIGHBORHOOD_LIMIT: u8 = 4;

    /// Create a v1-compliant configuration
    pub fn v1_default(block_id: BlockId) -> Self {
        Self {
            block_id,
            max_residents: Self::V1_MAX_RESIDENTS,
            neighborhood_limit: Self::V1_NEIGHBORHOOD_LIMIT,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "block_config(\"{}\", {}, {});",
            self.block_id, self.max_residents, self.neighborhood_limit
        )
    }

    /// Validate against v1 constraints
    pub fn validate_v1(&self) -> Result<(), SocialFactError> {
        if self.max_residents > Self::V1_MAX_RESIDENTS {
            return Err(SocialFactError::V1ConstraintViolation(format!(
                "max_residents {} exceeds v1 limit of {}",
                self.max_residents,
                Self::V1_MAX_RESIDENTS
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

/// Resident fact - user residing in a block
///
/// Corresponds to: `resident(authority_id, block_id, joined_at, storage_allocated)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResidentFact {
    /// Authority of the resident
    pub authority_id: AuthorityId,
    /// Block where the authority resides
    pub block_id: BlockId,
    /// When the authority joined the block
    pub joined_at: TimeStamp,
    /// Storage allocated by this resident (default: 200 KB)
    pub storage_allocated: u64,
}

impl ResidentFact {
    /// Default storage allocation per resident: 200 KB
    pub const DEFAULT_STORAGE_ALLOCATION: u64 = 200 * 1024;

    /// Create a new resident with default storage allocation
    pub fn new(authority_id: AuthorityId, block_id: BlockId, joined_at: TimeStamp) -> Self {
        Self {
            authority_id,
            block_id,
            joined_at,
            storage_allocated: Self::DEFAULT_STORAGE_ALLOCATION,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "resident(\"{}\", \"{}\", {}, {});",
            self.authority_id,
            self.block_id,
            self.joined_at.to_index_ms(),
            self.storage_allocated
        )
    }
}

/// Steward capability bundle
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StewardCapabilities {
    /// Can moderate content and users
    pub moderation: bool,
    /// Can pin/unpin content
    pub pin_content: bool,
    /// Can grant steward capabilities to others
    pub grant_steward: bool,
    /// Can manage channels
    pub manage_channel: bool,
    /// Custom capability strings
    pub custom: BTreeSet<String>,
}

impl Default for StewardCapabilities {
    fn default() -> Self {
        Self {
            moderation: true,
            pin_content: true,
            grant_steward: false,
            manage_channel: true,
            custom: BTreeSet::new(),
        }
    }
}

impl StewardCapabilities {
    /// Create full steward capabilities
    pub fn full() -> Self {
        Self {
            moderation: true,
            pin_content: true,
            grant_steward: true,
            manage_channel: true,
            custom: BTreeSet::new(),
        }
    }

    /// Convert to a capability string set for Biscuit
    pub fn to_capability_set(&self) -> BTreeSet<String> {
        let mut caps = BTreeSet::new();
        if self.moderation {
            caps.insert("moderation".to_string());
        }
        if self.pin_content {
            caps.insert("pin_content".to_string());
        }
        if self.grant_steward {
            caps.insert("grant_steward".to_string());
        }
        if self.manage_channel {
            caps.insert("manage_channel".to_string());
        }
        caps.extend(self.custom.clone());
        caps
    }
}

/// Steward fact - authority with elevated capabilities in a block
///
/// Corresponds to: `steward(authority_id, block_id, granted_at, capabilities)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StewardFact {
    /// Authority with steward role
    pub authority_id: AuthorityId,
    /// Block where the steward operates
    pub block_id: BlockId,
    /// When steward capabilities were granted
    pub granted_at: TimeStamp,
    /// Capability bundle for this steward
    pub capabilities: StewardCapabilities,
}

impl StewardFact {
    /// Create a new steward with default capabilities
    pub fn new(authority_id: AuthorityId, block_id: BlockId, granted_at: TimeStamp) -> Self {
        Self {
            authority_id,
            block_id,
            granted_at,
            capabilities: StewardCapabilities::default(),
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        let caps = self.capabilities.to_capability_set();
        let caps_str: Vec<_> = caps.iter().map(|s| format!("\"{}\"", s)).collect();
        format!(
            "steward(\"{}\", \"{}\", {}, [{}]);",
            self.authority_id,
            self.block_id,
            self.granted_at.to_index_ms(),
            caps_str.join(", ")
        )
    }
}

/// Block message membership fact (derived from residency)
///
/// Corresponds to: `block_message_member(authority_id, channel_id, block_id)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockMessageMemberFact {
    /// Authority with message access
    pub authority_id: AuthorityId,
    /// Channel the authority can access
    pub channel_id: ChannelId,
    /// Block this channel belongs to
    pub block_id: BlockId,
}

impl BlockMessageMemberFact {
    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "block_message_member(\"{}\", \"{}\", \"{}\");",
            self.authority_id, self.channel_id, self.block_id
        )
    }
}

/// Pinned content fact
///
/// Corresponds to: `pinned_content(content_hash, block_id, pinned_by, pinned_at, size_bytes)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedContentFact {
    /// Hash of the pinned content
    pub content_hash: Hash32,
    /// Block where content is pinned
    pub block_id: BlockId,
    /// Authority who pinned the content
    pub pinned_by: AuthorityId,
    /// When the content was pinned
    pub pinned_at: TimeStamp,
    /// Size of the pinned content in bytes
    pub size_bytes: u64,
}

impl PinnedContentFact {
    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "pinned_content(hex:{:?}, \"{}\", \"{}\", {}, {});",
            self.content_hash.0,
            self.block_id,
            self.pinned_by,
            self.pinned_at.to_index_ms(),
            self.size_bytes
        )
    }
}

// ============================================================================
// Neighborhood Facts (Section 6.2)
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
            "neighborhood(\"{}\", {});",
            self.neighborhood_id,
            self.created_at.to_index_ms()
        )
    }
}

/// Block membership in a neighborhood
///
/// Corresponds to: `block_member(block_id, neighborhood_id, joined_at, donated_storage)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockMemberFact {
    /// Block joining the neighborhood
    pub block_id: BlockId,
    /// Neighborhood being joined
    pub neighborhood_id: NeighborhoodId,
    /// When the block joined
    pub joined_at: TimeStamp,
    /// Storage donated to neighborhood infrastructure (default: 1 MB)
    pub donated_storage: u64,
}

impl BlockMemberFact {
    /// Default storage donation per neighborhood: 1 MB
    pub const DEFAULT_DONATION: u64 = 1024 * 1024;

    /// Create a new block membership with default donation
    pub fn new(block_id: BlockId, neighborhood_id: NeighborhoodId, joined_at: TimeStamp) -> Self {
        Self {
            block_id,
            neighborhood_id,
            joined_at,
            donated_storage: Self::DEFAULT_DONATION,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "block_member(\"{}\", \"{}\", {}, {});",
            self.block_id,
            self.neighborhood_id,
            self.joined_at.to_index_ms(),
            self.donated_storage
        )
    }
}

/// Adjacency relationship between blocks
///
/// Corresponds to: `adjacent(block_a, block_b, neighborhood_id)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AdjacencyFact {
    /// First block in the adjacency relationship
    pub block_a: BlockId,
    /// Second block in the adjacency relationship
    pub block_b: BlockId,
    /// Neighborhood where this adjacency exists
    pub neighborhood_id: NeighborhoodId,
}

impl AdjacencyFact {
    /// Create a new adjacency (ordered by block IDs for consistency)
    pub fn new(block_a: BlockId, block_b: BlockId, neighborhood_id: NeighborhoodId) -> Self {
        // Ensure consistent ordering
        let (a, b) = if block_a <= block_b {
            (block_a, block_b)
        } else {
            (block_b, block_a)
        };
        Self {
            block_a: a,
            block_b: b,
            neighborhood_id,
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "adjacent(\"{}\", \"{}\", \"{}\");",
            self.block_a, self.block_b, self.neighborhood_id
        )
    }
}

/// Traversal permission fact
///
/// Corresponds to: `traversal_allowed(from_block, to_block, capability_requirement)`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TraversalAllowedFact {
    /// Block to traverse from
    pub from_block: BlockId,
    /// Block to traverse to
    pub to_block: BlockId,
    /// Capability required for this traversal
    pub capability_requirement: String,
}

impl TraversalAllowedFact {
    /// Create a traversal permission
    pub fn new(
        from_block: BlockId,
        to_block: BlockId,
        capability_requirement: impl Into<String>,
    ) -> Self {
        Self {
            from_block,
            to_block,
            capability_requirement: capability_requirement.into(),
        }
    }

    /// Convert to Datalog fact string
    pub fn to_datalog(&self) -> String {
        format!(
            "traversal_allowed(\"{}\", \"{}\", \"{}\");",
            self.from_block, self.to_block, self.capability_requirement
        )
    }
}

// ============================================================================
// Traversal Position (Section 7)
// ============================================================================

/// Traversal depth in a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TraversalDepth {
    /// Can see frontage, no interior access
    Street,
    /// Can see public block info, limited interaction
    Frontage,
    /// Full resident-level access
    Interior,
}

/// Current position in neighborhood traversal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraversalPosition {
    /// Current neighborhood (None = "on the street")
    pub neighborhood: Option<NeighborhoodId>,
    /// Current block (None = between blocks)
    pub block: Option<BlockId>,
    /// Depth of access
    pub depth: TraversalDepth,
    /// Relational context containing capabilities
    pub context_id: ContextId,
    /// When this position was entered (for expiration tracking)
    pub entered_at: TimeStamp,
}

// ============================================================================
// Storage Budget (Section 8)
// ============================================================================

/// Block storage budget tracking
///
/// Tracks spent counters as facts; limits are derived at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockStorageBudget {
    /// Block this budget tracks
    pub block_id: BlockId,
    /// Current resident storage spent (sum of allocations)
    pub resident_storage_spent: u64,
    /// Current pinned storage spent
    pub pinned_storage_spent: u64,
    /// Current neighborhood donations (n * 1 MB)
    pub neighborhood_donations: u64,
}

impl BlockStorageBudget {
    /// Create a new empty budget
    pub fn new(block_id: BlockId) -> Self {
        Self {
            block_id,
            resident_storage_spent: 0,
            pinned_storage_spent: 0,
            neighborhood_donations: 0,
        }
    }

    /// Calculate remaining public-good space
    ///
    /// Formula: Total (10 MB) - Resident - Neighborhood Donations - Pinned
    pub fn remaining_public_good_space(&self) -> u64 {
        let total = BlockFact::DEFAULT_STORAGE_LIMIT;
        let spent =
            self.resident_storage_spent + self.neighborhood_donations + self.pinned_storage_spent;
        total.saturating_sub(spent)
    }

    /// Derive resident storage limit (8 * 200 KB = 1.6 MB)
    pub fn resident_storage_limit(&self) -> u64 {
        BlockConfigFact::V1_MAX_RESIDENTS as u64 * ResidentFact::DEFAULT_STORAGE_ALLOCATION
    }

    /// Derive pinned storage limit based on neighborhood count
    pub fn pinned_storage_limit(&self) -> u64 {
        let total = BlockFact::DEFAULT_STORAGE_LIMIT;
        let resident_limit = self.resident_storage_limit();
        total.saturating_sub(resident_limit + self.neighborhood_donations)
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
                write!(f, "v1 constraint violation: {}", msg)
            }
            SocialFactError::InvalidFact(msg) => {
                write!(f, "invalid fact: {}", msg)
            }
            SocialFactError::StorageLimitExceeded {
                budget_type,
                current,
                limit,
            } => {
                write!(
                    f,
                    "{} storage limit exceeded: {} / {} bytes",
                    budget_type, current, limit
                )
            }
        }
    }
}

impl std::error::Error for SocialFactError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    #[test]
    fn test_block_id_display() {
        let id = BlockId::from_bytes([
            0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0,
        ]);
        assert_eq!(format!("{}", id), "deadbeef...");
    }

    #[test]
    fn test_block_fact_to_datalog() {
        let block = BlockFact::new(BlockId::from_bytes([1u8; 32]), test_timestamp());
        let datalog = block.to_datalog();
        assert!(datalog.starts_with("block("));
        assert!(datalog.contains("10485760")); // 10 MB
    }

    #[test]
    fn test_block_config_v1_validation() {
        let valid = BlockConfigFact::v1_default(BlockId::new());
        assert!(valid.validate_v1().is_ok());

        let invalid = BlockConfigFact {
            block_id: BlockId::new(),
            max_residents: 10, // > 8
            neighborhood_limit: 4,
        };
        assert!(invalid.validate_v1().is_err());
    }

    #[test]
    fn test_resident_fact_to_datalog() {
        let resident = ResidentFact::new(AuthorityId::new(), BlockId::new(), test_timestamp());
        let datalog = resident.to_datalog();
        assert!(datalog.starts_with("resident("));
        assert!(datalog.contains("204800")); // 200 KB
    }

    #[test]
    fn test_steward_capabilities() {
        let caps = StewardCapabilities::full();
        let cap_set = caps.to_capability_set();
        assert!(cap_set.contains("moderation"));
        assert!(cap_set.contains("grant_steward"));
    }

    #[test]
    fn test_neighborhood_fact_to_datalog() {
        let neighborhood = NeighborhoodFact::new(NeighborhoodId::new(), test_timestamp());
        let datalog = neighborhood.to_datalog();
        assert!(datalog.starts_with("neighborhood("));
    }

    #[test]
    fn test_block_member_fact_to_datalog() {
        let member = BlockMemberFact::new(BlockId::new(), NeighborhoodId::new(), test_timestamp());
        let datalog = member.to_datalog();
        assert!(datalog.starts_with("block_member("));
        assert!(datalog.contains("1048576")); // 1 MB donation
    }

    #[test]
    fn test_adjacency_ordering() {
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);
        let neighborhood = NeighborhoodId::new();

        // Should order consistently regardless of input order
        let adj1 = AdjacencyFact::new(block_a, block_b, neighborhood);
        let adj2 = AdjacencyFact::new(block_b, block_a, neighborhood);

        assert_eq!(adj1.block_a, adj2.block_a);
        assert_eq!(adj1.block_b, adj2.block_b);
    }

    #[test]
    fn test_storage_budget_calculations() {
        let mut budget = BlockStorageBudget::new(BlockId::new());

        // Initial state
        assert_eq!(
            budget.remaining_public_good_space(),
            BlockFact::DEFAULT_STORAGE_LIMIT
        );

        // Add 8 residents
        budget.resident_storage_spent = 8 * ResidentFact::DEFAULT_STORAGE_ALLOCATION;
        // Join 4 neighborhoods
        budget.neighborhood_donations = 4 * BlockMemberFact::DEFAULT_DONATION;

        // Remaining should be 10 MB - 1.6 MB (residents) - 4 MB (donations) = ~4.4 MB
        // 10,485,760 - 1,638,400 (8 * 204,800) - 4,194,304 (4 * 1,048,576) = 4,653,056
        let expected = BlockFact::DEFAULT_STORAGE_LIMIT
            - (8 * ResidentFact::DEFAULT_STORAGE_ALLOCATION)
            - (4 * BlockMemberFact::DEFAULT_DONATION);
        assert_eq!(budget.remaining_public_good_space(), expected);
        assert_eq!(expected, 4_653_056); // ~4.4 MB
    }

    #[test]
    fn test_traversal_depth_ordering() {
        assert!(TraversalDepth::Street < TraversalDepth::Frontage);
        assert!(TraversalDepth::Frontage < TraversalDepth::Interior);
    }
}
