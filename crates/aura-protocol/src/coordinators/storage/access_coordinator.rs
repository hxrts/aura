//! Storage access coordination with capability verification
//!
//! Moved from aura-storage to provide Layer 4 coordination for storage access control.

use aura_core::effects::AuthorizationEffects;
use aura_core::{identifiers::DeviceId, AccountId, AuraError, Cap, ChunkId, ContentId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Coordinated storage access control manager
#[derive(Debug, Clone)]
pub struct StorageAccessCoordinator;

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
        limit: usize,
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
        Self::default()
    }

    /// Authorize a storage request using capability/Biscuit semantics.
    ///
    /// This uses effect-injected `AuthorizationEffects` so the coordinator stays stateless and
    /// runtime-agnostic. Consumers supply the capability set (Cap/Biscuit) and the effect
    /// implementation that knows how to validate it.
    pub async fn authorize(
        &self,
        auth: &dyn AuthorizationEffects,
        token: &Cap,
        request: &AccessRequest,
    ) -> Result<AccessDecision, AuraError> {
        let (operation, resource) = map_request(request);
        let authorized = auth
            .verify_capability(token, &operation, &resource)
            .await
            .map_err(|e| AuraError::permission_denied(format!("authorization failed: {e}")))?;

        if authorized {
            Ok(AccessDecision::Allow)
        } else {
            Ok(AccessDecision::Deny(format!(
                "operation {} on {} denied",
                operation, resource
            )))
        }
    }
}

impl Default for StorageAccessCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

fn map_request(request: &AccessRequest) -> (String, String) {
    let op = request.operation.to_string();
    let resource = match &request.resource {
        StorageResource::Content(id) => format!("storage:content:{id}"),
        StorageResource::Chunk(id) => format!("storage:chunk:{id}"),
        StorageResource::Namespace(account) => format!("storage:namespace:{account}"),
        StorageResource::SearchIndex => "storage:search_index".to_string(),
        StorageResource::GcSystem => "storage:gc_system".to_string(),
    };
    (op, resource)
}

// StorageCapabilityToken removed - use Biscuit tokens via effect system for authorization
// Legacy capability token type has been replaced by Biscuit-based authorization

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
