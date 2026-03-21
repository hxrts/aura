//! Choreography State
//!
//! In-memory choreography session state for runtime coordination.

use aura_core::effects::transport::TransportEnvelope;
use aura_core::{AuthorityId, ContextId, DeviceId, SessionId};
use aura_protocol::effects::{ChoreographicRole, ChoreographyMetrics, RoleIndex};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;
use std::thread::ThreadId;
use tokio::sync::Notify;
use tokio::task::Id as TaskId;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ExecutionBindingKey {
    Task(TaskId),
    Thread(ThreadId),
}

/// Runtime choreography session identity bound to one active protocol execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuntimeChoreographySessionId(Uuid);

impl RuntimeChoreographySessionId {
    /// Wrap one raw runtime session UUID.
    pub fn from_uuid(session_id: Uuid) -> Self {
        Self(session_id)
    }

    /// Borrow the raw runtime session UUID.
    pub fn as_uuid(self) -> Uuid {
        self.0
    }

    /// Explicitly bridge one durable Aura session identifier into runtime choreography scope.
    pub fn from_aura_session_id(session_id: SessionId) -> Self {
        Self::from_uuid(session_id.uuid())
    }

    /// Explicitly bridge one runtime choreography session into durable Aura session scope.
    pub fn into_aura_session_id(self) -> SessionId {
        SessionId::from_uuid(self.as_uuid())
    }
}

impl fmt::Display for RuntimeChoreographySessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Authoritative local owner record for one active runtime session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOwnerRecord {
    /// Stable local owner label.
    pub owner_label: String,
    /// Current capability proving authority to act for this owner.
    pub capability: SessionOwnerCapability,
}

/// Scope granted to one current runtime session owner capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOwnerCapabilityScope {
    /// Grants authority across the full runtime session.
    Session,
    /// Grants authority only to the listed fragments.
    Fragments(BTreeSet<String>),
}

/// Capability proving current authority to act on one runtime session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOwnerCapability {
    pub session_id: RuntimeChoreographySessionId,
    pub owner_label: String,
    pub generation: u64,
    pub scope: SessionOwnerCapabilityScope,
}

impl SessionOwnerCapability {
    pub fn full_session(
        session_id: RuntimeChoreographySessionId,
        owner_label: impl Into<String>,
        generation: u64,
    ) -> Self {
        Self {
            session_id,
            owner_label: owner_label.into(),
            generation,
            scope: SessionOwnerCapabilityScope::Session,
        }
    }

    pub fn allows_full_session(&self) -> bool {
        matches!(self.scope, SessionOwnerCapabilityScope::Session)
    }

    pub fn with_scope(mut self, scope: SessionOwnerCapabilityScope) -> Self {
        self.scope = scope;
        self
    }
}

/// Errors raised while managing runtime session ownership.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SessionOwnershipError {
    /// Another local owner already claimed the active session.
    #[error(
        "runtime session {session_id} is already owned by {existing_owner}; requested owner {requested_owner}"
    )]
    OwnerConflict {
        session_id: RuntimeChoreographySessionId,
        existing_owner: String,
        requested_owner: String,
    },
    /// No authoritative owner exists for the requested session.
    #[error("runtime session {session_id} has no owner record")]
    MissingOwner {
        session_id: RuntimeChoreographySessionId,
    },
    /// The caller does not match the authoritative owner for the requested session.
    #[error("runtime session {session_id} is not owned by expected owner {expected_owner}")]
    OwnerMismatch {
        session_id: RuntimeChoreographySessionId,
        expected_owner: String,
    },
    /// The caller's capability is stale or insufficient for the current owner record.
    #[error(
        "runtime session {session_id} rejected capability for owner {expected_owner}; current generation is {current_generation}"
    )]
    CapabilityMismatch {
        session_id: RuntimeChoreographySessionId,
        expected_owner: String,
        current_generation: u64,
    },
}

/// Typed reasons why a runtime choreography session start was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStartError {
    /// The current task or thread is still bound to an active session.
    TaskAlreadyBound {
        session_id: RuntimeChoreographySessionId,
    },
    /// Another active session already exists for the requested session id.
    SessionAlreadyExists {
        session_id: RuntimeChoreographySessionId,
    },
}

