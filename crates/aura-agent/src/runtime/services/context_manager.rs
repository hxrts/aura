//! Context Manager Service
//!
//! Manages execution contexts and authority relationships.
//! Provides context lifecycle management and isolation for concurrent protocol executions.

use crate::core::AgentConfig;
use aura_core::identifiers::{AuthorityId, ContextId};
use super::state::with_state_mut_validated;
use std::collections::HashMap;
use std::sync::Arc;
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
    /// Context storage by ID and authority index
    state: Arc<RwLock<ContextManagerState>>,
}

#[derive(Debug, Default)]
struct ContextManagerState {
    contexts: HashMap<ContextId, Context>,
    authority_contexts: HashMap<AuthorityId, Vec<ContextId>>,
}

impl ContextManagerState {
    fn validate(&self) -> Result<(), String> {
        for (authority, contexts) in &self.authority_contexts {
            let mut seen = std::collections::HashSet::new();
            for context_id in contexts {
                if !seen.insert(*context_id) {
                    return Err(format!(
                        "authority {:?} has duplicate context entry {:?}",
                        authority, context_id
                    ));
                }
                let context = self.contexts.get(context_id).ok_or_else(|| {
                    format!(
                        "authority {:?} references missing context {:?}",
                        authority, context_id
                    )
                })?;
                if context.authority_id != *authority {
                    return Err(format!(
                        "context {:?} authority mismatch: {:?} vs {:?}",
                        context_id, context.authority_id, authority
                    ));
                }
            }
        }

        for (context_id, context) in &self.contexts {
            let contexts = self
                .authority_contexts
                .get(&context.authority_id)
                .ok_or_else(|| {
                    format!(
                        "context {:?} missing authority index for {:?}",
                        context_id, context.authority_id
                    )
                })?;
            if !contexts.contains(context_id) {
                return Err(format!(
                    "context {:?} missing from authority index {:?}",
                    context_id, context.authority_id
                ));
            }
        }
        Ok(())
    }
}

impl ContextManager {
    /// Create a new context manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
            state: Arc::new(RwLock::new(ContextManagerState::default())),
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

        with_state_mut_validated(
            &self.state,
            |state| {
                if state.contexts.contains_key(&context_id) {
                    return Err(ContextError::AlreadyExists(context_id));
                }
                state.contexts.insert(context_id, context);
                state
                    .authority_contexts
                    .entry(authority)
                    .or_default()
                    .push(context_id);
                Ok(context_id)
            },
            |state| state.validate(),
        )
        .await
    }

    /// Get a context by ID
    pub async fn get_context(&self, id: ContextId) -> Result<Option<Context>, ContextError> {
        let state = self.state.read().await;
        Ok(state.contexts.get(&id).cloned())
    }

    /// List all contexts for an authority
    pub async fn list_contexts_for_authority(
        &self,
        authority: AuthorityId,
    ) -> Result<Vec<ContextId>, ContextError> {
        let state = self.state.read().await;
        Ok(state
            .authority_contexts
            .get(&authority)
            .cloned()
            .unwrap_or_default())
    }

    /// Suspend a context
    pub async fn suspend_context(&self, id: ContextId) -> Result<(), ContextError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let context = state.contexts.get_mut(&id).ok_or(ContextError::NotFound(id))?;

                if context.status != ContextStatus::Active {
                    return Err(ContextError::InvalidStateTransition {
                        from: context.status,
                        to: ContextStatus::Suspended,
                    });
                }

                context.status = ContextStatus::Suspended;
                Ok(())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Resume a suspended context
    pub async fn resume_context(&self, id: ContextId) -> Result<(), ContextError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let context = state.contexts.get_mut(&id).ok_or(ContextError::NotFound(id))?;

                if context.status != ContextStatus::Suspended {
                    return Err(ContextError::InvalidStateTransition {
                        from: context.status,
                        to: ContextStatus::Active,
                    });
                }

                context.status = ContextStatus::Active;
                Ok(())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Terminate a context
    pub async fn terminate_context(&self, id: ContextId) -> Result<(), ContextError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let context = state.contexts.get_mut(&id).ok_or(ContextError::NotFound(id))?;

                if context.status == ContextStatus::Terminated {
                    return Ok(()); // Already terminated
                }

                context.status = ContextStatus::Terminated;
                Ok(())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Add a peer to a context
    pub async fn add_peer(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Result<(), ContextError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let context = state
                    .contexts
                    .get_mut(&context_id)
                    .ok_or(ContextError::NotFound(context_id))?;

                if !context.peers.contains(&peer) {
                    context.peers.push(peer);
                }
                Ok(())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Update last activity timestamp
    pub async fn touch(&self, context_id: ContextId, timestamp: u64) -> Result<(), ContextError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                if let Some(context) = state.contexts.get_mut(&context_id) {
                    context.last_activity = timestamp;
                }
            },
            |state| state.validate(),
        )
        .await;
        Ok(())
    }

    /// List all active contexts
    pub async fn list_active_contexts(&self) -> Result<Vec<ContextId>, ContextError> {
        let state = self.state.read().await;
        Ok(state
            .contexts
            .iter()
            .filter(|(_, ctx)| ctx.status == ContextStatus::Active)
            .map(|(id, _)| *id)
            .collect())
    }
}
