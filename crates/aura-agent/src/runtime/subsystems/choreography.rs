//! Choreography State
//!
//! In-memory choreography session state for runtime coordination.

use aura_core::{ContextId, DeviceId};
use aura_protocol::effects::{ChoreographicRole, ChoreographyMetrics, RoleIndex};
use std::collections::HashMap;
use std::thread::ThreadId;
use tokio::task::Id as TaskId;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ExecutionBindingKey {
    Task(TaskId),
    Thread(ThreadId),
}

/// In-memory choreography session state for one runtime session.
#[derive(Debug, Clone)]
pub struct ChoreographySessionState {
    /// Current session ID.
    pub session_id: Uuid,
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
        session_id: Uuid,
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
    sessions: HashMap<Uuid, ChoreographySessionState>,
    task_bindings: HashMap<ExecutionBindingKey, Uuid>,
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
            session_id: Uuid::nil(),
            context_id: ContextId::new_from_entropy([0; 32]),
            roles: Vec::new(),
            current_role: ChoreographicRole::new(
                DeviceId::from_uuid(Uuid::nil()),
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
    pub fn current_session_id(&self) -> Option<Uuid> {
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
        session_id: Uuid,
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
        self.task_bindings.insert(task_id, session_id);
        Ok(())
    }

    /// End the current task-bound session and clean up all session bindings.
    pub fn end_session(&mut self, now_ms: u64) -> Result<Uuid, String> {
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
        self.task_bindings.retain(|_, sid| *sid != session_id);
        Ok(session_id)
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
}
