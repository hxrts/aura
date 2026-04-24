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
    let mut material = operation_session_material(authority_id, context_id, execution_mode);
    let nonce = OPERATION_SESSION_NONCE.fetch_add(1, Ordering::Relaxed);
    material.extend_from_slice(&nonce.to_le_bytes());
    OperationSessionId::new(SessionId::new_from_entropy(hash(&material)))
}

fn deterministic_operation_session_id(
    authority_id: AuthorityId,
    context_id: ContextId,
    execution_mode: ExecutionMode,
) -> OperationSessionId {
    let material = operation_session_material(authority_id, context_id, execution_mode);
    OperationSessionId::new(SessionId::new_from_entropy(hash(&material)))
}

fn operation_session_material(
    authority_id: AuthorityId,
    context_id: ContextId,
    execution_mode: ExecutionMode,
) -> Vec<u8> {
    let mut material = Vec::with_capacity(1 + 32 + 32 + 9 + 8);
    material.extend_from_slice(b"aura-session");
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
    material
}

fn default_context_id_for_authority(authority_id: AuthorityId) -> ContextId {
    ContextId::new_from_entropy(hash(&authority_id.to_bytes()))
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
        Self::from_session_id(
            authority_id,
            context_id,
            execution_mode,
            derive_operation_session_id(authority_id, context_id, execution_mode),
        )
    }

    /// Create a context with an explicit operation/session identity.
    pub fn from_session_id(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
        session_id: OperationSessionId,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            session_id,
            execution_mode,
            metadata: HashMap::new(),
        }
    }

    /// Convenience for creating a fresh context in the authority's default context.
    ///
    /// This derives a stable default `ContextId` from the authority but still allocates a
    /// fresh operation/session id. Prefer `new(...)` with an explicit `ContextId` when possible.
    #[must_use]
    pub fn with_default_context(authority_id: AuthorityId, execution_mode: ExecutionMode) -> Self {
        let context_id = default_context_id_for_authority(authority_id);
        Self::new(authority_id, context_id, execution_mode)
    }

    /// Create a deterministic context for testing or simulation.
    ///
    /// Production code must not treat deterministic operation/session ids as freshness evidence.
    #[must_use]
    pub fn deterministic(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> Self {
        assert!(
            execution_mode.is_deterministic(),
            "deterministic EffectContext constructors are reserved for testing/simulation"
        );
        Self::from_session_id(
            authority_id,
            context_id,
            execution_mode,
            deterministic_operation_session_id(authority_id, context_id, execution_mode),
        )
    }

    /// Create a deterministic context in the authority's default context for testing/simulation.
    #[must_use]
    pub fn deterministic_with_default_context(
        authority_id: AuthorityId,
        execution_mode: ExecutionMode,
    ) -> Self {
        let context_id = default_context_id_for_authority(authority_id);
        Self::deterministic(authority_id, context_id, execution_mode)
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
    /// Create a new snapshot with an explicit operation/session id.
    pub fn from_session_id(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
        session_id: OperationSessionId,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            session_id,
            execution_mode,
        }
    }

    /// Create a new snapshot with a fresh session id.
    pub fn fresh(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self::from_session_id(
            authority_id,
            context_id,
            execution_mode,
            derive_operation_session_id(authority_id, context_id, execution_mode),
        )
    }

    /// Create a deterministic snapshot for testing or simulation.
    pub fn deterministic(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> Self {
        assert!(
            execution_mode.is_deterministic(),
            "deterministic ContextSnapshot constructors are reserved for testing/simulation"
        );
        Self::from_session_id(
            authority_id,
            context_id,
            execution_mode,
            deterministic_operation_session_id(authority_id, context_id, execution_mode),
        )
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

    #[test]
    fn fresh_effect_context_allocates_distinct_session_ids_for_same_scope() {
        let authority_id = AuthorityId::new_from_entropy([3; 32]);
        let context_id = ContextId::new_from_entropy([4; 32]);

        let first = EffectContext::new(authority_id, context_id, ExecutionMode::Production);
        let second = EffectContext::new(authority_id, context_id, ExecutionMode::Production);

        assert_ne!(first.session_id(), second.session_id());
    }

    #[test]
    fn deterministic_effect_context_is_replayable_for_testing() {
        let authority_id = AuthorityId::new_from_entropy([5; 32]);
        let context_id = ContextId::new_from_entropy([6; 32]);

        let first = EffectContext::deterministic(authority_id, context_id, ExecutionMode::Testing);
        let second = EffectContext::deterministic(authority_id, context_id, ExecutionMode::Testing);

        assert_eq!(first.session_id(), second.session_id());
    }

    #[test]
    #[should_panic(expected = "deterministic EffectContext constructors are reserved")]
    fn deterministic_effect_context_rejects_production_mode() {
        let authority_id = AuthorityId::new_from_entropy([7; 32]);
        let context_id = ContextId::new_from_entropy([8; 32]);
        let _ = EffectContext::deterministic(authority_id, context_id, ExecutionMode::Production);
    }

    #[test]
    fn context_snapshot_fresh_and_deterministic_constructors_match_semantics() {
        let authority_id = AuthorityId::new_from_entropy([9; 32]);
        let context_id = ContextId::new_from_entropy([10; 32]);

        let fresh_a = ContextSnapshot::fresh(authority_id, context_id, ExecutionMode::Testing);
        let fresh_b = ContextSnapshot::fresh(authority_id, context_id, ExecutionMode::Testing);
        assert_ne!(fresh_a.session_id(), fresh_b.session_id());

        let deterministic_a =
            ContextSnapshot::deterministic(authority_id, context_id, ExecutionMode::Testing);
        let deterministic_b =
            ContextSnapshot::deterministic(authority_id, context_id, ExecutionMode::Testing);
        assert_eq!(deterministic_a.session_id(), deterministic_b.session_id());
    }
}
