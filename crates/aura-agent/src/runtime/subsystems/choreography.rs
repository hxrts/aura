//! Choreography State
//!
//! In-memory choreography session state for runtime coordination.

use aura_core::ContextId;
use aura_protocol::effects::{ChoreographicRole, ChoreographyMetrics};
use uuid::Uuid;

/// In-memory choreography session state
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
