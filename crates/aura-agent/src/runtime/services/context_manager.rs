//! Context Manager Service
//!
//! Manages execution contexts and authority relationships.
//! Provides context lifecycle management and isolation for concurrent protocol executions.

use crate::core::AgentConfig;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::HashMap;
use tokio::sync::RwLock;

/// Context status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextStatus {
    /// Context is active
    Active,
    /// Context is suspended
    Suspended,
    /// Context is being terminated
    Terminating,
    /// Context has been terminated
    Terminated,
}

/// An execution context
#[derive(Debug, Clone)]
pub struct Context {
    /// Context ID
    pub id: ContextId,
    /// Primary authority for this context
    pub authority_id: AuthorityId,
    /// Current status
    pub status: ContextStatus,
    /// Creation timestamp (ms since epoch)
    pub created_at: u64,
    /// Last activity timestamp (ms since epoch)
    pub last_activity: u64,
    /// Peer authorities involved in this context
    pub peers: Vec<AuthorityId>,
}

impl Context {
    /// Create a new context
    pub fn new(id: ContextId, authority_id: AuthorityId, timestamp: u64) -> Self {
        Self {
            id,
            authority_id,
            status: ContextStatus::Active,
            created_at: timestamp,
            last_activity: timestamp,
            peers: Vec::new(),
        }
    }
}

/// Context manager error
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("Context not found: {0:?}")]
    NotFound(ContextId),
    #[error("Lock error")]
    LockError,
    #[error("Invalid state transition: {from:?} -> {to:?}")]
    InvalidStateTransition {
        from: ContextStatus,
        to: ContextStatus,
    },
    #[error("Context already exists: {0:?}")]
    AlreadyExists(ContextId),
}

/// Context manager service
pub struct ContextManager {
    #[allow(dead_code)] // Will be used for context configuration
    config: AgentConfig,
    /// Context storage by ID
    contexts: RwLock<HashMap<ContextId, Context>>,
    /// Authority-to-contexts index
    authority_contexts: RwLock<HashMap<AuthorityId, Vec<ContextId>>>,
}

impl ContextManager {
    /// Create a new context manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
            contexts: RwLock::new(HashMap::new()),
            authority_contexts: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new context for an authority
    pub async fn create_context(
        &self,
        authority: AuthorityId,
        timestamp: u64,
    ) -> Result<ContextId, ContextError> {
        // Generate a new context ID
        let context_id =
            ContextId::new_from_entropy(aura_core::hash::hash(&timestamp.to_le_bytes()));

        let context = Context::new(context_id, authority, timestamp);

        // Store the context
        let mut contexts = self.contexts.write().await;
        if contexts.contains_key(&context_id) {
            return Err(ContextError::AlreadyExists(context_id));
        }
        contexts.insert(context_id, context);

        // Update the authority index
        let mut authority_contexts = self.authority_contexts.write().await;
        authority_contexts
            .entry(authority)
            .or_default()
            .push(context_id);

        Ok(context_id)
    }

    /// Get a context by ID
    pub async fn get_context(&self, id: ContextId) -> Result<Option<Context>, ContextError> {
        let contexts = self.contexts.read().await;
        Ok(contexts.get(&id).cloned())
    }

    /// List all contexts for an authority
    pub async fn list_contexts_for_authority(
        &self,
        authority: AuthorityId,
    ) -> Result<Vec<ContextId>, ContextError> {
        let authority_contexts = self.authority_contexts.read().await;
        Ok(authority_contexts
            .get(&authority)
            .cloned()
            .unwrap_or_default())
    }

    /// Suspend a context
    pub async fn suspend_context(&self, id: ContextId) -> Result<(), ContextError> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(&id).ok_or(ContextError::NotFound(id))?;

        if context.status != ContextStatus::Active {
            return Err(ContextError::InvalidStateTransition {
                from: context.status,
                to: ContextStatus::Suspended,
            });
        }

        context.status = ContextStatus::Suspended;
        Ok(())
    }

    /// Resume a suspended context
    pub async fn resume_context(&self, id: ContextId) -> Result<(), ContextError> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(&id).ok_or(ContextError::NotFound(id))?;

        if context.status != ContextStatus::Suspended {
            return Err(ContextError::InvalidStateTransition {
                from: context.status,
                to: ContextStatus::Active,
            });
        }

        context.status = ContextStatus::Active;
        Ok(())
    }

    /// Terminate a context
    pub async fn terminate_context(&self, id: ContextId) -> Result<(), ContextError> {
        let mut contexts = self.contexts.write().await;
        let context = contexts.get_mut(&id).ok_or(ContextError::NotFound(id))?;

        if context.status == ContextStatus::Terminated {
            return Ok(()); // Already terminated
        }

        context.status = ContextStatus::Terminated;
        Ok(())
    }

    /// Add a peer to a context
    pub async fn add_peer(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Result<(), ContextError> {
        let mut contexts = self.contexts.write().await;
        let context = contexts
            .get_mut(&context_id)
            .ok_or(ContextError::NotFound(context_id))?;

        if !context.peers.contains(&peer) {
            context.peers.push(peer);
        }
        Ok(())
    }

    /// Update last activity timestamp
    pub async fn touch(&self, context_id: ContextId, timestamp: u64) -> Result<(), ContextError> {
        let mut contexts = self.contexts.write().await;
        if let Some(context) = contexts.get_mut(&context_id) {
            context.last_activity = timestamp;
        }
        Ok(())
    }

    /// List all active contexts
    pub async fn list_active_contexts(&self) -> Result<Vec<ContextId>, ContextError> {
        let contexts = self.contexts.read().await;
        Ok(contexts
            .iter()
            .filter(|(_, ctx)| ctx.status == ContextStatus::Active)
            .map(|(id, _)| *id)
            .collect())
    }
}