impl fmt::Display for SessionStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TaskAlreadyBound { session_id } => {
                write!(
                    f,
                    "task already bound to active choreography session {session_id}"
                )
            }
            Self::SessionAlreadyExists { session_id } => {
                write!(f, "choreography session already exists: {session_id}")
            }
        }
    }
}

/// In-memory choreography session state for one runtime session.
#[derive(Debug, Clone)]
pub struct ChoreographySessionState {
    /// Current session ID.
    pub session_id: RuntimeChoreographySessionId,
    /// Context ID for this session
    pub context_id: ContextId,
    /// Roles participating in this choreography
    pub roles: Vec<ChoreographicRole>,
    /// This node's current role
    pub current_role: ChoreographicRole,
    /// Session timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Session start time in milliseconds since epoch
    pub started_at_ms: Option<u64>,
    /// Session metrics
    pub metrics: ChoreographyMetrics,
}

impl ChoreographySessionState {
    fn new(
        session_id: RuntimeChoreographySessionId,
        context_id: ContextId,
        roles: Vec<ChoreographicRole>,
        current_role: ChoreographicRole,
        timeout_ms: Option<u64>,
        now_ms: u64,
    ) -> Self {
        Self {
            session_id,
            context_id,
            roles,
            current_role,
            timeout_ms,
            started_at_ms: Some(now_ms),
            metrics: default_metrics(),
        }
    }
}

/// In-memory choreography session registry keyed by runtime session id.
#[derive(Debug, Clone, Default)]
pub struct ChoreographyState {
    sessions: HashMap<RuntimeChoreographySessionId, ChoreographySessionState>,
    session_owners: HashMap<RuntimeChoreographySessionId, SessionOwnerRecord>,
    session_owner_generations: HashMap<RuntimeChoreographySessionId, u64>,
    task_bindings: HashMap<ExecutionBindingKey, RuntimeChoreographySessionId>,
    session_inbox_notifiers: HashMap<RuntimeChoreographySessionId, Arc<Notify>>,
    session_inboxes: HashMap<RuntimeChoreographySessionId, Vec<TransportEnvelope>>,
}

fn default_metrics() -> ChoreographyMetrics {
    ChoreographyMetrics {
        messages_sent: 0,
        messages_received: 0,
        avg_latency_ms: 0.0,
        timeout_count: 0,
        retry_count: 0,
        total_duration_ms: 0,
    }
}

impl Default for ChoreographySessionState {
    fn default() -> Self {
        Self {
            session_id: RuntimeChoreographySessionId::from_uuid(Uuid::from_bytes([0xA5; 16])),
            context_id: ContextId::new_from_entropy([0; 32]),
            roles: Vec::new(),
            current_role: ChoreographicRole::new(
                DeviceId::from_uuid(Uuid::from_bytes([0x5A; 16])),
                AuthorityId::from_uuid(Uuid::from_bytes([0x5B; 16])),
                RoleIndex::new(0).expect("role index"),
            ),
            timeout_ms: None,
            started_at_ms: None,
            metrics: default_metrics(),
        }
    }
}

#[allow(dead_code)]
impl ChoreographyState {
    #[allow(clippy::disallowed_methods)] // Fallback for tests/sync callers outside a Tokio task.
    fn current_binding_key() -> ExecutionBindingKey {
        tokio::task::try_id()
            .map(ExecutionBindingKey::Task)
            .unwrap_or_else(|| ExecutionBindingKey::Thread(std::thread::current().id()))
    }

    /// Create a new empty state
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the active session id bound to the current Tokio task, if any.
    pub fn current_session_id(&self) -> Option<RuntimeChoreographySessionId> {
        self.task_bindings
            .get(&Self::current_binding_key())
            .copied()
    }

    /// Return a clone of the session state bound to the current Tokio task.
    pub fn current_session(&self) -> Option<ChoreographySessionState> {
        let session_id = self.current_session_id()?;
        self.sessions.get(&session_id).cloned()
    }

