//! Authority Manager Service
//!
//! Provides authority lifecycle management, multi-authority coordination,
//! and authority-scoped resource management in the runtime layer.
//!
//! ## Functionality
//!
//! - Register and track authority lifecycle
//! - Manage active contexts per authority
//! - Track authority status (Initializing, Active, Suspended, ShuttingDown, Terminated)
//! - Support multi-authority coordination in the runtime
//!
//! # Blocking Lock Usage
//!
//! Uses `std::sync::RwLock` (not tokio or parking_lot) because:
//! 1. Lock poisoning detection is required - the code handles `PoisonError` explicitly
//! 2. Operations are brief HashMap lookups/inserts (sub-millisecond)
//! 3. No `.await` points inside lock scope

#![allow(clippy::disallowed_types)]

use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Manager for authority lifecycle and coordination
#[derive(Debug)]
#[allow(dead_code)] // Part of future authority management API
pub struct AuthorityManager {
    authorities: Arc<RwLock<HashMap<AuthorityId, AuthorityState>>>,
}

impl AuthorityManager {
    /// Create a new authority manager
    #[allow(dead_code)] // Part of future authority management API
    pub fn new() -> Self {
        Self {
            authorities: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an authority
    #[allow(dead_code)] // Part of future authority management API
    pub fn register_authority(&self, authority_id: AuthorityId) -> Result<(), AuthorityError> {
        let mut authorities = self
            .authorities
            .write()
            .map_err(|_| AuthorityError::LockError)?;

        authorities.insert(authority_id, AuthorityState::new(authority_id));
        Ok(())
    }

    /// Get authority state
    #[allow(dead_code)] // Part of future authority management API
    pub fn get_authority(
        &self,
        authority_id: AuthorityId,
    ) -> Result<Option<AuthorityState>, AuthorityError> {
        let authorities = self
            .authorities
            .read()
            .map_err(|_| AuthorityError::LockError)?;

        Ok(authorities.get(&authority_id).cloned())
    }

    /// List all registered authorities
    #[allow(dead_code)] // Part of future authority management API
    pub fn list_authorities(&self) -> Result<Vec<AuthorityId>, AuthorityError> {
        let authorities = self
            .authorities
            .read()
            .map_err(|_| AuthorityError::LockError)?;

        Ok(authorities.keys().cloned().collect())
    }

    /// Remove an authority
    #[allow(dead_code)] // Part of future authority management API
    pub fn remove_authority(
        &self,
        authority_id: AuthorityId,
    ) -> Result<Option<AuthorityState>, AuthorityError> {
        let mut authorities = self
            .authorities
            .write()
            .map_err(|_| AuthorityError::LockError)?;

        Ok(authorities.remove(&authority_id))
    }

    /// Check if authority is registered
    #[allow(dead_code)] // Part of future authority management API
    pub fn has_authority(&self, authority_id: AuthorityId) -> bool {
        self.authorities
            .read()
            .map(|authorities| authorities.contains_key(&authority_id))
            .unwrap_or(false)
    }
}

impl Default for AuthorityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// State tracking for an individual authority
#[derive(Debug, Clone)]
#[allow(dead_code)] // Part of future authority management API
pub struct AuthorityState {
    authority_id: AuthorityId,
    active_contexts: HashMap<ContextId, crate::runtime::EffectContext>,
    status: AuthorityStatus,
}

impl AuthorityState {
    /// Create new authority state
    #[allow(dead_code)] // Part of future authority management API
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            active_contexts: HashMap::new(),
            status: AuthorityStatus::Initializing,
        }
    }

    /// Get the authority ID
    #[allow(dead_code)] // Part of future authority management API
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get the current status
    #[allow(dead_code)] // Part of future authority management API
    pub fn status(&self) -> AuthorityStatus {
        self.status
    }

    /// Set the status
    #[allow(dead_code)] // Part of future authority management API
    pub fn set_status(&mut self, status: AuthorityStatus) {
        self.status = status;
    }

    /// Add an active context
    #[allow(dead_code)] // Part of future authority management API
    pub fn add_context(&mut self, context: crate::runtime::EffectContext) {
        let context_id = context.context_id();
        self.active_contexts.insert(context_id, context);
    }

    /// Remove an active context
    #[allow(dead_code)] // Part of future authority management API
    pub fn remove_context(
        &mut self,
        context_id: ContextId,
    ) -> Option<crate::runtime::EffectContext> {
        self.active_contexts.remove(&context_id)
    }

    /// Get active contexts
    #[allow(dead_code)] // Part of future authority management API
    pub fn active_contexts(&self) -> &HashMap<ContextId, crate::runtime::EffectContext> {
        &self.active_contexts
    }

    /// Get count of active contexts
    #[allow(dead_code)] // Part of future authority management API
    pub fn context_count(&self) -> usize {
        self.active_contexts.len()
    }
}

/// Status of an authority
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Part of future authority management API
pub enum AuthorityStatus {
    Initializing,
    Active,
    Suspended,
    ShuttingDown,
    Terminated,
}

/// Shared reference to AuthorityManager
#[allow(dead_code)] // Part of future authority management API
pub type SharedAuthorityManager = Arc<AuthorityManager>;

/// Authority management errors
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Part of future authority management API
pub enum AuthorityError {
    #[error("Lock error")]
    LockError,
    #[error("Authority not found: {authority_id:?}")]
    AuthorityNotFound { authority_id: AuthorityId },
    #[error("Authority already exists: {authority_id:?}")]
    AuthorityAlreadyExists { authority_id: AuthorityId },
    #[error("Invalid state transition: {from:?} -> {to:?}")]
    InvalidStateTransition {
        from: AuthorityStatus,
        to: AuthorityStatus,
    },
}
