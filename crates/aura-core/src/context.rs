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
use crate::types::identifiers::{AuthorityId, ContextId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

static OPERATION_SESSION_NONCE: AtomicU64 = AtomicU64::new(1);

/// Operation-scoped session identity for `EffectContext`.
///
/// This is intentionally distinct from long-lived Aura `SessionId` values used for
/// durable domain state and from runtime choreography session identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationSessionId(SessionId);

impl OperationSessionId {
    /// Wrap one raw Aura session identifier as an operation-scoped session identity.
    #[must_use]
    pub fn new(session_id: SessionId) -> Self {
        Self(session_id)
    }

    /// Borrow the underlying Aura session identifier.
    #[must_use]
    pub fn raw(self) -> SessionId {
        self.0
    }
}

impl fmt::Display for OperationSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<SessionId> for OperationSessionId {
    fn from(value: SessionId) -> Self {
        Self::new(value)
    }
}

impl From<OperationSessionId> for SessionId {
    fn from(value: OperationSessionId) -> Self {
        value.raw()
    }
}

/// Operation-scoped context threaded through effectful calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectContext {
    authority_id: AuthorityId,
    context_id: ContextId,
    session_id: OperationSessionId,
    execution_mode: ExecutionMode,
    metadata: HashMap<String, String>,
}

/// Lightweight snapshot of operation context for handlers that don't need metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSnapshot {
    authority_id: AuthorityId,
    context_id: ContextId,
    session_id: OperationSessionId,
    execution_mode: ExecutionMode,
}

fn derive_operation_session_id(
    authority_id: AuthorityId,
    context_id: ContextId,
    execution_mode: ExecutionMode,
) -> OperationSessionId {
    let nonce = OPERATION_SESSION_NONCE.fetch_add(1, Ordering::Relaxed);
    let mut material = Vec::with_capacity(1 + 32 + 32 + 9 + 8);
    material.extend_from_slice(b"aura-session");
    material.extend_from_slice(&nonce.to_le_bytes());
    material.extend_from_slice(&authority_id.to_bytes());
    material.extend_from_slice(&context_id.to_bytes());
    match execution_mode {
        ExecutionMode::Testing => material.push(0),
        ExecutionMode::Production => material.push(1),
        ExecutionMode::Simulation { seed } => {
            material.push(2);
            material.extend_from_slice(&seed.to_le_bytes());
        }
    }
    OperationSessionId::new(SessionId::new_from_entropy(hash(&material)))
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
            session_id: derive_operation_session_id(authority_id, context_id, execution_mode),
            execution_mode,
            metadata: HashMap::new(),
        }
    }

    /// Convenience for creating a context when only an authority is known.
    ///
    /// This derives a deterministic `ContextId` from the authority for callers that need a
    /// stable default context. Prefer `new(...)` with an explicit `ContextId` when possible.
    #[must_use]
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
    pub fn session_id(&self) -> OperationSessionId {
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
            session_id: derive_operation_session_id(
                self.authority_id,
                context_id,
                self.execution_mode,
            ),
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
            session_id: derive_operation_session_id(authority_id, context_id, execution_mode),
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
    pub fn session_id(&self) -> OperationSessionId {
        self.session_id
    }

    /// Execution mode controlling handler selection.
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_session_id_round_trips_raw_session_id() {
        let raw = SessionId::new_from_entropy([7; 32]);
        let operation = OperationSessionId::new(raw);
        assert_eq!(operation.raw(), raw);
    }

    #[test]
    fn effect_context_uses_operation_session_id_boundary() {
        let authority_id = AuthorityId::new_from_entropy([1; 32]);
        let context_id = ContextId::new_from_entropy([2; 32]);
        let context = EffectContext::new(authority_id, context_id, ExecutionMode::Testing);

        let operation_session = context.session_id();
        let raw_session: SessionId = operation_session.into();

        assert_eq!(operation_session.raw(), raw_session);
    }
}
