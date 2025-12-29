//! Storage access coordination with capability verification
//!
//! Moved from aura-storage to provide Layer 4 coordination for storage access control.

use aura_core::{identifiers::DeviceId, AccountId, ChunkId, ContentId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Coordinated storage access control manager
///
/// Note: authorization here still uses simplified capability strings; Biscuit token integration should replace it when available.
#[derive(Debug, Clone)]
pub struct StorageAccessCoordinator {
    // Unit struct - internal state pending Biscuit authorization integration
}

/// Unified access request for storage operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRequest {
    /// Requesting device
    pub device_id: DeviceId,
    /// Requested operation
    pub operation: StorageOperation,
    /// Target resource
    pub resource: StorageResource,
}

/// Storage operations requiring capability checks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageOperation {
    /// Read content or chunk
    Read,
    /// Write new content or chunk
    Write,
    /// Delete content or chunk
    Delete,
    /// Search content
    Search {
        /// Search query terms
        terms: Vec<String>,
        /// Maximum results
        limit: u32,
    },
    /// Garbage collection proposal
    GarbageCollect {
        /// Proposed snapshot point
        snapshot_root: ChunkId,
    },
}

impl fmt::Display for StorageOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageOperation::Read => write!(f, "read"),
            StorageOperation::Write => write!(f, "write"),
            StorageOperation::Delete => write!(f, "delete"),
            StorageOperation::Search { .. } => write!(f, "search"),
            StorageOperation::GarbageCollect { .. } => write!(f, "garbage_collect"),
        }
    }
}

/// Storage resources requiring access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageResource {
    /// Specific content item
    Content(ContentId),
    /// Specific chunk
    Chunk(ChunkId),
    /// Content namespace for account
    Namespace(AccountId),
    /// Global search index
    SearchIndex,
    /// Garbage collection system
    GcSystem,
}

/// Access control decision
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access granted
    Allow,
    /// Access denied with reason
    Deny(String),
    /// Access requires additional verification
    RequiresVerification(String),
}

impl StorageAccessCoordinator {
    /// Create new storage access coordinator
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for StorageAccessCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Resource access constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceConstraint {
    /// Restrict to specific content namespace
    NamespaceOnly(AccountId),
    /// Restrict to content owned by device
    OwnContentOnly,
    /// Rate limiting
    RateLimit {
        /// Operations per time window
        operations_per_window: u32,
        /// Time window in seconds
        window_seconds: u32,
    },
    /// Size limits
    SizeLimit {
        /// Maximum content size in bytes
        max_content_size: u64,
        /// Maximum total storage used
        max_total_size: u64,
    },
}
