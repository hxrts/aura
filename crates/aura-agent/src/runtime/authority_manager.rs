//! Authority manager for runtime coordination.
//!
//! This module keeps an in-memory cache of authorities and relational contexts
//! derived from fact-based journals. Persistence hooks are intentionally no-ops
//! until the storage layer lands; callers can still exercise the API in tests.

use aura_core::{Authority, AuthorityId, AuraError, ContextId, Result};
use aura_journal::{
    authority_state::DerivedAuthority,
    fact_journal::{Journal, JournalNamespace},
};
use aura_relational::RelationalContext;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

/// Authority manager for runtime coordination
pub struct AuthorityManager {
    /// Cached authorities by ID
    authorities: HashMap<AuthorityId, Arc<dyn Authority>>,
    /// Authority journals
    authority_journals: HashMap<AuthorityId, Journal>,
    /// Relational contexts
    contexts: HashMap<ContextId, Arc<RelationalContext>>,
    /// Context journals
    context_journals: HashMap<ContextId, Journal>,
    /// Storage path for journals
    storage_path: String,
}

impl AuthorityManager {
    /// Create a new authority manager
    pub fn new(storage_path: String) -> Self {
        Self {
            authorities: HashMap::new(),
            authority_journals: HashMap::new(),
            contexts: HashMap::new(),
            context_journals: HashMap::new(),
            storage_path,
        }
    }
    
    /// Load an authority from its journal
    pub async fn load_authority(
        &mut self,
        id: AuthorityId,
    ) -> Result<Arc<dyn Authority>> {
        // Check cache first
        if let Some(authority) = self.authorities.get(&id) {
            return Ok(authority.clone());
        }

        let journal_entry = self
            .authority_journals
            .entry(id)
            .or_insert_with(|| Journal::new(JournalNamespace::Authority(id)));

        let authority = DerivedAuthority::from_journal(id, journal_entry)?;
        let authority_arc: Arc<dyn Authority> = Arc::new(authority);
        self.authorities.insert(id, authority_arc.clone());
        Ok(authority_arc)
    }

    /// Create a new authority
    pub async fn create_authority(
        &mut self,
        initial_device_key: Vec<u8>,
        threshold: u16,
    ) -> Result<AuthorityId> {
        let _ = initial_device_key;
        let _ = threshold;
        let authority_id = AuthorityId::new();
        self.authority_journals
            .insert(authority_id, Journal::new(JournalNamespace::Authority(authority_id)));
        Ok(authority_id)
    }

    /// List all authorities
    pub fn list_authorities(&self) -> Vec<AuthorityId> {
        self.authorities.keys().cloned().collect()
    }

    /// Create a new relational context
    pub async fn create_context(
        &mut self,
        participants: Vec<AuthorityId>,
        context_type: String,
    ) -> Result<ContextId> {
        let _ = context_type;
        let context_id = ContextId::new();
        let context = RelationalContext::with_id(context_id, participants);
        self.context_journals
            .insert(context.context_id, Journal::new(JournalNamespace::Context(context.context_id)));
        self.contexts
            .insert(context.context_id, Arc::new(context));
        Ok(context_id)
    }

    /// Get a relational context
    pub fn get_context(&self, id: &ContextId) -> Option<Arc<RelationalContext>> {
        self.contexts.get(id).cloned()
    }

    /// Add device to authority
    pub async fn add_device_to_authority(
        &mut self,
        authority_id: AuthorityId,
        device_public_key: Vec<u8>,
    ) -> Result<()> {
        let _ = (authority_id, device_public_key);
        Err(AuraError::internal(
            "Device management not yet implemented"
        ))
    }
}

/// Thread-safe authority manager wrapper
pub struct SharedAuthorityManager {
    inner: Arc<RwLock<AuthorityManager>>,
}

impl SharedAuthorityManager {
    /// Create a new shared authority manager
    pub fn new(storage_path: String) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AuthorityManager::new(storage_path))),
        }
    }
    
    /// Get read access to the manager
    pub async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, AuthorityManager> {
        self.inner.read().await
    }
    
    /// Get write access to the manager
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, AuthorityManager> {
        self.inner.write().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_authority_creation() {
        let mut manager = AuthorityManager::new("/tmp/test".to_string());
        
        let device_key = vec![1, 2, 3, 4]; // Mock public key
        let authority_id = manager.create_authority(device_key, 2).await.unwrap();

        assert!(!authority_id.to_bytes().is_empty());
        assert_eq!(manager.list_authorities().len(), 1);
    }
    
    #[tokio::test]
    async fn test_context_creation() {
        let mut manager = AuthorityManager::new("/tmp/test".to_string());
        
        let auth1 = AuthorityId::new();
        let auth2 = AuthorityId::new();
        
        let context_id = manager.create_context(
            vec![auth1, auth2],
            "guardian".to_string()
        ).await.unwrap();
        
        let context = manager.get_context(&context_id).unwrap();
        assert_eq!(context.context_id, context_id);
    }
}