    /// Number of active runtime choreography sessions.
    pub fn active_session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Start a new session and bind it to the current Tokio task.
    pub fn start_session(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        context_id: ContextId,
        roles: Vec<ChoreographicRole>,
        current_role: ChoreographicRole,
        timeout_ms: Option<u64>,
        now_ms: u64,
    ) -> Result<(), SessionStartError> {
        let task_id = Self::current_binding_key();
        if let Some(existing_session_id) = self.task_bindings.get(&task_id).copied() {
            if self.sessions.contains_key(&existing_session_id) {
                return Err(SessionStartError::TaskAlreadyBound {
                    session_id: existing_session_id,
                });
            }
            self.task_bindings.remove(&task_id);
        }
        if self.sessions.contains_key(&session_id) {
            return Err(SessionStartError::SessionAlreadyExists { session_id });
        }

        self.sessions.insert(
            session_id,
            ChoreographySessionState::new(
                session_id,
                context_id,
                roles,
                current_role,
                timeout_ms,
                now_ms,
            ),
        );
        self.session_inbox_notifiers
            .insert(session_id, Arc::new(Notify::new()));
        self.session_inboxes.entry(session_id).or_default();
        self.task_bindings.insert(task_id, session_id);
        Ok(())
    }

    /// End the current task-bound session and clean up all session bindings.
    pub fn end_session(&mut self, now_ms: u64) -> Result<RuntimeChoreographySessionId, String> {
        let task_id = Self::current_binding_key();
        let session_id = self
            .task_bindings
            .remove(&task_id)
            .ok_or_else(|| "no choreography session bound to current task".to_string())?;

        let Some(mut session) = self.sessions.remove(&session_id) else {
            return Err(format!(
                "missing choreography session state for bound session {session_id}"
            ));
        };

        if let Some(started) = session.started_at_ms {
            session.metrics.total_duration_ms = now_ms.saturating_sub(started);
        }
        if let Some(notify) = self.session_inbox_notifiers.remove(&session_id) {
            notify.notify_waiters();
        }
        self.session_owners.remove(&session_id);
        self.session_inboxes.remove(&session_id);
        self.task_bindings.retain(|_, sid| *sid != session_id);
        Ok(session_id)
    }

    /// Cancel one session by explicit runtime session id and wake any blocked waiters.
    pub fn cancel_session(&mut self, session_id: RuntimeChoreographySessionId) -> bool {
        let removed = self.sessions.remove(&session_id).is_some();
        if let Some(notify) = self.session_inbox_notifiers.remove(&session_id) {
            notify.notify_waiters();
        }
        self.session_owners.remove(&session_id);
        self.session_inboxes.remove(&session_id);
        self.task_bindings.retain(|_, sid| *sid != session_id);
        removed
    }

    /// Run a mutable update against the current task-bound session.
    pub fn with_current_session_mut<T>(
        &mut self,
        f: impl FnOnce(&mut ChoreographySessionState) -> T,
    ) -> Result<T, String> {
        let task_id = Self::current_binding_key();
        let session_id = self
            .task_bindings
            .get(&task_id)
            .copied()
            .ok_or_else(|| "no choreography session bound to current task".to_string())?;
        let Some(session) = self.sessions.get_mut(&session_id) else {
            self.task_bindings.remove(&task_id);
            return Err(format!(
                "missing choreography session state for bound session {session_id}"
            ));
        };
        Ok(f(session))
    }

    /// Check if a session is active
    pub fn is_active(&self) -> bool {
        self.current_session_id()
            .and_then(|session_id| self.sessions.get(&session_id))
            .is_some()
    }

    /// Check if the session has timed out
    pub fn is_timed_out(&self, now_ms: u64) -> bool {
        let Some(session) = self.current_session() else {
            return false;
        };
        match (session.started_at_ms, session.timeout_ms) {
            (Some(started), Some(timeout)) => now_ms.saturating_sub(started) > timeout,
            _ => false,
        }
    }

    /// Snapshot the inbox notifier for one active session.
    pub fn session_inbox_notify(
        &self,
        session_id: RuntimeChoreographySessionId,
    ) -> Option<Arc<Notify>> {
        self.session_inbox_notifiers.get(&session_id).cloned()
    }

    /// Wake any waiters blocked on one session-local inbox.
    pub fn notify_session_inbox(&self, session_id: RuntimeChoreographySessionId) {
        if let Some(notify) = self.session_inbox_notifiers.get(&session_id) {
            notify.notify_waiters();
        }
    }

    /// Queue one choreography envelope for one session-local inbox and wake waiters.
    pub fn queue_session_envelope(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        envelope: TransportEnvelope,
    ) {
        self.session_inboxes
            .entry(session_id)
            .or_default()
            .push(envelope);
        self.notify_session_inbox(session_id);
    }

