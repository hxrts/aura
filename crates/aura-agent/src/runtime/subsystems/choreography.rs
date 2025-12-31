//! Choreography Subsystem
//!
//! Groups choreography-related fields from AuraEffectSystem:
//! - `choreography_state`: In-memory session state for runtime coordination
//! - `composite`: Composite handler adapter for handler registration
//!
//! ## Lock Usage
//!
//! Uses `parking_lot::RwLock` for `choreography_state` because:
//! - Session state is accessed synchronously for quick reads/writes
//! - State transitions are atomic and brief
//! - See `runtime/CONCURRENCY.md` for full rationale

#![allow(clippy::disallowed_types)]

use aura_composition::CompositeHandlerAdapter;
use aura_core::ContextId;
use aura_protocol::effects::{ChoreographicRole, ChoreographyMetrics};
use parking_lot::RwLock;
use uuid::Uuid;

/// In-memory choreography session state
///
/// Note: Infrastructure for future choreography integration.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ChoreographyState {
    /// Current session ID (if active)
    pub session_id: Option<Uuid>,
    /// Context ID for this session
    pub context_id: Option<ContextId>,
    /// Roles participating in this choreography
    pub roles: Vec<ChoreographicRole>,
    /// This node's current role
    pub current_role: Option<ChoreographicRole>,
    /// Session timeout in milliseconds
    pub timeout_ms: Option<u64>,
    /// Session start time in milliseconds since epoch
    pub started_at_ms: Option<u64>,
    /// Session metrics
    pub metrics: ChoreographyMetrics,
}

impl Default for ChoreographyState {
    fn default() -> Self {
        Self {
            session_id: None,
            context_id: None,
            roles: Vec::new(),
            current_role: None,
            timeout_ms: None,
            started_at_ms: None,
            metrics: ChoreographyMetrics {
                messages_sent: 0,
                messages_received: 0,
                avg_latency_ms: 0.0,
                timeout_count: 0,
                retry_count: 0,
                total_duration_ms: 0,
            },
        }
    }
}

#[allow(dead_code)]
impl ChoreographyState {
    /// Create a new empty state
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new session
    pub fn start_session(
        &mut self,
        session_id: Uuid,
        context_id: ContextId,
        roles: Vec<ChoreographicRole>,
        current_role: ChoreographicRole,
        timeout_ms: Option<u64>,
        now_ms: u64,
    ) {
        self.session_id = Some(session_id);
        self.context_id = Some(context_id);
        self.roles = roles;
        self.current_role = Some(current_role);
        self.timeout_ms = timeout_ms;
        self.started_at_ms = Some(now_ms);
        self.reset_metrics();
    }

    /// End the current session
    pub fn end_session(&mut self, now_ms: u64) {
        if let Some(started) = self.started_at_ms {
            self.metrics.total_duration_ms = now_ms.saturating_sub(started);
        }
        self.session_id = None;
        self.context_id = None;
        self.current_role = None;
        self.started_at_ms = None;
    }

    /// Reset metrics to default values
    pub fn reset_metrics(&mut self) {
        self.metrics = ChoreographyMetrics {
            messages_sent: 0,
            messages_received: 0,
            avg_latency_ms: 0.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 0,
        };
    }

    /// Check if a session is active
    pub fn is_active(&self) -> bool {
        self.session_id.is_some()
    }

    /// Check if the session has timed out
    pub fn is_timed_out(&self, now_ms: u64) -> bool {
        match (self.started_at_ms, self.timeout_ms) {
            (Some(started), Some(timeout)) => now_ms.saturating_sub(started) > timeout,
            _ => false,
        }
    }

    /// Record a message sent
    pub fn record_message_sent(&mut self) {
        self.metrics.messages_sent += 1;
    }

    /// Record a message received
    pub fn record_message_received(&mut self) {
        self.metrics.messages_received += 1;
    }

    /// Record a timeout
    pub fn record_timeout(&mut self) {
        self.metrics.timeout_count += 1;
    }

    /// Record a retry
    pub fn record_retry(&mut self) {
        self.metrics.retry_count += 1;
    }
}

