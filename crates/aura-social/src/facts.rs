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

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::PhysicalTime;
use aura_journal::facts::social::{BlockId, NeighborhoodId};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use serde::{Deserialize, Serialize};

/// Type identifier for social facts
pub const SOCIAL_FACT_TYPE_ID: &str = "social";
/// Schema version for social fact serialization
pub const SOCIAL_FACT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionedSocialFact {
    schema_version: u32,
    fact: SocialFact,
}

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
            SocialFact::NeighborhoodCreated { neighborhood_id, .. } => SocialFactKey {
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

impl DomainFact for SocialFact {
    fn type_id(&self) -> &'static str {
        SOCIAL_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        match self {
            SocialFact::BlockCreated { context_id, .. } => *context_id,
            SocialFact::BlockDeleted { context_id, .. } => *context_id,
            SocialFact::ResidentJoined { context_id, .. } => *context_id,
            SocialFact::ResidentLeft { context_id, .. } => *context_id,
            SocialFact::StewardGranted { context_id, .. } => *context_id,
            SocialFact::StewardRevoked { context_id, .. } => *context_id,
            SocialFact::StorageUpdated { context_id, .. } => *context_id,
            SocialFact::NeighborhoodCreated { context_id, .. } => *context_id,
            SocialFact::BlockJoinedNeighborhood { context_id, .. } => *context_id,
            SocialFact::BlockLeftNeighborhood { context_id, .. } => *context_id,
        }
    }

    #[allow(clippy::expect_used)] // DomainFact::to_bytes is infallible by trait signature.
    fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(&VersionedSocialFact {
            schema_version: SOCIAL_FACT_SCHEMA_VERSION,
            fact: self.clone(),
        })
        .expect("SocialFact must serialize")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        if let Ok(versioned) = bincode::deserialize::<VersionedSocialFact>(bytes) {
            if versioned.schema_version == SOCIAL_FACT_SCHEMA_VERSION {
                return Some(versioned.fact);
            }
        }
        if let Ok(versioned) = serde_json::from_slice::<VersionedSocialFact>(bytes) {
            if versioned.schema_version == SOCIAL_FACT_SCHEMA_VERSION {
                return Some(versioned.fact);
            }
        }
        serde_json::from_slice(bytes).ok()
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
}
