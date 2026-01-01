//! Authority state tracking for runtime services.

use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::{HashMap, HashSet};

/// Authority lifecycle status within the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityStatus {
    /// Authority is active and available for protocol work.
    Active,
    /// Authority is temporarily suspended.
    Suspended,
    /// Authority has been terminated (runtime-local).
    Terminated,
}

/// Runtime authority state snapshot.
#[derive(Debug, Clone)]
pub struct AuthorityState {
    /// Authority identity.
    pub authority_id: AuthorityId,
    /// Current status.
    pub status: AuthorityStatus,
    /// Active contexts owned by this authority.
    pub contexts: Vec<ContextId>,
    /// Creation timestamp (ms since epoch).
    pub created_at: u64,
    /// Last activity timestamp (ms since epoch).
    pub last_activity: u64,
}

impl AuthorityState {
    /// Create a new authority state snapshot.
    pub fn new(authority_id: AuthorityId, timestamp_ms: u64) -> Self {
        Self {
            authority_id,
            status: AuthorityStatus::Active,
            contexts: Vec::new(),
            created_at: timestamp_ms,
            last_activity: timestamp_ms,
        }
    }

    pub(crate) fn touch(&mut self, timestamp_ms: u64) {
        self.last_activity = timestamp_ms;
    }
}

#[derive(Debug, Default)]
pub(crate) struct AuthorityManagerState {
    pub(crate) authorities: HashMap<AuthorityId, AuthorityState>,
}

impl AuthorityManagerState {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for (authority_id, state) in &self.authorities {
            if *authority_id != state.authority_id {
                return Err(format!(
                    "authority key {:?} does not match state {:?}",
                    authority_id, state.authority_id
                ));
            }
            if state.last_activity < state.created_at {
                return Err(format!(
                    "authority {:?} last_activity < created_at",
                    authority_id
                ));
            }
            let mut seen = HashSet::new();
            for context_id in &state.contexts {
                if !seen.insert(*context_id) {
                    return Err(format!(
                        "authority {:?} has duplicate context {:?}",
                        authority_id, context_id
                    ));
                }
            }
        }
        Ok(())
    }
}