/// Choreography subsystem grouping session state and handler composition.
///
/// This subsystem encapsulates:
/// - In-memory session state for choreography coordination
/// - Composite handler adapter for effect registration
///
/// Note: Infrastructure for future integration into AuraEffectSystem.
#[allow(dead_code)]
pub struct ChoreographySubsystem {
    /// In-memory choreography session state
    ///
    /// Protected by parking_lot::RwLock for concurrent access.
    /// Lock is never held across .await points.
    state: RwLock<ChoreographyState>,

    /// Composite handler adapter for handler registration
    composite: CompositeHandlerAdapter,
}

#[allow(dead_code)]
impl ChoreographySubsystem {
    /// Create a new choreography subsystem
    pub fn new(composite: CompositeHandlerAdapter) -> Self {
        Self {
            state: RwLock::new(ChoreographyState::new()),
            composite,
        }
    }

    /// Get reference to the composite handler
    pub fn composite(&self) -> &CompositeHandlerAdapter {
        &self.composite
    }

    /// Get mutable reference to the composite handler
    pub fn composite_mut(&mut self) -> &mut CompositeHandlerAdapter {
        &mut self.composite
    }

    /// Execute a function with read access to the state
    pub fn with_state<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ChoreographyState) -> R,
    {
        let state = self.state.read();
        f(&state)
    }

    /// Execute a function with write access to the state
    pub fn with_state_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ChoreographyState) -> R,
    {
        let mut state = self.state.write();
        f(&mut state)
    }

    /// Check if a session is active
    pub fn is_session_active(&self) -> bool {
        self.state.read().is_active()
    }

    /// Get the current session ID
    pub fn session_id(&self) -> Option<Uuid> {
        self.state.read().session_id
    }

    /// Get the current role
    pub fn current_role(&self) -> Option<ChoreographicRole> {
        self.state.read().current_role
    }

    /// Get current metrics snapshot
    pub fn metrics(&self) -> ChoreographyMetrics {
        self.state.read().metrics.clone()
    }

    /// Start a new choreography session
    pub fn start_session(
        &self,
        session_id: Uuid,
        context_id: ContextId,
        roles: Vec<ChoreographicRole>,
        current_role: ChoreographicRole,
        timeout_ms: Option<u64>,
        now_ms: u64,
    ) {
        self.state.write().start_session(
            session_id,
            context_id,
            roles,
            current_role,
            timeout_ms,
            now_ms,
        );
    }

    /// End the current session
    pub fn end_session(&self, now_ms: u64) {
        self.state.write().end_session(now_ms);
    }
}

// Note: ChoreographySubsystem is intentionally not Clone because
// CompositeHandlerAdapter does not implement Clone. The subsystem
// should be wrapped in Arc when shared.

impl std::fmt::Debug for ChoreographySubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.read();
        f.debug_struct("ChoreographySubsystem")
            .field("session_id", &state.session_id)
            .field("current_role", &state.current_role)
            .field("is_active", &state.is_active())
            .field("composite", &"<CompositeHandlerAdapter>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_role() -> ChoreographicRole {
        ChoreographicRole {
            device_id: Uuid::new_v4(),
            role_index: 0,
        }
    }

    #[test]
    fn test_choreography_state_lifecycle() {
        let mut state = ChoreographyState::new();
        assert!(!state.is_active());

        let session_id = Uuid::new_v4();
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let role = test_role();

        state.start_session(session_id, context_id, vec![role.clone()], role, Some(5000), 1000);
        assert!(state.is_active());
        assert!(!state.is_timed_out(3000));
        assert!(state.is_timed_out(7000));

        state.end_session(2000);
        assert!(!state.is_active());
        assert_eq!(state.metrics.total_duration_ms, 1000);
    }

    #[test]
    fn test_choreography_state_metrics() {
        let mut state = ChoreographyState::new();

        state.record_message_sent();
        state.record_message_sent();
        state.record_message_received();
        state.record_timeout();
        state.record_retry();

        assert_eq!(state.metrics.messages_sent, 2);
        assert_eq!(state.metrics.messages_received, 1);
        assert_eq!(state.metrics.timeout_count, 1);
        assert_eq!(state.metrics.retry_count, 1);
    }
}
