//! Authority Manager Service
//!
//! Tracks runtime-local authority lifecycle and active contexts.

use super::authority_state::{AuthorityManagerState, AuthorityState, AuthorityStatus};
use super::state::with_state_mut_validated;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Authority manager error
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    #[error("Authority not found: {0:?}")]
    NotFound(AuthorityId),
    #[error("Authority already exists: {0:?}")]
    AlreadyExists(AuthorityId),
    #[error("Invalid state transition: {from:?} -> {to:?}")]
    InvalidStateTransition {
        from: AuthorityStatus,
        to: AuthorityStatus,
    },
    #[error("Context already tracked: {0:?}")]
    ContextAlreadyTracked(ContextId),
    #[error("Context not tracked: {0:?}")]
    ContextNotTracked(ContextId),
}

/// Authority manager service
pub struct AuthorityManager {
    state: Arc<RwLock<AuthorityManagerState>>,
}

impl AuthorityManager {
    /// Create a new authority manager.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(AuthorityManagerState::default())),
        }
    }

    /// Register a new authority.
    pub async fn register_authority(
        &self,
        authority_id: AuthorityId,
        timestamp_ms: u64,
    ) -> Result<AuthorityState, AuthorityError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                if state.authorities.contains_key(&authority_id) {
                    return Err(AuthorityError::AlreadyExists(authority_id));
                }
                let authority_state = AuthorityState::new(authority_id, timestamp_ms);
                state
                    .authorities
                    .insert(authority_id, authority_state.clone());
                Ok(authority_state)
            },
            |state| state.validate(),
        )
        .await
    }

    /// Ensure an authority exists, returning the current state.
    pub async fn ensure_authority(
        &self,
        authority_id: AuthorityId,
        timestamp_ms: u64,
    ) -> Result<AuthorityState, AuthorityError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                if let Some(existing) = state.authorities.get(&authority_id) {
                    return Ok(existing.clone());
                }
                let authority_state = AuthorityState::new(authority_id, timestamp_ms);
                state
                    .authorities
                    .insert(authority_id, authority_state.clone());
                Ok(authority_state)
            },
            |state| state.validate(),
        )
        .await
    }

    /// Get authority state.
    pub async fn get_authority(
        &self,
        authority_id: AuthorityId,
    ) -> Result<Option<AuthorityState>, AuthorityError> {
        let state = self.state.read().await;
        Ok(state.authorities.get(&authority_id).cloned())
    }

    /// List all authority IDs.
    pub async fn list_authorities(&self) -> Result<Vec<AuthorityId>, AuthorityError> {
        let state = self.state.read().await;
        Ok(state.authorities.keys().copied().collect())
    }

    /// Set authority status with lifecycle validation.
    pub async fn set_status(
        &self,
        authority_id: AuthorityId,
        status: AuthorityStatus,
        timestamp_ms: u64,
    ) -> Result<(), AuthorityError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let authority = state
                    .authorities
                    .get_mut(&authority_id)
                    .ok_or(AuthorityError::NotFound(authority_id))?;

                if !transition_allowed(authority.status, status) {
                    return Err(AuthorityError::InvalidStateTransition {
                        from: authority.status,
                        to: status,
                    });
                }

                authority.status = status;
                authority.touch(timestamp_ms);
                Ok(())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Add a context to the authority.
    pub async fn add_context(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
        timestamp_ms: u64,
    ) -> Result<(), AuthorityError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let authority = state
                    .authorities
                    .get_mut(&authority_id)
                    .ok_or(AuthorityError::NotFound(authority_id))?;
                if authority.contexts.contains(&context_id) {
                    return Err(AuthorityError::ContextAlreadyTracked(context_id));
                }
                authority.contexts.push(context_id);
                authority.touch(timestamp_ms);
                Ok(())
            },
            |state| state.validate(),
        )
        .await
    }

    /// Remove a context from the authority.
    pub async fn remove_context(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
        timestamp_ms: u64,
    ) -> Result<(), AuthorityError> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let authority = state
                    .authorities
                    .get_mut(&authority_id)
                    .ok_or(AuthorityError::NotFound(authority_id))?;
                if let Some(pos) = authority.contexts.iter().position(|id| *id == context_id) {
                    authority.contexts.swap_remove(pos);
                    authority.touch(timestamp_ms);
                    Ok(())
                } else {
                    Err(AuthorityError::ContextNotTracked(context_id))
                }
            },
            |state| state.validate(),
        )
        .await
    }

    /// Active contexts for the authority (snapshot).
    pub async fn active_contexts(
        &self,
        authority_id: AuthorityId,
    ) -> Result<Vec<ContextId>, AuthorityError> {
        let state = self.state.read().await;
        let authority = state
            .authorities
            .get(&authority_id)
            .ok_or(AuthorityError::NotFound(authority_id))?;
        Ok(authority.contexts.clone())
    }

    /// Returns true if authority exists in the registry.
    pub async fn has_authority(&self, authority_id: AuthorityId) -> Result<bool, AuthorityError> {
        let state = self.state.read().await;
        Ok(state.authorities.contains_key(&authority_id))
    }
}

impl Default for AuthorityManager {
    fn default() -> Self {
        Self::new()
    }
}

fn transition_allowed(from: AuthorityStatus, to: AuthorityStatus) -> bool {
    use AuthorityStatus::*;
    matches!(
        (from, to),
        (Active, Active)
            | (Active, Suspended)
            | (Active, Terminated)
            | (Suspended, Active)
            | (Suspended, Suspended)
            | (Suspended, Terminated)
            | (Terminated, Terminated)
            | (Terminated, Active)
    )
}
