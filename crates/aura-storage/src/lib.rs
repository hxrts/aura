//! Aura Storage Layer
//!
//! This crate provides capability-based storage with choreographic search protocols
//! for the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements storage application layer:
//! - `access_control/` - Capability-based access control for storage resources
//! - `search/` - Choreographic search protocols (G_search)
//! - `garbage_collection/` - Coordinated GC protocols (G_gc)  
//! - `content/` - Content addressing and chunk management
//!
//! # Design Principles
//!
//! - Uses capability-based access control for all storage operations
//! - Uses stateless effect system for distributed coordination
//! - Provides privacy-preserving search with leakage budget tracking
//! - Implements coordinated garbage collection with snapshot safety

#![allow(missing_docs)]
#![forbid(unsafe_code)]

/// Capability-based access control
pub mod access_control;

/// Choreographic search protocols (G_search)
pub mod search;

/// Coordinated garbage collection (G_gc)  
pub mod garbage_collection;

/// Content addressing and chunk management
pub mod content;

/// Storage-specific errors
pub mod error;

// Re-export core types
pub use aura_core::{AccountId, AuraError, AuraResult, Cap, ChunkId, ContentId, DeviceId, Journal};

// Re-export WoT types for capabilities
pub use aura_wot::{
    Capability, CapabilityEvaluator, CapabilityToken, StoragePermission, TrustLevel,
};

// Re-export protocol effect system
pub use aura_protocol::AuraEffectSystem;

// Re-export main APIs
pub use access_control::StorageAccessControl;
pub use content::{ChunkStore, ContentAddressing, ContentStore};
pub use garbage_collection::{GcChoreography, GcProposal, GcRole};
pub use search::{SearchChoreography, SearchQuery, SearchResults, SearchRole};