    /// Remove one matching choreography envelope from one session-local inbox.
    pub fn take_matching_session_envelope(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        source: AuthorityId,
        context_id: ContextId,
        self_authority: AuthorityId,
        self_device_id: &str,
    ) -> Option<TransportEnvelope> {
        let inbox = self.session_inboxes.get_mut(&session_id)?;
        inbox
            .iter()
            .position(|env| {
                let device_match = env
                    .metadata
                    .get("aura-destination-device-id")
                    .is_some_and(|dst| dst == self_device_id);

                if env.destination == self_authority {
                    env.source == source
                        && env.context == context_id
                        && match env.metadata.get("aura-destination-device-id") {
                            Some(dst) => dst == self_device_id,
                            None => true,
                        }
                } else {
                    env.source == source && env.context == context_id && device_match
                }
            })
            .map(|pos| inbox.remove(pos))
    }

    /// Snapshot the current queued envelope count for one session-local inbox.
    pub fn session_inbox_len(&self, session_id: RuntimeChoreographySessionId) -> usize {
        self.session_inboxes
            .get(&session_id)
            .map_or(0, std::vec::Vec::len)
    }

    /// Clone the current queued envelopes for one session-local inbox.
    pub fn session_inbox_snapshot(
        &self,
        session_id: RuntimeChoreographySessionId,
    ) -> Vec<TransportEnvelope> {
        self.session_inboxes
            .get(&session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Claim authoritative ownership for one active session.
    pub fn claim_session_owner(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        owner_label: impl Into<String>,
    ) -> Result<SessionOwnerCapability, SessionOwnershipError> {
        let owner_label = owner_label.into();
        if !self.sessions.contains_key(&session_id) {
            return Err(SessionOwnershipError::MissingOwner { session_id });
        }

        if let Some(existing) = self.session_owners.get(&session_id) {
            if existing.owner_label != owner_label {
                return Err(SessionOwnershipError::OwnerConflict {
                    session_id,
                    existing_owner: existing.owner_label.clone(),
                    requested_owner: owner_label,
                });
            }
            return Ok(existing.capability.clone());
        }

        let generation = self
            .session_owner_generations
            .entry(session_id)
            .and_modify(|generation| *generation = generation.saturating_add(1))
            .or_insert(1);
        let capability =
            SessionOwnerCapability::full_session(session_id, owner_label.clone(), *generation);
        self.session_owners.insert(
            session_id,
            SessionOwnerRecord {
                owner_label,
                capability: capability.clone(),
            },
        );
        Ok(capability)
    }

    /// Ensure the requested owner still holds the active session.
    pub fn ensure_session_owner(
        &self,
        session_id: RuntimeChoreographySessionId,
        expected_capability: &SessionOwnerCapability,
    ) -> Result<(), SessionOwnershipError> {
        let Some(owner) = self.session_owners.get(&session_id) else {
            return Err(SessionOwnershipError::MissingOwner { session_id });
        };

        if owner.owner_label != expected_capability.owner_label {
            return Err(SessionOwnershipError::OwnerMismatch {
                session_id,
                expected_owner: expected_capability.owner_label.clone(),
            });
        }

        if owner.capability.generation != expected_capability.generation
            || owner.capability.scope != expected_capability.scope
        {
            return Err(SessionOwnershipError::CapabilityMismatch {
                session_id,
                expected_owner: expected_capability.owner_label.clone(),
                current_generation: owner.capability.generation,
            });
        }

        Ok(())
    }

    /// Release authoritative ownership for one active session.
    pub fn release_session_owner(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        expected_capability: &SessionOwnerCapability,
    ) -> Result<(), SessionOwnershipError> {
        self.ensure_session_owner(session_id, expected_capability)?;
        self.session_owners.remove(&session_id);
        Ok(())
    }

    /// Atomically transfer authoritative ownership for one active session.
    pub fn transfer_session_owner(
        &mut self,
        session_id: RuntimeChoreographySessionId,
        expected_capability: &SessionOwnerCapability,
        next_owner_label: impl Into<String>,
        next_scope: SessionOwnerCapabilityScope,
    ) -> Result<SessionOwnerCapability, SessionOwnershipError> {
        self.ensure_session_owner(session_id, expected_capability)?;

        let next_owner_label = next_owner_label.into();
        let generation = self
            .session_owner_generations
            .entry(session_id)
            .and_modify(|generation| *generation = generation.saturating_add(1))
            .or_insert(1);
        let next_capability =
            SessionOwnerCapability::full_session(session_id, next_owner_label.clone(), *generation)
                .with_scope(next_scope);
        self.session_owners.insert(
            session_id,
            SessionOwnerRecord {
                owner_label: next_owner_label,
                capability: next_capability.clone(),
            },
        );
        Ok(next_capability)
    }

    /// Snapshot the current owner for one active session.
    pub fn session_owner(
        &self,
        session_id: RuntimeChoreographySessionId,
    ) -> Option<&SessionOwnerRecord> {
        self.session_owners.get(&session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_choreography_session_id_bridges_aura_session_id_explicitly() {
        let aura_session_id = SessionId::new_from_entropy([9; 32]);
        let runtime_session_id =
            RuntimeChoreographySessionId::from_aura_session_id(aura_session_id);

        assert_eq!(runtime_session_id.into_aura_session_id(), aura_session_id);
    }

    #[test]
    fn session_notifier_tracks_session_lifecycle() {
        let authority_id = DeviceId::from_uuid(Uuid::from_bytes([4; 16]));
        let role = ChoreographicRole::new(
            authority_id,
            AuthorityId::new_from_entropy([0u8; 32]),
            RoleIndex::new(0).expect("role index"),
        );
        let session_id = RuntimeChoreographySessionId::from_uuid(Uuid::from_u128(44));
        let context_id = ContextId::new_from_entropy([7; 32]);
        let mut state = ChoreographyState::new();

        state
            .start_session(session_id, context_id, vec![role], role, Some(1000), 0)
            .expect("session starts");
        assert!(
            state.session_inbox_notify(session_id).is_some(),
            "active session should expose an inbox notifier"
        );

        state.end_session(10).expect("session ends");
        assert!(
            state.session_inbox_notify(session_id).is_none(),
            "ended session should release its inbox notifier"
        );
    }

    #[test]
    fn cancel_session_releases_bindings_and_inbox_state() {
        let authority_id = DeviceId::from_uuid(Uuid::from_bytes([5; 16]));
        let role = ChoreographicRole::new(
            authority_id,
            AuthorityId::new_from_entropy([0u8; 32]),
            RoleIndex::new(0).expect("role index"),
        );
        let session_id = RuntimeChoreographySessionId::from_uuid(Uuid::from_u128(45));
        let context_id = ContextId::new_from_entropy([8; 32]);
        let mut state = ChoreographyState::new();

        state
            .start_session(session_id, context_id, vec![role], role, Some(1000), 0)
            .expect("session starts");
        state.queue_session_envelope(
            session_id,
            TransportEnvelope {
                destination: AuthorityId::from_uuid(Uuid::from_bytes([5; 16])),
                source: AuthorityId::from_uuid(Uuid::from_bytes([6; 16])),
                context: context_id,
                payload: vec![1],
                metadata: std::collections::HashMap::new(),
                receipt: None,
            },
        );

        assert!(state.cancel_session(session_id));
        assert!(state.current_session_id().is_none());
        assert!(state.session_inbox_notify(session_id).is_none());
        assert_eq!(state.session_inbox_len(session_id), 0);
    }

    #[test]
    fn session_owner_conflict_is_rejected() {
        let authority_id = DeviceId::from_uuid(Uuid::from_bytes([7; 16]));
        let role = ChoreographicRole::new(
            authority_id,
            AuthorityId::new_from_entropy([1u8; 32]),
            RoleIndex::new(0).expect("role index"),
        );
        let session_id = RuntimeChoreographySessionId::from_uuid(Uuid::from_u128(46));
        let context_id = ContextId::new_from_entropy([9; 32]);
        let mut state = ChoreographyState::new();

        state
            .start_session(session_id, context_id, vec![role], role, Some(1000), 0)
            .expect("session starts");
        state
            .claim_session_owner(session_id, "owner-a")
            .expect("owner a claims session");

        assert!(matches!(
            state.claim_session_owner(session_id, "owner-b"),
            Err(SessionOwnershipError::OwnerConflict { .. })
        ));
    }

    #[test]
    fn ending_session_releases_owner_record() {
        let authority_id = DeviceId::from_uuid(Uuid::from_bytes([8; 16]));
        let role = ChoreographicRole::new(
            authority_id,
            AuthorityId::new_from_entropy([2u8; 32]),
            RoleIndex::new(0).expect("role index"),
        );
        let session_id = RuntimeChoreographySessionId::from_uuid(Uuid::from_u128(47));
        let context_id = ContextId::new_from_entropy([10; 32]);
        let mut state = ChoreographyState::new();

        state
            .start_session(session_id, context_id, vec![role], role, Some(1000), 0)
            .expect("session starts");
        let capability = state
            .claim_session_owner(session_id, "owner-a")
            .expect("owner claims session");
        state.end_session(10).expect("session ends");

        assert!(matches!(
            state.ensure_session_owner(session_id, &capability),
            Err(SessionOwnershipError::MissingOwner { .. })
        ));
    }

    #[test]
    fn reclaiming_owner_invalidates_stale_capability_generation() {
        let authority_id = DeviceId::from_uuid(Uuid::from_bytes([9; 16]));
        let role = ChoreographicRole::new(
            authority_id,
            AuthorityId::new_from_entropy([3u8; 32]),
            RoleIndex::new(0).expect("role index"),
        );
        let session_id = RuntimeChoreographySessionId::from_uuid(Uuid::from_u128(48));
        let context_id = ContextId::new_from_entropy([11; 32]);
        let mut state = ChoreographyState::new();

        state
            .start_session(session_id, context_id, vec![role], role, Some(1000), 0)
            .expect("session starts");
        let first = state
            .claim_session_owner(session_id, "owner-a")
            .expect("owner claims session");
        state
            .release_session_owner(session_id, &first)
            .expect("owner releases session");
        let second = state
            .claim_session_owner(session_id, "owner-a")
            .expect("owner reclaims session");

        assert!(second.generation > first.generation);
        assert!(matches!(
            state.ensure_session_owner(session_id, &first),
            Err(SessionOwnershipError::CapabilityMismatch { .. })
        ));
        assert!(state.ensure_session_owner(session_id, &second).is_ok());
    }

    #[test]
    fn transfer_session_owner_is_atomic_and_invalidates_old_capability() {
        let authority_id = DeviceId::from_uuid(Uuid::from_bytes([10; 16]));
        let role = ChoreographicRole::new(
            authority_id,
            AuthorityId::new_from_entropy([4u8; 32]),
            RoleIndex::new(0).expect("role index"),
        );
        let session_id = RuntimeChoreographySessionId::from_uuid(Uuid::from_u128(49));
        let context_id = ContextId::new_from_entropy([12; 32]);
        let mut state = ChoreographyState::new();

        state
            .start_session(session_id, context_id, vec![role], role, Some(1000), 0)
            .expect("session starts");
        let original = state
            .claim_session_owner(session_id, "owner-a")
            .expect("owner claims session");
        let transferred = state
            .transfer_session_owner(
                session_id,
                &original,
                "owner-b",
                SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
                    "fragment.alpha".to_string()
                ])),
            )
            .expect("ownership transfers");

        assert_eq!(transferred.owner_label, "owner-b");
        assert!(matches!(
            transferred.scope,
            SessionOwnerCapabilityScope::Fragments(_)
        ));
        assert!(matches!(
            state.ensure_session_owner(session_id, &original),
            Err(SessionOwnershipError::OwnerMismatch { .. })
                | Err(SessionOwnershipError::CapabilityMismatch { .. })
        ));
        assert!(state.ensure_session_owner(session_id, &transferred).is_ok());
    }

    #[test]
    fn duplicate_session_start_is_typed() {
        let authority_id = DeviceId::from_uuid(Uuid::from_bytes([11; 16]));
        let role = ChoreographicRole::new(
            authority_id,
            AuthorityId::new_from_entropy([5u8; 32]),
            RoleIndex::new(0).expect("role index"),
        );
        let session_id = RuntimeChoreographySessionId::from_uuid(Uuid::from_u128(50));
        let context_id = ContextId::new_from_entropy([13; 32]);
        let mut state = ChoreographyState::new();

        state
            .start_session(session_id, context_id, vec![role], role, Some(1000), 0)
            .expect("session starts");
        state.task_bindings.clear();

        assert_eq!(
            state
                .start_session(session_id, context_id, vec![role], role, Some(1000), 1)
                .expect_err("duplicate session should be rejected"),
            SessionStartError::SessionAlreadyExists { session_id }
        );
    }
}
