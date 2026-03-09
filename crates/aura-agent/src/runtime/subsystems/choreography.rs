//! Choreography State
//!
//! In-memory choreography session state for runtime coordination.

use aura_core::effects::transport::TransportEnvelope;
use aura_core::{AuthorityId, ContextId, DeviceId, SessionId};
use aura_protocol::effects::{ChoreographicRole, ChoreographyMetrics, RoleIndex};
use std::collections::HashMap;
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
    ) -> Result<(), String> {
        let task_id = Self::current_binding_key();
        if let Some(existing_session_id) = self.task_bindings.get(&task_id).copied() {
            if self.sessions.contains_key(&existing_session_id) {
                return Err(format!(
                    "task already bound to active choreography session {existing_session_id}"
                ));
            }
            self.task_bindings.remove(&task_id);
        }
        if self.sessions.contains_key(&session_id) {
            return Err(format!("choreography session already exists: {session_id}"));
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
        let role = ChoreographicRole::new(authority_id, RoleIndex::new(0).expect("role index"));
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
        let role = ChoreographicRole::new(authority_id, RoleIndex::new(0).expect("role index"));
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
}
