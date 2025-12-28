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
//! // Create a block created fact
//! let fact = SocialFact::block_created(block_id, context_id, timestamp, creator_id, "My Block");
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

// ============================================================================
// Social Fact Schemas - Block and Neighborhood Facts
// ============================================================================

/// Unique identifier for a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockId(pub [u8; 32]);

impl BlockId {
    fn derive_bytes(label: &[u8]) -> [u8; 32] {
        let mut hasher = aura_core::hash::hasher();
        hasher.update(b"AURA_BLOCK_ID");
        hasher.update(label);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        bytes
    }
    /// Create a new random BlockId
    pub fn new() -> Self {
        Self::from_bytes(Self::derive_bytes(b"block-id"))
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
    fn derive_bytes(label: &[u8]) -> [u8; 32] {
        let mut hasher = aura_core::hash::hasher();
        hasher.update(b"AURA_NEIGHBORHOOD_ID");
        hasher.update(label);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        bytes
    }
    /// Create a new random NeighborhoodId
    pub fn new() -> Self {
        Self::from_bytes(Self::derive_bytes(b"neighborhood-id"))
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
// Block Facts
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
// Traversal Position
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
// Storage Budget
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
/// including blocks, residents, stewards, and neighborhoods.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "social", schema_version = 1, context = "context_id")]
pub enum SocialFact {
    /// Block created
    BlockCreated {
        /// Unique block identifier
        block_id: BlockId,
        /// Relational context for this block
        context_id: ContextId,
        /// When the block was created
        created_at: PhysicalTime,
        /// Authority that created the block
        creator_id: AuthorityId,
        /// Human-readable block name
        name: String,
        /// Storage limit in bytes (default: 10 MB)
        storage_limit: u64,
    },
    /// Block deleted/archived
    BlockDeleted {
        /// Block being deleted
        block_id: BlockId,
        /// Relational context for this block
        context_id: ContextId,
        /// When the block was deleted
        deleted_at: PhysicalTime,
        /// Authority that deleted the block
        actor_id: AuthorityId,
    },
    /// Resident joined a block
    ResidentJoined {
        /// Authority joining the block
        authority_id: AuthorityId,
        /// Block being joined
        block_id: BlockId,
        /// Relational context
        context_id: ContextId,
        /// When the resident joined
        joined_at: PhysicalTime,
        /// Human-readable name for the resident
        name: String,
        /// Storage allocated in bytes (default: 200 KB)
        storage_allocated: u64,
    },
    /// Resident left a block
    ResidentLeft {
        /// Authority leaving the block
        authority_id: AuthorityId,
        /// Block being left
        block_id: BlockId,
        /// Relational context
        context_id: ContextId,
        /// When the resident left
        left_at: PhysicalTime,
    },
    /// Steward granted capabilities in a block
    StewardGranted {
        /// Authority being granted steward role
        authority_id: AuthorityId,
        /// Block where steward operates
        block_id: BlockId,
        /// Relational context
        context_id: ContextId,
        /// When steward was granted
        granted_at: PhysicalTime,
        /// Authority granting the steward role
        grantor_id: AuthorityId,
        /// Capability strings granted
        capabilities: Vec<String>,
    },
    /// Steward revoked from a block
    StewardRevoked {
        /// Authority losing steward role
        authority_id: AuthorityId,
        /// Block where steward was revoked
        block_id: BlockId,
        /// Relational context
        context_id: ContextId,
        /// When steward was revoked
        revoked_at: PhysicalTime,
        /// Authority revoking the steward role
        revoker_id: AuthorityId,
    },
    /// Block storage updated
    StorageUpdated {
        /// Block whose storage changed
        block_id: BlockId,
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
    /// Block joined a neighborhood
    BlockJoinedNeighborhood {
        /// Block joining the neighborhood
        block_id: BlockId,
        /// Neighborhood being joined
        neighborhood_id: NeighborhoodId,
        /// Relational context
        context_id: ContextId,
        /// When the block joined
        joined_at: PhysicalTime,
    },
    /// Block left a neighborhood
    BlockLeftNeighborhood {
        /// Block leaving the neighborhood
        block_id: BlockId,
        /// Neighborhood being left
        neighborhood_id: NeighborhoodId,
        /// Relational context
        context_id: ContextId,
        /// When the block left
        left_at: PhysicalTime,
    },
}

impl SocialFact {
    /// Default storage limit for blocks: 10 MB
    pub const DEFAULT_BLOCK_STORAGE_LIMIT: u64 = 10 * 1024 * 1024;

    /// Default storage allocation for residents: 200 KB
    pub const DEFAULT_RESIDENT_STORAGE: u64 = 200 * 1024;

    /// Get the timestamp in milliseconds
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            SocialFact::BlockCreated { created_at, .. } => created_at.ts_ms,
            SocialFact::BlockDeleted { deleted_at, .. } => deleted_at.ts_ms,
            SocialFact::ResidentJoined { joined_at, .. } => joined_at.ts_ms,
            SocialFact::ResidentLeft { left_at, .. } => left_at.ts_ms,
            SocialFact::StewardGranted { granted_at, .. } => granted_at.ts_ms,
            SocialFact::StewardRevoked { revoked_at, .. } => revoked_at.ts_ms,
            SocialFact::StorageUpdated { updated_at, .. } => updated_at.ts_ms,
            SocialFact::NeighborhoodCreated { created_at, .. } => created_at.ts_ms,
            SocialFact::BlockJoinedNeighborhood { joined_at, .. } => joined_at.ts_ms,
            SocialFact::BlockLeftNeighborhood { left_at, .. } => left_at.ts_ms,
        }
    }

    /// Validate that this fact can be reduced under the provided context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id() == context_id
    }

    /// Derive the relational binding subtype and key data for this fact.
    pub fn binding_key(&self) -> SocialFactKey {
        match self {
            SocialFact::BlockCreated { block_id, .. } => SocialFactKey {
                sub_type: "block-created",
                data: block_id.0.to_vec(),
            },
            SocialFact::BlockDeleted { block_id, .. } => SocialFactKey {
                sub_type: "block-deleted",
                data: block_id.0.to_vec(),
            },
            SocialFact::ResidentJoined { authority_id, .. } => SocialFactKey {
                sub_type: "resident-joined",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::ResidentLeft { authority_id, .. } => SocialFactKey {
                sub_type: "resident-left",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::StewardGranted { authority_id, .. } => SocialFactKey {
                sub_type: "steward-granted",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::StewardRevoked { authority_id, .. } => SocialFactKey {
                sub_type: "steward-revoked",
                data: authority_id.to_string().into_bytes(),
            },
            SocialFact::StorageUpdated { block_id, .. } => SocialFactKey {
                sub_type: "storage-updated",
                data: block_id.0.to_vec(),
            },
            SocialFact::NeighborhoodCreated {
                neighborhood_id, ..
            } => SocialFactKey {
                sub_type: "neighborhood-created",
                data: neighborhood_id.0.to_vec(),
            },
            SocialFact::BlockJoinedNeighborhood {
                block_id,
                neighborhood_id,
                ..
            } => {
                let mut data = block_id.0.to_vec();
                data.extend_from_slice(&neighborhood_id.0);
                SocialFactKey {
                    sub_type: "block-joined-neighborhood",
                    data,
                }
            }
            SocialFact::BlockLeftNeighborhood {
                block_id,
                neighborhood_id,
                ..
            } => {
                let mut data = block_id.0.to_vec();
                data.extend_from_slice(&neighborhood_id.0);
                SocialFactKey {
                    sub_type: "block-left-neighborhood",
                    data,
                }
            }
        }
    }

    /// Create a BlockCreated fact with millisecond timestamp
    pub fn block_created_ms(
        block_id: BlockId,
        context_id: ContextId,
        created_at_ms: u64,
        creator_id: AuthorityId,
        name: String,
    ) -> Self {
        Self::BlockCreated {
            block_id,
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

    /// Create a ResidentJoined fact with millisecond timestamp
    pub fn resident_joined_ms(
        authority_id: AuthorityId,
        block_id: BlockId,
        context_id: ContextId,
        joined_at_ms: u64,
        name: String,
    ) -> Self {
        Self::ResidentJoined {
            authority_id,
            block_id,
            context_id,
            joined_at: PhysicalTime {
                ts_ms: joined_at_ms,
                uncertainty: None,
            },
            name,
            storage_allocated: Self::DEFAULT_RESIDENT_STORAGE,
        }
    }

    /// Create a ResidentLeft fact with millisecond timestamp
    pub fn resident_left_ms(
        authority_id: AuthorityId,
        block_id: BlockId,
        context_id: ContextId,
        left_at_ms: u64,
    ) -> Self {
        Self::ResidentLeft {
            authority_id,
            block_id,
            context_id,
            left_at: PhysicalTime {
                ts_ms: left_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a StorageUpdated fact with millisecond timestamp
    pub fn storage_updated_ms(
        block_id: BlockId,
        context_id: ContextId,
        used_bytes: u64,
        total_bytes: u64,
        updated_at_ms: u64,
    ) -> Self {
        Self::StorageUpdated {
            block_id,
            context_id,
            used_bytes,
            total_bytes,
            updated_at: PhysicalTime {
                ts_ms: updated_at_ms,
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

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != SOCIAL_FACT_TYPE_ID {
            return None;
        }

        let fact = SocialFact::from_bytes(binding_data)?;

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

    fn test_block_id() -> BlockId {
        BlockId::from_bytes([1u8; 32])
    }

    fn test_authority_id() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    #[test]
    fn test_block_created_serialization() {
        let fact = SocialFact::block_created_ms(
            test_block_id(),
            test_context_id(),
            1234567890,
            test_authority_id(),
            "Test Block".to_string(),
        );

        let bytes = fact.to_bytes();
        let restored = match SocialFact::from_bytes(&bytes) {
            Some(restored) => restored,
            None => panic!("should deserialize"),
        };

        assert_eq!(fact, restored);
    }

    #[test]
    fn test_resident_joined_serialization() {
        let fact = SocialFact::resident_joined_ms(
            test_authority_id(),
            test_block_id(),
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
            test_block_id(),
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
        let fact = SocialFact::block_created_ms(
            test_block_id(),
            test_context_id(),
            1234567890,
            test_authority_id(),
            "Test Block".to_string(),
        );

        assert_eq!(fact.type_id(), SOCIAL_FACT_TYPE_ID);
        assert_eq!(fact.context_id(), test_context_id());
    }

    #[test]
    fn test_reducer() {
        let reducer = SocialFactReducer;
        assert_eq!(reducer.handles_type(), SOCIAL_FACT_TYPE_ID);

        let fact = SocialFact::block_created_ms(
            test_block_id(),
            test_context_id(),
            1234567890,
            test_authority_id(),
            "Test Block".to_string(),
        );

        let binding = match reducer.reduce(test_context_id(), SOCIAL_FACT_TYPE_ID, &fact.to_bytes())
        {
            Some(binding) => binding,
            None => panic!("should reduce"),
        };

        assert_eq!(binding.context_id, test_context_id());
        match binding.binding_type {
            RelationalBindingType::Generic(sub_type) => {
                assert_eq!(sub_type, "block-created");
            }
            _ => panic!("expected Generic binding type"),
        }
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = SocialFactReducer;
        let context_id = test_context_id();
        let fact = SocialFact::block_created_ms(
            test_block_id(),
            context_id,
            1234567890,
            test_authority_id(),
            "Test Block".to_string(),
        );

        let bytes = fact.to_bytes();
        let binding1 = reducer.reduce(context_id, SOCIAL_FACT_TYPE_ID, &bytes);
        let binding2 = reducer.reduce(context_id, SOCIAL_FACT_TYPE_ID, &bytes);
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
    fn test_block_id_display() {
        let id = BlockId::from_bytes([
            0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
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
        let valid = BlockConfigFact::v1_default(BlockId::from_bytes([1u8; 32]));
        assert!(valid.validate_v1().is_ok());

        let invalid = BlockConfigFact {
            block_id: BlockId::from_bytes([2u8; 32]),
            max_residents: 10, // > 8
            neighborhood_limit: 4,
        };
        assert!(invalid.validate_v1().is_err());
    }

    #[test]
    fn test_resident_fact_to_datalog() {
        let resident = ResidentFact::new(
            AuthorityId::new_from_entropy([3u8; 32]),
            BlockId::from_bytes([4u8; 32]),
            test_timestamp(),
        );
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
        let neighborhood =
            NeighborhoodFact::new(NeighborhoodId::from_bytes([5u8; 32]), test_timestamp());
        let datalog = neighborhood.to_datalog();
        assert!(datalog.starts_with("neighborhood("));
    }

    #[test]
    fn test_block_member_fact_to_datalog() {
        let member = BlockMemberFact::new(
            BlockId::from_bytes([6u8; 32]),
            NeighborhoodId::from_bytes([7u8; 32]),
            test_timestamp(),
        );
        let datalog = member.to_datalog();
        assert!(datalog.starts_with("block_member("));
        assert!(datalog.contains("1048576")); // 1 MB donation
    }

    #[test]
    fn test_adjacency_ordering() {
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);
        let neighborhood = NeighborhoodId::from_bytes([8u8; 32]);

        // Should order consistently regardless of input order
        let adj1 = AdjacencyFact::new(block_a, block_b, neighborhood);
        let adj2 = AdjacencyFact::new(block_b, block_a, neighborhood);

        assert_eq!(adj1.block_a, adj2.block_a);
        assert_eq!(adj1.block_b, adj2.block_b);
    }

    #[test]
    fn test_storage_budget_calculations() {
        let mut budget = BlockStorageBudget::new(BlockId::from_bytes([9u8; 32]));

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
