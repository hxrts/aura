//! Authority manager for runtime coordination.
//!
//! This module keeps an in-memory cache of authorities and relational contexts
//! derived from fact-based journals. Persistence hooks are intentionally no-ops
//! until the storage layer lands; callers can still exercise the API in tests.

use aura_core::{AuraError, Authority, AuthorityId, ContextId, Result};
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
    pub async fn load_authority(&mut self, id: AuthorityId) -> Result<Arc<dyn Authority>> {
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
    ///
    /// If initial_device_key is empty, creates an authority with no devices.
    /// The threshold parameter is only applied if an initial device is provided.
    pub async fn create_authority(
        &mut self,
        initial_device_key: Vec<u8>,
        threshold: u16,
    ) -> Result<AuthorityId> {
        let authority_id = AuthorityId::new();
        let mut journal = Journal::new(JournalNamespace::Authority(authority_id));

        // Add initial device if provided
        if !initial_device_key.is_empty() {
            use aura_core::Hash32;
            use aura_journal::fact::{AttestedOp, Fact, FactContent, TreeOpKind};

            // Create the tree operation to add initial leaf
            let tree_op = TreeOpKind::AddLeaf {
                public_key: initial_device_key.clone(),
            };

            // Create commitment hash
            use aura_core::hash;
            let mut hasher = hash::hasher();
            hasher.update(b"add_initial_leaf_commitment");
            hasher.update(&authority_id.to_bytes());
            let commitment = hasher.finalize();

            // Create attested operation
            let attested_op = AttestedOp {
                tree_op,
                parent_commitment: Hash32::new([0; 32]),
                new_commitment: Hash32::new(commitment),
                witness_threshold: 1,
                signature: vec![],
            };

            // Create fact
            let fact = Fact {
                fact_id: aura_journal::fact_journal::FactId::new(),
                content: FactContent::AttestedOp(attested_op),
            };

            // Add fact to journal
            journal.add_fact(fact)?;

            // Set threshold if specified (only when we have an initial device)
            if threshold > 0 {
                let tree_op = TreeOpKind::UpdatePolicy { threshold };

                let mut hasher = hash::hasher();
                hasher.update(b"update_initial_threshold_commitment");
                hasher.update(&authority_id.to_bytes());
                hasher.update(&threshold.to_le_bytes());
                let commitment = hasher.finalize();

                let attested_op = AttestedOp {
                    tree_op,
                    parent_commitment: Hash32::new([0; 32]),
                    new_commitment: Hash32::new(commitment),
                    witness_threshold: 1,
                    signature: vec![],
                };

                let fact = Fact {
                    fact_id: aura_journal::fact_journal::FactId::new(),
                    content: FactContent::AttestedOp(attested_op),
                };

                journal.add_fact(fact)?;
            }
        }
        // If initial_device_key is empty, the threshold parameter is ignored
        // and the authority is created with no devices and default threshold

        self.authority_journals.insert(authority_id, journal);
        Ok(authority_id)
    }

    /// List all authorities
    pub fn list_authorities(&self) -> Vec<AuthorityId> {
        self.authority_journals.keys().cloned().collect()
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
        self.context_journals.insert(
            context.context_id,
            Journal::new(JournalNamespace::Context(context.context_id)),
        );
        self.contexts.insert(context.context_id, Arc::new(context));
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
        // Get the authority journal for this authority
        let journal = self
            .authority_journals
            .get_mut(&authority_id)
            .ok_or_else(|| AuraError::not_found(format!("Authority {} not found", authority_id)))?;

        // Create an attested operation to add a leaf (device) to the tree
        use aura_core::Hash32;
        use aura_journal::fact::{AttestedOp, Fact, FactContent, TreeOpKind};

        // Create the tree operation
        let tree_op = TreeOpKind::AddLeaf {
            public_key: device_public_key,
        };

        // Create commitment hash (simplified for now)
        use aura_core::hash;
        let mut hasher = hash::hasher();
        hasher.update(b"add_leaf_commitment");
        hasher.update(&authority_id.to_bytes());
        let commitment = hasher.finalize();

        // Create attested operation
        let attested_op = AttestedOp {
            tree_op,
            parent_commitment: Hash32::new([0; 32]), // Zero commitment as placeholder for parent
            new_commitment: Hash32::new(commitment),
            witness_threshold: 1, // Minimum threshold for now
            signature: vec![],    // Empty signature for now
        };

        // Create fact
        let fact = Fact {
            fact_id: aura_journal::fact_journal::FactId::new(),
            content: FactContent::AttestedOp(attested_op),
        };

        // Add fact to journal
        journal.add_fact(fact)?;

        // Invalidate cached authority to force reload
        self.authorities.remove(&authority_id);

        Ok(())
    }

    /// Remove device from authority
    pub async fn remove_device_from_authority(
        &mut self,
        authority_id: AuthorityId,
        leaf_index: u32,
    ) -> Result<()> {
        // Get the authority journal for this authority
        let journal = self
            .authority_journals
            .get_mut(&authority_id)
            .ok_or_else(|| AuraError::not_found(format!("Authority {} not found", authority_id)))?;

        // Create an attested operation to remove a leaf (device) from the tree
        use aura_core::Hash32;
        use aura_journal::fact::{AttestedOp, Fact, FactContent, TreeOpKind};

        // Create the tree operation
        let tree_op = TreeOpKind::RemoveLeaf { leaf_index };

        // Create commitment hash
        use aura_core::hash;
        let mut hasher = hash::hasher();
        hasher.update(b"remove_leaf_commitment");
        hasher.update(&authority_id.to_bytes());
        hasher.update(&leaf_index.to_le_bytes());
        let commitment = hasher.finalize();

        // Create attested operation
        let attested_op = AttestedOp {
            tree_op,
            parent_commitment: Hash32::new([0; 32]), // Zero commitment as placeholder for parent
            new_commitment: Hash32::new(commitment),
            witness_threshold: 1, // Minimum threshold for now
            signature: vec![],    // Empty signature for now
        };

        // Create fact
        let fact = Fact {
            fact_id: aura_journal::fact_journal::FactId::new(),
            content: FactContent::AttestedOp(attested_op),
        };

        // Add fact to journal
        journal.add_fact(fact)?;

        // Invalidate cached authority to force reload
        self.authorities.remove(&authority_id);

        Ok(())
    }

    /// Update authority threshold policy
    pub async fn update_authority_threshold(
        &mut self,
        authority_id: AuthorityId,
        new_threshold: u16,
    ) -> Result<()> {
        // Get the authority journal for this authority (immutably first for validation)
        let journal = self
            .authority_journals
            .get(&authority_id)
            .ok_or_else(|| AuraError::not_found(format!("Authority {} not found", authority_id)))?;

        // Validate against current state by reducing the journal
        use aura_journal::authority_state::reduce_authority_state;
        let current_state = reduce_authority_state(authority_id, journal)?;

        // Validate the threshold operation
        if new_threshold == 0 {
            return Err(AuraError::invalid("Threshold cannot be zero".to_string()));
        }

        let active_leaves = current_state.tree_state.active_leaf_count();
        if new_threshold as usize > active_leaves {
            return Err(AuraError::invalid(format!(
                "Threshold {} exceeds number of active leaves {}",
                new_threshold, active_leaves
            )));
        }

        // Now get mutable reference to add the fact
        let journal = self
            .authority_journals
            .get_mut(&authority_id)
            .ok_or_else(|| AuraError::not_found(format!("Authority {} not found", authority_id)))?;

        // Create an attested operation to update the threshold policy
        use aura_core::Hash32;
        use aura_journal::fact::{AttestedOp, Fact, FactContent, TreeOpKind};

        // Create the tree operation
        let tree_op = TreeOpKind::UpdatePolicy {
            threshold: new_threshold,
        };

        // Create commitment hash
        use aura_core::hash;
        let mut hasher = hash::hasher();
        hasher.update(b"update_threshold_commitment");
        hasher.update(&authority_id.to_bytes());
        hasher.update(&new_threshold.to_le_bytes());
        let commitment = hasher.finalize();

        // Create attested operation
        let attested_op = AttestedOp {
            tree_op,
            parent_commitment: Hash32::new([0; 32]), // Zero commitment as placeholder for parent
            new_commitment: Hash32::new(commitment),
            witness_threshold: 1, // Minimum threshold for now
            signature: vec![],    // Empty signature for now
        };

        // Create fact
        let fact = Fact {
            fact_id: aura_journal::fact_journal::FactId::new(),
            content: FactContent::AttestedOp(attested_op),
        };

        // Add fact to journal
        journal.add_fact(fact)?;

        // Invalidate cached authority to force reload
        self.authorities.remove(&authority_id);

        Ok(())
    }

    /// Rotate authority epoch (invalidates old shares)
    pub async fn rotate_authority_epoch(&mut self, authority_id: AuthorityId) -> Result<()> {
        // Get the authority journal for this authority
        let journal = self
            .authority_journals
            .get_mut(&authority_id)
            .ok_or_else(|| AuraError::not_found(format!("Authority {} not found", authority_id)))?;

        // Create an attested operation to rotate the epoch
        use aura_core::Hash32;
        use aura_journal::fact::{AttestedOp, Fact, FactContent, TreeOpKind};

        // Create the tree operation
        let tree_op = TreeOpKind::RotateEpoch;

        // Create commitment hash
        use aura_core::hash;
        let mut hasher = hash::hasher();
        hasher.update(b"rotate_epoch_commitment");
        hasher.update(&authority_id.to_bytes());
        let commitment = hasher.finalize();

        // Create attested operation
        let attested_op = AttestedOp {
            tree_op,
            parent_commitment: Hash32::new([0; 32]), // Zero commitment as placeholder for parent
            new_commitment: Hash32::new(commitment),
            witness_threshold: 1, // Minimum threshold for now
            signature: vec![],    // Empty signature for now
        };

        // Create fact
        let fact = Fact {
            fact_id: aura_journal::fact_journal::FactId::new(),
            content: FactContent::AttestedOp(attested_op),
        };

        // Add fact to journal
        journal.add_fact(fact)?;

        // Invalidate cached authority to force reload
        self.authorities.remove(&authority_id);

        Ok(())
    }

    /// Get authority tree information
    pub async fn get_authority_tree_info(
        &mut self,
        authority_id: AuthorityId,
    ) -> Result<(u16, usize, Vec<u8>)> {
        // (threshold, active_devices, root_commitment)
        let authority = self.load_authority(authority_id).await?;

        // Get tree information from the authority trait methods
        let threshold = authority.get_threshold();
        let active_devices = authority.active_device_count();
        let root_commitment = authority.root_commitment().as_bytes().to_vec();

        Ok((threshold, active_devices, root_commitment))
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

        let context_id = manager
            .create_context(vec![auth1, auth2], "guardian".to_string())
            .await
            .unwrap();

        let context = manager.get_context(&context_id).unwrap();
        assert_eq!(context.context_id, context_id);
    }
}
