//! Execution context for effectful operations.
//!
//! This is the canonical `EffectContext` type used across Aura.
//!
//! Design intent:
//! - `AuthorityContext` (in aura-agent) is *identity scope* (who am I, known contexts).
//! - `EffectContext` is *operation scope* (which authority/context/session is this call in).
//! - Protocol/session-specific context lives in session-type runtimes and should not be
//!   merged into long-lived identity containers.

use crate::effects::ExecutionMode;
use crate::hash::hash;
use crate::identifiers::{AuthorityId, ContextId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Operation-scoped context threaded through effectful calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectContext {
    authority_id: AuthorityId,
    context_id: ContextId,
    session_id: SessionId,
    execution_mode: ExecutionMode,
    metadata: HashMap<String, String>,
}

/// Lightweight snapshot of operation context for handlers that don't need metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    authority_id: AuthorityId,
    context_id: ContextId,
    session_id: SessionId,
    execution_mode: ExecutionMode,
}

impl EffectContext {
    /// Create a new context for an operation scoped to a specific authority and context.
    ///
    /// A fresh `SessionId` is allocated to represent the operation/session boundary.
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            session_id: SessionId::new(),
            execution_mode,
            metadata: HashMap::new(),
        }
    }

    /// Convenience for creating a context when only an authority is known.
    ///
    /// This derives a deterministic `ContextId` from the authority for callers that need a
    /// stable default context. Prefer `new(...)` with an explicit `ContextId` when possible.
    pub fn with_authority(authority_id: AuthorityId) -> Self {
        let context_id = ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
        Self::new(authority_id, context_id, ExecutionMode::Production)
    }

    /// Authority performing the operation.
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Relational context in which the operation executes.
    pub fn context_id(&self) -> ContextId {
        self.context_id
    }

    /// Operation/session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Execution mode controlling handler selection.
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Set metadata for diagnostics/telemetry.
    pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }

    /// Get metadata by key.
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Get all metadata.
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// Create a lightweight snapshot without metadata.
    pub fn snapshot(&self) -> ContextSnapshot {
        ContextSnapshot {
            authority_id: self.authority_id,
            context_id: self.context_id,
            session_id: self.session_id,
            execution_mode: self.execution_mode,
        }
    }

    /// Create a child context in the same authority, with a new `ContextId`.
    ///
    /// A new `SessionId` is allocated to avoid accidentally smuggling operation boundaries.
    pub fn create_child(&self, context_id: ContextId) -> Self {
        Self {
            authority_id: self.authority_id,
            context_id,
            session_id: SessionId::new(),
            execution_mode: self.execution_mode,
            metadata: self.metadata.clone(),
        }
    }
}

impl ContextSnapshot {
    /// Create a new snapshot with a fresh session id.
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            session_id: SessionId::new(),
            execution_mode,
        }
    }
    /// Authority performing the operation.
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Relational context in which the operation executes.
    pub fn context_id(&self) -> ContextId {
        self.context_id
    }

    /// Operation/session identifier.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Execution mode controlling handler selection.
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}
