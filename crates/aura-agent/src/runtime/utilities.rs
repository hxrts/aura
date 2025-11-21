//! Runtime Utilities
//!
//! Shared utilities for persistence, storage keys, and effect API helpers.

use aura_core::identifiers::{AuthorityId, ContextId, SessionId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Persistence utilities for consistent storage key generation
pub struct PersistenceUtils;

impl PersistenceUtils {
    /// Generate storage key for authority data
    pub fn authority_key(authority_id: &AuthorityId) -> String {
        format!("authority/{}", authority_id)
    }
    
    /// Generate storage key for context data
    pub fn context_key(context_id: &ContextId) -> String {
        format!("context/{}", context_id)
    }
    
    /// Generate storage key for session data
    pub fn session_key(session_id: &SessionId) -> String {
        format!("session/{}", session_id)
    }
    
    /// Generate storage key for journal data
    pub fn journal_key(authority_id: &AuthorityId, journal_type: &str) -> String {
        format!("journal/{}/{}", authority_id, journal_type)
    }
}

/// Storage key manager for consistent storage operations
pub struct StorageKeyManager {
    base_path: PathBuf,
}

impl StorageKeyManager {
    /// Create a new storage key manager
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }
    
    /// Get full path for a storage key
    pub fn path_for_key(&self, key: &str) -> PathBuf {
        self.base_path.join(key)
    }
    
    /// Get directory for a storage key
    pub fn dir_for_key(&self, key: &str) -> PathBuf {
        let mut path = self.path_for_key(key);
        path.pop();
        path
    }
    
    /// Ensure directory exists for a storage key
    pub async fn ensure_dir(&self, key: &str) -> Result<(), std::io::Error> {
        let dir = self.dir_for_key(key);
        tokio::fs::create_dir_all(dir).await
    }
}

/// Effect API helpers for common patterns
pub struct EffectApiHelpers;

impl EffectApiHelpers {
    /// Create effect context from authority context
    pub fn create_context(
        authority_id: AuthorityId,
        session_id: Option<SessionId>,
        metadata: HashMap<String, String>,
    ) -> EffectContext {
        EffectContext {
            authority_id,
            session_id,
            metadata,
        }
    }
    
    /// Extract authority from effect context
    pub fn extract_authority(context: &EffectContext) -> AuthorityId {
        context.authority_id
    }
    
    /// Extract session from effect context
    pub fn extract_session(context: &EffectContext) -> Option<SessionId> {
        context.session_id
    }
}

/// Effect context for API operations
#[derive(Debug, Clone)]
pub struct EffectContext {
    /// Authority performing the operation
    pub authority_id: AuthorityId,
    
    /// Session context (if any)
    pub session_id: Option<SessionId>,
    
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}