//! Fact Schemas for Aura's Domain Model
//!
//! This module contains specialized fact types that extend the core journal
//! fact system with domain-specific semantics.
//!
//! # Modules
//!
//! - `social`: Block and Neighborhood facts for urban social topology

pub mod social;

// Re-export primary types for convenience
pub use social::{
    // Neighborhood facts
    AdjacencyFact,
    // Block facts
    BlockConfigFact,
    BlockFact,
    // Identifiers
    BlockId,
    BlockMemberFact,
    BlockMessageMemberFact,
    // Storage
    BlockStorageBudget,
    NeighborhoodFact,
    NeighborhoodId,
    PinnedContentFact,
    ResidentFact,
    // Errors
    SocialFactError,
    StewardCapabilities,
    StewardFact,
    TraversalAllowedFact,
    // Traversal
    TraversalDepth,
    TraversalPosition,
};
